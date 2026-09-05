//! 事件表从「每行一条绝对路径 + 一条宽索引」换成「file_id + 工具汇总表」的形态迁移。
//!
//! 迁移是在后台线程上对既有缓存原地做的，不重新解析源文件，所以这里的断言全部围绕
//! 「迁移前后对外可见的行为一模一样」。

use crate::domain::{ConversationMatchField, ConversationQuery};
use crate::test_support::*;

fn seed(home: &std::path::Path) -> rusqlite::Connection {
    write_home_fixture(
        home,
        ".codex/sessions/2026/08/rollout-semantic-1.jsonl",
        "codex-semantic-events.jsonl",
    );
    let conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();
    conn
}

fn count(conn: &rusqlite::Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |row| row.get(0)).unwrap()
}

fn tool_names(conn: &rusqlite::Connection) -> Vec<String> {
    crate::conversation::catalog_tool_names(conn, &ConversationQuery::default()).unwrap()
}

fn sessions_with_tool(conn: &rusqlite::Connection, name: &str) -> u32 {
    crate::conversation::sessions_page(
        conn,
        &ConversationQuery {
            tool_names: vec![name.to_string()],
            ..Default::default()
        },
    )
    .unwrap()
    .total
}

fn body_search(conn: &rusqlite::Connection, term: &str) -> u32 {
    crate::conversation::sessions_page(
        conn,
        &ConversationQuery {
            search: Some(term.to_string()),
            ..Default::default()
        },
    )
    .unwrap()
    .total
}

/// 把库改回旧形态：事件表带 `source_file`、宽索引还在、倒排回到 `detail=full`。
/// 这是升级路径的起点，迁移 SQL 必须能从这里走到新形态。
fn downgrade_to_legacy_layout(conn: &rusqlite::Connection) {
    conn.execute_batch(
        r#"
        DROP TABLE conversation_events_fts;
        DROP TRIGGER IF EXISTS conversation_events_ai;
        DROP TRIGGER IF EXISTS conversation_events_ad;
        DROP TRIGGER IF EXISTS conversation_events_au;
        CREATE TABLE conversation_events_legacy (
            source TEXT NOT NULL,
            session_id TEXT NOT NULL,
            event_id TEXT NOT NULL,
            sequence INTEGER,
            source_file TEXT NOT NULL,
            source_sequence INTEGER NOT NULL,
            kind TEXT NOT NULL,
            actor TEXT,
            name TEXT,
            occurred_at TEXT,
            occurred_at_sort TEXT,
            text TEXT,
            attachments_json TEXT NOT NULL DEFAULT '[]',
            capability_status TEXT NOT NULL,
            content_status TEXT NOT NULL,
            identity_hash TEXT NOT NULL,
            identity_occurrence INTEGER NOT NULL,
            index_generation INTEGER NOT NULL
        );
        INSERT INTO conversation_events_legacy
        SELECT e.source, e.session_id, e.event_id, e.sequence, f.path, e.source_sequence,
               e.kind, e.actor, e.name, e.occurred_at, e.occurred_at_sort, e.text,
               e.attachments_json, e.capability_status, e.content_status, e.identity_hash,
               e.identity_occurrence, e.index_generation
        FROM conversation_events e
        JOIN conversation_files f ON f.file_id = e.file_id;
        DROP TABLE conversation_events;
        ALTER TABLE conversation_events_legacy RENAME TO conversation_events;
        CREATE INDEX idx_conversation_events_session_gen
            ON conversation_events(source, session_id, index_generation, sequence);
        CREATE INDEX idx_conversation_events_session_kind_name
            ON conversation_events(source, session_id, index_generation, kind, actor, name);
        DELETE FROM conversation_files;
        DELETE FROM conversation_session_tools;
        CREATE VIRTUAL TABLE conversation_events_fts USING fts5(
            text,
            name,
            content='conversation_events',
            content_rowid='rowid',
            tokenize='trigram'
        );
        CREATE TRIGGER conversation_events_ai AFTER INSERT ON conversation_events BEGIN
            INSERT INTO conversation_events_fts(rowid, text, name)
            VALUES (new.rowid, COALESCE(new.text, ''), COALESCE(new.name, ''));
        END;
        CREATE TRIGGER conversation_events_ad AFTER DELETE ON conversation_events BEGIN
            INSERT INTO conversation_events_fts(conversation_events_fts, rowid, text, name)
            VALUES ('delete', old.rowid, COALESCE(old.text, ''), COALESCE(old.name, ''));
        END;
        INSERT INTO conversation_events_fts(conversation_events_fts) VALUES('rebuild');
        "#,
    )
    .unwrap();
}

#[test]
fn source_file_is_stored_once_per_path_not_once_per_event() {
    let temp = tempfile::tempdir().unwrap();
    let conn = seed(temp.path());

    let events = count(&conn, "SELECT COUNT(*) FROM conversation_events");
    let files = count(&conn, "SELECT COUNT(*) FROM conversation_files");
    assert!(events > 1, "夹具应产生多条事件");
    assert_eq!(files, 1, "一个来源文件只该在字典里占一行");
}

#[test]
fn session_tools_summary_has_one_row_per_tool_not_per_call() {
    let temp = tempfile::tempdir().unwrap();
    let conn = seed(temp.path());

    let names = tool_names(&conn);
    assert!(names.contains(&"read_file".to_string()), "{names:?}");
    let summary_rows = count(&conn, "SELECT COUNT(*) FROM conversation_session_tools");
    let tool_events = count(
        &conn,
        "SELECT COUNT(*) FROM conversation_events
         WHERE kind IN ('tool_call', 'tool_result') OR (kind = 'error' AND actor = 'tool')",
    );
    assert!(summary_rows > 0);
    assert!(
        summary_rows <= tool_events,
        "汇总表不该比事件还多：{summary_rows} vs {tool_events}"
    );
    assert_eq!(sessions_with_tool(&conn, "read_file"), 1);
}

#[test]
fn reindexing_a_session_does_not_leave_stale_tool_rows() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let conn = seed(home);
    let before = count(&conn, "SELECT COUNT(*) FROM conversation_session_tools");

    crate::conversation::refresh_codex(&conn, home).unwrap();

    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM conversation_session_tools"),
        before,
        "重复索引同一批会话不该让汇总表增长"
    );
    assert_eq!(sessions_with_tool(&conn, "read_file"), 1);
}

#[test]
fn fresh_cache_needs_no_layout_migration() {
    let conn = store::open_memory().unwrap();
    assert!(!store::conversation_events_needs_layout_migration(&conn).unwrap());
}

#[test]
fn migrating_legacy_layout_preserves_search_tools_and_details() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let conn = seed(home);

    let expected_events =
        crate::conversation::indexed_events(&conn, "codex", "semantic-1").unwrap();
    assert!(!expected_events.is_empty());
    let expected_tools = tool_names(&conn);
    let expected_body = body_search(&conn, "fn main");
    assert_eq!(expected_body, 1);

    downgrade_to_legacy_layout(&conn);
    assert!(store::conversation_events_needs_layout_migration(&conn).unwrap());
    assert!(store::conversation_fts_needs_migration(&conn).unwrap());

    store::migrate_conversation_events_layout(&conn).unwrap();

    assert!(!store::conversation_events_needs_layout_migration(&conn).unwrap());
    // 形态迁移在同一个事务里连带重建了倒排，正文索引已经是新形态，不该再要一次迁移。
    assert!(!store::conversation_fts_needs_migration(&conn).unwrap());
    assert_eq!(
        crate::conversation::indexed_events(&conn, "codex", "semantic-1").unwrap(),
        expected_events,
        "迁移不得改变事件内容或顺序"
    );
    assert_eq!(tool_names(&conn), expected_tools);
    assert_eq!(sessions_with_tool(&conn, "read_file"), 1);
    assert_eq!(body_search(&conn, "fn main"), expected_body);

    let page = crate::conversation::sessions_page(
        &conn,
        &ConversationQuery {
            search: Some("fn main".to_string()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(page.rows[0].match_field, Some(ConversationMatchField::Body));
}

#[test]
fn migrating_legacy_layout_drops_the_wide_tool_index() {
    let temp = tempfile::tempdir().unwrap();
    let conn = seed(temp.path());
    downgrade_to_legacy_layout(&conn);
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'index' AND name = 'idx_conversation_events_session_kind_name'"
        ),
        1
    );

    store::migrate_conversation_events_layout(&conn).unwrap();

    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'index' AND name = 'idx_conversation_events_session_kind_name'"
        ),
        0,
        "工具筛选改走汇总表之后，这条宽索引不该再留着"
    );
}
