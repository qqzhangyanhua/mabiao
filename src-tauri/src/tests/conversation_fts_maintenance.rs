//! 对话派生缓存的持续维护（ADR 0024）。

use crate::test_support::*;

/// 收窄后的更新触发器只在 `text` / `name` 变化时重写倒排。重排 `sequence` 不应增加
/// 删除计数器——这是换代时 `finalize_session_events` 逐行做的操作。
#[test]
fn sequence_update_does_not_touch_fts() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    write_codex_session(
        home,
        "rollout.jsonl",
        "conv-seq",
        "title here",
        "body text searchable",
    );
    let conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();

    // 删除计数应该是 0（refresh 只插入+删除旧代，但这里是新会话没有旧代）
    let before: i64 = conn
        .query_row(
            "SELECT deleted_since_optimize FROM conversation_fts_maintenance WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    // UPDATE sequence 不触发倒排重写，所以计数器不应增长
    conn.execute(
        "UPDATE conversation_events SET sequence = 999 WHERE source = 'codex' AND session_id = 'conv-seq'",
        [],
    )
    .unwrap();
    let after: i64 = conn
        .query_row(
            "SELECT deleted_since_optimize FROM conversation_fts_maintenance WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(before, after, "重排 sequence 不应增加删除计数");
}

/// 换成不存原文的倒排之前，老形态库的触发器是 `AFTER UPDATE ON conversation_events`
/// （不收窄列）。重新打开后应该自动替换成 `AFTER UPDATE OF text, name`，且仍是回表形态。
#[test]
fn legacy_triggers_replaced_on_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("usage.sqlite");
    let conn = store::open_db(db_path.to_str().unwrap()).unwrap();
    conn.execute_batch(
        r#"
        DROP TRIGGER conversation_events_ad;
        DROP TABLE conversation_events_fts;
        CREATE VIRTUAL TABLE conversation_events_fts USING fts5(
            text, name, content='conversation_events', content_rowid='rowid',
            tokenize='trigram', detail='none', columnsize=0
        );
        CREATE TRIGGER conversation_events_au AFTER UPDATE ON conversation_events BEGIN
            INSERT INTO conversation_events_fts(conversation_events_fts, rowid, text, name)
            VALUES ('delete', old.rowid, COALESCE(old.text, ''), COALESCE(old.name, ''));
            INSERT INTO conversation_events_fts(rowid, text, name)
            VALUES (new.rowid, COALESCE(new.text, ''), COALESCE(new.name, ''));
        END;
        "#,
    )
    .unwrap();
    drop(conn);

    // 重新打开，init_schema 应该检测到旧触发器并替换
    let reopened = store::open_db(db_path.to_str().unwrap()).unwrap();
    let trigger_sql: String = reopened
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'trigger' AND name = 'conversation_events_au'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        trigger_sql.contains("UPDATE OF text, name"),
        "触发器应收窄为 UPDATE OF text, name，实际：{trigger_sql}"
    );
    let delete_sql: String = reopened
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'trigger' AND name = 'conversation_events_ad'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        delete_sql.contains("conversation_fts_maintenance") && delete_sql.contains("'delete'"),
        "老形态的删除触发器应回表删并计数删除量，实际：{delete_sql}"
    );
    assert!(store::conversation_fts_needs_migration(&reopened).unwrap());
}

/// 不存原文的倒排只有删除触发器：插入由写入方显式做，删除按 rowid 删。
#[test]
fn contentless_fts_has_only_delete_trigger() {
    let conn = store::open_memory().unwrap();
    assert!(store::conversation_fts_is_contentless(&conn).unwrap());
    let triggers: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'trigger' AND tbl_name = 'conversation_events' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(triggers, vec!["conversation_events_ad".to_string()]);
}

/// `conversation_fts_needs_optimize` 在没有维护记录时（老库升级、从备份恢复）返回 true，
/// 有记录且删除量低于门槛时返回 false。门槛 = max(上次行数 × 50%, 100000)。
#[test]
fn needs_optimize_threshold() {
    let conn = store::open_memory().unwrap();
    // 刚打开，rows_at_optimize = NULL → 需要
    assert!(store::conversation_fts_needs_optimize(&conn).unwrap());

    // 设基准：100 行，0 删除 → 不需要
    conn.execute(
        r#"UPDATE conversation_fts_maintenance
           SET deleted_since_optimize = 0, rows_at_optimize = 100 WHERE id = 1"#,
        [],
    )
    .unwrap();
    assert!(!store::conversation_fts_needs_optimize(&conn).unwrap());

    // 删除 49 行：49 < max(50, 100000) = 100000 → 不需要
    conn.execute(
        r#"UPDATE conversation_fts_maintenance SET deleted_since_optimize = 49 WHERE id = 1"#,
        [],
    )
    .unwrap();
    assert!(!store::conversation_fts_needs_optimize(&conn).unwrap());

    // 删除 100000 行：100000 >= max(50, 100000) = 100000 → 需要
    conn.execute(
        r#"UPDATE conversation_fts_maintenance SET deleted_since_optimize = 100000 WHERE id = 1"#,
        [],
    )
    .unwrap();
    assert!(store::conversation_fts_needs_optimize(&conn).unwrap());

    // 基准 300000 行，删 149999 行：149999 < max(150000, 100000) = 150000 → 不需要
    conn.execute(
        r#"UPDATE conversation_fts_maintenance
           SET deleted_since_optimize = 149999, rows_at_optimize = 300000 WHERE id = 1"#,
        [],
    )
    .unwrap();
    assert!(!store::conversation_fts_needs_optimize(&conn).unwrap());

    // 删 150000 行：150000 >= 150000 → 需要
    conn.execute(
        r#"UPDATE conversation_fts_maintenance SET deleted_since_optimize = 150000 WHERE id = 1"#,
        [],
    )
    .unwrap();
    assert!(store::conversation_fts_needs_optimize(&conn).unwrap());
}

/// `vacuum_is_due` 纯函数：空闲页 ≥ 256 MB 且 ≥ 20% 时返回 true。
#[test]
fn vacuum_threshold_logic() {
    use crate::store::conversation_fts::vacuum_is_due;

    const PAGE: i64 = 4096;
    const MIN_PAGES: i64 = (256 * 1024 * 1024) / PAGE;

    // 空闲不够 256 MB → false
    assert!(!vacuum_is_due(PAGE, MIN_PAGES * 2, MIN_PAGES - 1));

    // 空闲够 256 MB 但占比 < 20% → false
    assert!(!vacuum_is_due(PAGE, MIN_PAGES * 10, MIN_PAGES));

    // 真实库重灌倒排后：2.2 GB 里腾出约 500 MB → true
    assert!(vacuum_is_due(PAGE, MIN_PAGES * 9, MIN_PAGES * 2));

    // 空闲够 256 MB 且占比刚好 20% → true
    assert!(vacuum_is_due(PAGE, MIN_PAGES * 10, MIN_PAGES * 2));
}

/// `compact_conversation_fts` 执行后，删除计数清零、记下当前行数，正文仍能搜到。
#[test]
fn compact_resets_counter() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    write_codex_session(home, "rollout.jsonl", "conv-opt", "title", "body text");
    let conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();

    // 模拟足够多删除以达到门槛
    conn.execute(
        "UPDATE conversation_fts_maintenance SET deleted_since_optimize = 200000, rows_at_optimize = 300000 WHERE id = 1",
        [],
    )
    .unwrap();
    assert!(store::conversation_fts_needs_optimize(&conn).unwrap());

    store::compact_conversation_fts(&conn).unwrap();

    let (deleted, rows): (i64, Option<i64>) = conn
        .query_row(
            "SELECT deleted_since_optimize, rows_at_optimize FROM conversation_fts_maintenance WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(deleted, 0);
    assert!(rows.is_some_and(|r| r > 0));
    assert!(!store::conversation_fts_needs_optimize(&conn).unwrap());
    let hits: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM conversation_events_fts WHERE conversation_events_fts MATCH '\"ext\"'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(hits, 1, "整理后正文 body text 仍应可搜");
}

/// 维护方式改成重灌之前就记过基准的老库，升级后要清掉基准、尽快重灌一次；只清这一次。
#[test]
fn legacy_maintenance_baseline_cleared_once() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("usage.sqlite");
    let conn = store::open_db(db_path.to_str().unwrap()).unwrap();
    conn.execute_batch(
        "UPDATE conversation_fts_maintenance SET rows_at_optimize = 100 WHERE id = 1;
         PRAGMA user_version = 1;",
    )
    .unwrap();
    drop(conn);

    let upgraded = store::open_db(db_path.to_str().unwrap()).unwrap();
    assert!(store::conversation_fts_needs_optimize(&upgraded).unwrap());
    upgraded
        .execute(
            "UPDATE conversation_fts_maintenance SET rows_at_optimize = 100 WHERE id = 1",
            [],
        )
        .unwrap();
    drop(upgraded);

    let reopened = store::open_db(db_path.to_str().unwrap()).unwrap();
    assert!(!store::conversation_fts_needs_optimize(&reopened).unwrap());
}

/// 备份不携带维护表。
#[test]
fn backup_excludes_maintenance_table() {
    let dir = tempfile::tempdir().unwrap();
    let live = dir.path().join("live");
    std::fs::create_dir_all(&live).unwrap();
    let db_path = live.join("usage.sqlite");
    let conn = store::open_db(db_path.to_str().unwrap()).unwrap();
    conn.execute(
        "UPDATE conversation_fts_maintenance SET deleted_since_optimize = 99, rows_at_optimize = 100 WHERE id = 1",
        [],
    )
    .unwrap();
    drop(conn);

    let dest = dir.path().join("backup");
    let conn = store::open_db(db_path.to_str().unwrap()).unwrap();
    let manifest = crate::backup::backup_to(
        &conn,
        &dest,
        &crate::backup::AppDataPaths {
            db_path: db_path.clone(),
            prices_path: live.join("prices.json"),
            snapshot_path: live.join("litellm_prices.json"),
            budget_path: live.join("budget.json"),
            budget_notify_path: live.join("budget_notify_state.json"),
            official_quota_path: live.join("official_quota.json"),
            official_quota_notify_path: live.join("official_quota_notify_state.json"),
        },
    )
    .unwrap();
    drop(conn);
    assert!(manifest.files.contains(&"usage.sqlite".to_string()));

    let restored = store::open_db(dest.join("usage.sqlite").to_str().unwrap()).unwrap();
    let rows: Option<i64> = restored
        .query_row(
            "SELECT rows_at_optimize FROM conversation_fts_maintenance WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap_or(None);
    assert!(rows.is_none(), "备份不应携带维护记录");
}

/// 索引占用读存储值而非每次跑 dbstat。
#[test]
fn index_progress_reads_stored_bytes() {
    let conn = store::open_memory().unwrap();
    let progress = crate::conversation::event_index_progress(&conn).unwrap();
    assert_eq!(progress.index_bytes, 0);

    conn.execute(
        "UPDATE conversation_fts_maintenance SET index_bytes = 12345678 WHERE id = 1",
        [],
    )
    .unwrap();
    let progress = crate::conversation::event_index_progress(&conn).unwrap();
    assert_eq!(progress.index_bytes, 12345678);
}

// ── 辅助 ──────────────────────────────────────────────────────────────────

fn write_codex_session(
    home: &std::path::Path,
    file_name: &str,
    session_id: &str,
    title: &str,
    body: &str,
) {
    let records = [
        serde_json::json!({
            "type": "session_meta",
            "timestamp": "2026-08-20T00:00:00Z",
            "payload": {"id": session_id, "cwd": "/workspace/example-project", "model_provider": "openai"}
        }),
        serde_json::json!({
            "type": "turn_context",
            "timestamp": "2026-08-20T00:00:02Z",
            "payload": {"cwd": "/workspace/example-project", "model": "gpt-5.6-sol"}
        }),
        serde_json::json!({
            "type": "response_item",
            "timestamp": "2026-08-20T00:00:03Z",
            "payload": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": title}]}
        }),
        serde_json::json!({
            "type": "response_item",
            "timestamp": "2026-08-20T00:00:10Z",
            "payload": {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": body}]}
        }),
    ];
    let path = home.join(".codex/sessions/2026/08").join(file_name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let content = records
        .iter()
        .map(serde_json::Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(path, format!("{content}\n")).unwrap();
}
