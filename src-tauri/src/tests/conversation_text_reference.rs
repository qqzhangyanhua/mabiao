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
