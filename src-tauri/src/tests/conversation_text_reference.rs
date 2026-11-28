//! 正文外置与 `event_id` 推导存储（ADR 0025）。

use crate::test_support::*;

const SEMANTIC_PATH: &str = ".codex/sessions/2026/08/rollout-semantic-1.jsonl";

fn seed(home: &std::path::Path) -> (rusqlite::Connection, std::path::PathBuf) {
    let path = write_home_fixture(home, SEMANTIC_PATH, "codex-semantic-events.jsonl");
    let conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();
    (conn, path)
}

struct StoredRow {
    source_sequence: u32,
    kind: String,
    event_id: String,
    text: Option<String>,
    text_hash: Option<i64>,
}

fn stored_rows(conn: &rusqlite::Connection) -> Vec<StoredRow> {
    conn.prepare(
        "SELECT e.source_sequence, e.kind, e.event_id, e.text, e.text_hash
         FROM conversation_events AS e
         JOIN conversation_sessions AS s
           ON s.source = e.source
          AND s.session_id = e.session_id
          AND s.event_index_generation = e.index_generation
         WHERE e.session_id = 'semantic-1'
         ORDER BY e.sequence",
    )
    .unwrap()
    .query_map([], |row| {
        Ok(StoredRow {
            source_sequence: row.get(0)?,
            kind: row.get(1)?,
            event_id: row.get(2)?,
            text: row.get(3)?,
            text_hash: row.get(4)?,
        })
    })
    .unwrap()
    .collect::<Result<_, _>>()
    .unwrap()
}

fn row_at(rows: &[StoredRow], source_sequence: u32) -> &StoredRow {
    rows.iter()
        .find(|row| row.source_sequence == source_sequence)
        .unwrap_or_else(|| panic!("缺少源文件第 {source_sequence} 行的事件"))
}

fn adapter_version(conn: &rusqlite::Connection) -> i64 {
    conn.query_row(
        "SELECT adapter_version FROM conversation_sessions WHERE session_id = 'semantic-1'",
        [],
        |row| row.get(0),
    )
    .unwrap()
}

/// 模拟 ADR 0025 之前写下的行：完整 `event_id`、正文在库里。
fn inline_as_legacy(conn: &rusqlite::Connection) {
    let events = crate::conversation::indexed_events(conn, "codex", "semantic-1").unwrap();
    for event in events {
        conn.execute(
            "UPDATE conversation_events
             SET event_id = ?1, text = ?2, line_offset = NULL, text_hash = NULL
             WHERE session_id = 'semantic-1' AND sequence = ?3",
            rusqlite::params![event.event_id, event.text, event.sequence],
        )
        .unwrap();
    }
}

#[test]
fn line_reconstructable_texts_are_stored_by_reference() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let (conn, _) = seed(home);

    let rows = stored_rows(&conn);
    for (line, kind) in [(2, "message"), (7, "plan"), (9, "tool_result")] {
        let row = row_at(&rows, line);
        assert_eq!(row.kind, kind);
        assert_eq!(row.text, None, "第 {line} 行的正文应外置");
        assert!(row.text_hash.is_some(), "第 {line} 行应留指纹");
    }
    let merged = row_at(&rows, 3);
    assert_eq!(
        merged.text.as_deref(),
        Some("我先检查现有实现。"),
        "跨行合并的正文按行重建不出来，必须留在库里"
    );
    assert_eq!(merged.text_hash, None);
    for line in [8, 11] {
        let row = row_at(&rows, line);
        assert!(row.text.is_some(), "{} 的正文留在库里", row.kind);
        assert_eq!(row.text_hash, None);
    }
    assert!(
        rows.iter().all(|row| row.event_id.is_empty()),
        "可推导的 event_id 不逐行存"
    );

    assert_conversation_index_matches_parse(&conn, home, "codex", "semantic-1");
}

#[test]
fn rewritten_source_line_falls_back_to_full_parse() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let (conn, path) = seed(home);

    let original = std::fs::read_to_string(&path).unwrap();
    let rewritten = original.replace("实现语义时间线", "实现语义时间轴");
    assert_eq!(
        rewritten.len(),
        original.len(),
        "同长改写，偏移不变只靠指纹发现"
    );
    std::fs::write(&path, rewritten).unwrap();

    let events = crate::conversation::indexed_events(&conn, "codex", "semantic-1").unwrap();
    assert!(
        events
            .iter()
            .any(|event| event.text.as_deref() == Some("实现语义时间轴")),
        "指纹对不上时必须退回整份解析，不得给出旧正文或空正文"
    );
    assert!(events
        .iter()
        .all(|event| event.text.as_deref() != Some("实现语义时间线")));
}

#[test]
fn adopting_legacy_rows_compacts_ids_and_backfill_moves_texts_out() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let (conn, _) = seed(home);
    let expected = crate::conversation::indexed_events(&conn, "codex", "semantic-1").unwrap();
    inline_as_legacy(&conn);
    assert!(stored_rows(&conn)
        .iter()
        .all(|row| !row.event_id.is_empty()));

    crate::conversation::adopt_text_references(&conn).unwrap();

    assert!(
        stored_rows(&conn).iter().all(|row| row.event_id.is_empty()),
        "接管后已有行改存推导形态"
    );
    assert_eq!(adapter_version(&conn), store::STORAGE_STALE_ADAPTER_VERSION);
    assert_eq!(
        crate::conversation::indexed_events(&conn, "codex", "semantic-1").unwrap(),
        expected,
        "待补建期间读回不变"
    );

    assert_eq!(
        crate::conversation::backfill_event_index(&conn, home).unwrap(),
        1
    );
    assert_ne!(adapter_version(&conn), store::STORAGE_STALE_ADAPTER_VERSION);
    assert!(row_at(&stored_rows(&conn), 2).text_hash.is_some());
    assert_eq!(
        crate::conversation::indexed_events(&conn, "codex", "semantic-1").unwrap(),
        expected
    );
}

#[test]
fn ready_session_text_ref_mismatch_falls_back_on_general_readers_and_preserves_deferred_policy() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let big_output = format!("{}DEFERRED-END-MARKER", "B".repeat(5_000));
    let path = home.join(".codex/sessions/2026/08/rollout-defer-1.jsonl");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let lines = [
        serde_json::json!({
            "type": "session_meta",
            "timestamp": "2026-08-20T00:00:00Z",
            "payload": {"id": "defer-1", "cwd": "/workspace/app", "model_provider": "openai"}
        }),
        serde_json::json!({
            "type": "response_item",
            "timestamp": "2026-08-20T00:00:01Z",
            "payload": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "检查大输出"}]}
        }),
        serde_json::json!({
            "type": "response_item",
            "timestamp": "2026-08-20T00:00:02Z",
            "payload": {"type": "function_call", "name": "Bash", "arguments": "{}", "call_id": "call-1"}
        }),
        serde_json::json!({
            "type": "response_item",
            "timestamp": "2026-08-20T00:00:03Z",
            "payload": {"type": "function_call_output", "call_id": "call-1", "output": big_output}
        }),
        serde_json::json!({
            "type": "response_item",
            "timestamp": "2026-08-20T00:00:04Z",
            "payload": {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "检查完成"}]}
        }),
    ];
    let body = lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&path, format!("{body}\n")).unwrap();

    let conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();

    // 正常就绪态：分页保持 Deferred；单条展开、导出、推送展开全文。
    let ready_page = crate::conversation::load_events(
        &conn,
        home,
        "codex",
        "defer-1",
        crate::domain::ConversationEventAnchor::First,
        50,
    )
    .unwrap();
    let deferred_event = ready_page
        .events
        .iter()
        .find(|event| event.content_status == ConversationEventContentStatus::Deferred)
        .expect("分页应保留 >4KB 工具输出为 Deferred");
    assert!(
        !deferred_event
            .text
            .as_deref()
            .unwrap_or("")
            .contains("DEFERRED-END-MARKER"),
        "分页不应展开延迟正文"
    );
    let expanded = crate::conversation::load_event_content(
        &conn,
        home,
        "codex",
        "defer-1",
        &deferred_event.event_id,
    )
    .unwrap();
    assert!(expanded
        .text
        .as_deref()
        .unwrap_or("")
        .contains("DEFERRED-END-MARKER"));

    let exported = crate::conversation::build_export(
        &conn,
        home,
        "codex",
        "defer-1",
        ConversationExportFormat::Markdown,
    )
    .unwrap();
    assert!(String::from_utf8(exported.content)
        .unwrap()
        .contains("DEFERRED-END-MARKER"));

    let row = crate::conversation::load_session(&conn, "codex", "defer-1")
        .unwrap()
        .unwrap();
    let pushed = crate::conversation::read_push_session(&conn, home, &row).unwrap();
    assert!(pushed.events.iter().any(|event| event
        .text
        .as_deref()
        .unwrap_or("")
        .contains("DEFERRED-END-MARKER")));

    // 破坏外置正文指纹而保持源文件不动（索引仍处于就绪态）：
    // 常规读取方（分页、单条展开、导出）走整份解析回退；推送按 ADR-0026 整场跳过。
    let changed = conn
        .execute(
            "UPDATE conversation_events SET text_hash = text_hash + 1
             WHERE source = 'codex' AND session_id = 'defer-1' AND text_hash IS NOT NULL",
            [],
        )
        .unwrap();
    assert!(changed > 0);
    let prepared = crate::conversation::prepare_detail(&conn, "codex", "defer-1").unwrap();
    assert!(crate::conversation::event_index_ready(&conn, home, &prepared).unwrap());

    let fallback_page = crate::conversation::load_events(
        &conn,
        home,
        "codex",
        "defer-1",
        crate::domain::ConversationEventAnchor::First,
        50,
    )
    .unwrap();
    assert!(fallback_page
        .events
        .iter()
        .any(|event| event.text.as_deref() == Some("检查大输出")));
    let fallback_deferred = fallback_page
        .events
        .iter()
        .find(|event| event.content_status == ConversationEventContentStatus::Deferred)
        .expect("分页回退也应保持 >4KB 工具输出为 Deferred");
    assert!(!fallback_deferred
        .text
        .as_deref()
        .unwrap_or("")
        .contains("DEFERRED-END-MARKER"));

    let fallback_expanded = crate::conversation::load_event_content(
        &conn,
        home,
        "codex",
        "defer-1",
        &deferred_event.event_id,
    )
    .unwrap();
    assert!(fallback_expanded
        .text
        .as_deref()
        .unwrap_or("")
        .contains("DEFERRED-END-MARKER"));

    let fallback_exported = crate::conversation::build_export(
        &conn,
        home,
        "codex",
        "defer-1",
        ConversationExportFormat::Markdown,
    )
    .unwrap();
    assert!(String::from_utf8(fallback_exported.content)
        .unwrap()
        .contains("DEFERRED-END-MARKER"));

    let push_err = crate::conversation::read_push_session(&conn, home, &row)
        .err()
        .expect("推送在已就绪外置正文失配时必须整场跳过");
    assert!(push_err.contains("外置正文"), "{push_err}");
}
