use crate::conversation::{
    plan_conversation_file_index, ConversationFileFingerprint, ConversationFileIndexPlan,
};
use crate::test_support::*;

fn seed_codex_fixture(
    home: &std::path::Path,
    file_name: &str,
    fixture_name: &str,
) -> std::path::PathBuf {
    let path = home.join(".codex/sessions/2026/08").join(file_name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, fixture(fixture_name)).unwrap();
    path
}

fn fingerprint(offset: i64, size: i64, mtime: i64) -> ConversationFileFingerprint {
    ConversationFileFingerprint {
        mtime_ns: mtime,
        size,
        revision: "rev".to_string(),
        indexed_byte_offset: offset,
        has_live_generation: true,
    }
}

#[test]
fn incremental_plan_skips_unchanged_files_and_rebuilds_when_rewritten() {
    let cached = fingerprint(120, 120, 50);
    assert_eq!(
        plan_conversation_file_index(Some(&cached), 50, 120, "rev", true),
        ConversationFileIndexPlan::Skip
    );
    assert_eq!(
        plan_conversation_file_index(Some(&cached), 50, 80, "rev-2", true),
        ConversationFileIndexPlan::Full,
        "体积缩小视为重写"
    );
    assert_eq!(
        plan_conversation_file_index(Some(&cached), 40, 180, "rev-2", true),
        ConversationFileIndexPlan::Full,
        "mtime 缩小视为重写"
    );
    assert_eq!(
        plan_conversation_file_index(Some(&fingerprint(0, 120, 50)), 60, 180, "rev-2", true),
        ConversationFileIndexPlan::Full,
        "没有游标时不能增量"
    );
}

#[test]
fn incremental_plan_appends_when_the_file_only_grows() {
    let cached = fingerprint(120, 120, 50);
    assert_eq!(
        plan_conversation_file_index(Some(&cached), 60, 180, "rev-2", true),
        ConversationFileIndexPlan::Incremental
    );
    assert_eq!(
        plan_conversation_file_index(Some(&cached), 60, 180, "rev-2", false),
        ConversationFileIndexPlan::Full
    );
}

#[test]
fn appending_lines_adds_events_without_reordering_existing_sequences() {
    use std::io::Write;

    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let path = seed_codex_fixture(home, "rollout-conv-1.jsonl", "codex-conversation.jsonl");
    let conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();
    let before = crate::conversation::indexed_events(&conn, "codex", "conv-1").unwrap();
    assert!(!before.is_empty());
    let before_ids = before
        .iter()
        .map(|event| (event.event_id.clone(), event.sequence, event.text.clone()))
        .collect::<Vec<_>>();

    writeln!(
        std::fs::OpenOptions::new().append(true).open(&path).unwrap(),
        r#"{{"type":"response_item","timestamp":"2026-08-20T00:04:00Z","payload":{{"type":"message","role":"assistant","content":[{{"type":"output_text","text":"follow-up"}}]}}}}"#
    )
    .unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();

    let after = crate::conversation::indexed_events(&conn, "codex", "conv-1").unwrap();
    assert_eq!(
        after[..before.len()]
            .iter()
            .map(|event| (event.event_id.clone(), event.sequence, event.text.clone()))
            .collect::<Vec<_>>(),
        before_ids,
        "已有事件的序号与正文不得被重排"
    );
    assert_eq!(after.len(), before.len() + 1);
    assert_eq!(
        after.last().and_then(|event| event.text.as_deref()),
        Some("follow-up")
    );
    assert_eq!(
        after.last().map(|event| event.sequence),
        Some(before.last().expect("indexed").sequence + 1)
    );
    assert_conversation_index_matches_parse(&conn, home, "codex", "conv-1");
}

#[test]
fn appending_still_works_when_the_already_indexed_prefix_is_corrupted() {
    use std::io::Write;

    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let path = seed_codex_fixture(home, "rollout-conv-1.jsonl", "codex-conversation.jsonl");
    let original = std::fs::read(&path).unwrap();
    let conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();
    let before = crate::conversation::indexed_events(&conn, "codex", "conv-1").unwrap();

    let mut rewritten = original.clone();
    rewritten[0] = b'!';
    std::fs::write(&path, rewritten).unwrap();
    writeln!(
        std::fs::OpenOptions::new().append(true).open(&path).unwrap(),
        r#"{{"type":"response_item","timestamp":"2026-08-20T00:04:00Z","payload":{{"type":"message","role":"assistant","content":[{{"type":"output_text","text":"still-indexed"}}]}}}}"#
    )
    .unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();

    let after = crate::conversation::indexed_events(&conn, "codex", "conv-1").unwrap();
    assert_eq!(
        after[..before.len()]
            .iter()
            .map(|event| event.event_id.clone())
            .collect::<Vec<_>>(),
        before
            .iter()
            .map(|event| event.event_id.clone())
            .collect::<Vec<_>>(),
        "增量索引不得因为前缀被改写而丢掉已有事件"
    );
    assert_eq!(
        after.last().and_then(|event| event.text.as_deref()),
        Some("still-indexed"),
        "只解析新增行时，前缀损坏仍应追加新事件"
    );
}

#[test]
fn truncating_a_source_file_rebuilds_the_session_to_match_a_full_parse() {
    use std::io::Write;

    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let path = seed_codex_fixture(home, "rollout-conv-1.jsonl", "codex-conversation.jsonl");
    let original = std::fs::read_to_string(&path).unwrap();
    let conn = store::open_memory().unwrap();

    writeln!(
        std::fs::OpenOptions::new().append(true).open(&path).unwrap(),
        r#"{{"type":"response_item","timestamp":"2026-08-20T00:04:00Z","payload":{{"type":"message","role":"assistant","content":[{{"type":"output_text","text":"later"}}]}}}}"#
    )
    .unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();
    assert!(
        crate::conversation::indexed_events(&conn, "codex", "conv-1")
            .unwrap()
            .iter()
            .any(|event| event.text.as_deref() == Some("later"))
    );

    std::fs::write(&path, original).unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();
    assert_conversation_index_matches_parse(&conn, home, "codex", "conv-1");
    assert!(
        crate::conversation::indexed_events(&conn, "codex", "conv-1")
            .unwrap()
            .iter()
            .all(|event| event.text.as_deref() != Some("later"))
    );
}

#[test]
fn repeated_appends_match_a_single_full_parse() {
    use std::io::Write;

    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let incremental = seed_codex_fixture(home, "rollout-conv-1.jsonl", "codex-conversation.jsonl");
    let conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();

    for (stamp, text) in [
        ("2026-08-20T00:04:00Z", "first-append"),
        ("2026-08-20T00:05:00Z", "second-append"),
        ("2026-08-20T00:06:00Z", "third-append"),
    ] {
        writeln!(
            std::fs::OpenOptions::new()
                .append(true)
                .open(&incremental)
                .unwrap(),
            r#"{{"type":"response_item","timestamp":"{stamp}","payload":{{"type":"message","role":"assistant","content":[{{"type":"output_text","text":"{text}"}}]}}}}"#
        )
        .unwrap();
        crate::conversation::refresh_codex(&conn, home).unwrap();
    }

    let once = tempfile::tempdir().unwrap();
    let once_home = once.path();
    let once_path = seed_codex_fixture(
        once_home,
        "rollout-conv-1.jsonl",
        "codex-conversation.jsonl",
    );
    std::fs::write(&once_path, std::fs::read_to_string(&incremental).unwrap()).unwrap();
    let once_conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&once_conn, once_home).unwrap();

    let incremental_events = crate::conversation::indexed_events(&conn, "codex", "conv-1").unwrap();
    let once_events = crate::conversation::indexed_events(&once_conn, "codex", "conv-1").unwrap();
    let summary = |events: &[crate::domain::ConversationEvent]| {
        events
            .iter()
            .map(|event| {
                (
                    event.sequence,
                    event.source_sequence,
                    event.kind,
                    event.occurred_at.clone(),
                    event.text.clone(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(summary(&incremental_events), summary(&once_events));
    assert_conversation_index_matches_parse(&conn, home, "codex", "conv-1");
}

#[test]
fn an_earlier_timestamp_on_new_events_rebuilds_the_session() {
    use std::io::Write;

    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let path = seed_codex_fixture(home, "rollout-conv-1.jsonl", "codex-conversation.jsonl");
    let conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();

    writeln!(
        std::fs::OpenOptions::new().append(true).open(&path).unwrap(),
        r#"{{"type":"response_item","timestamp":"2026-08-19T23:00:00Z","payload":{{"type":"message","role":"assistant","content":[{{"type":"output_text","text":"rewound"}}]}}}}"#
    )
    .unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();
    assert_conversation_index_matches_parse(&conn, home, "codex", "conv-1");
    let events = crate::conversation::indexed_events(&conn, "codex", "conv-1").unwrap();
    assert_eq!(
        events.first().and_then(|event| event.text.as_deref()),
        Some("rewound")
    );
}

#[test]
fn a_suffix_with_a_different_session_id_rebuilds_instead_of_appending() {
    use std::io::Write;

    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let path = seed_codex_fixture(home, "rollout-conv-1.jsonl", "codex-conversation.jsonl");
    let conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();
    let before = crate::conversation::indexed_events(&conn, "codex", "conv-1").unwrap();
    assert!(!before.is_empty());

    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    writeln!(
        file,
        r#"{{"type":"session_meta","timestamp":"2026-08-20T00:04:00Z","payload":{{"id":"conv-other","cwd":"/workspace/example-project"}}}}"#
    )
    .unwrap();
    writeln!(
        file,
        r#"{{"type":"response_item","timestamp":"2026-08-20T00:04:01Z","payload":{{"type":"message","role":"assistant","content":[{{"type":"output_text","text":"hijacked"}}]}}}}"#
    )
    .unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();

    let original = crate::conversation::indexed_events(&conn, "codex", "conv-1").unwrap();
    assert!(
        original
            .iter()
            .all(|event| event.text.as_deref() != Some("hijacked")),
        "不得把另一会话的后缀追加到原会话"
    );
    let rebuilt = crate::conversation::indexed_events(&conn, "codex", "conv-other").unwrap();
    assert!(
        rebuilt
            .iter()
            .any(|event| event.text.as_deref() == Some("hijacked")),
        "会话 ID 不一致时应整份重索引到后缀里的会话"
    );
    assert_conversation_index_matches_parse(&conn, home, "codex", "conv-other");
}
