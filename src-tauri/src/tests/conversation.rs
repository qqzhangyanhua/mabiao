use base64::{engine::general_purpose::STANDARD as BASE64, Engine};

use crate::test_support::*;

fn test_png_bytes() -> Vec<u8> {
    let pixels = image::RgbaImage::from_pixel(2, 2, image::Rgba([24, 160, 200, 255]));
    let mut output = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(pixels)
        .write_to(&mut output, image::ImageFormat::Png)
        .unwrap();
    output.into_inner()
}

fn seed_codex_conversation(home: &std::path::Path) -> std::path::PathBuf {
    seed_codex_fixture(home, "rollout-conv-1.jsonl", "codex-conversation.jsonl")
}

fn seed_conversation_fixture(
    home: &std::path::Path,
    relative_path: &str,
    fixture_name: &str,
) -> std::path::PathBuf {
    let path = home.join(relative_path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, fixture(fixture_name)).unwrap();
    path
}

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

fn seed_codex_records(
    home: &std::path::Path,
    file_name: &str,
    records: &[serde_json::Value],
) -> std::path::PathBuf {
    let path = home.join(".codex/sessions/2026/08").join(file_name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let content = records
        .iter()
        .map(serde_json::Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&path, format!("{content}\n")).unwrap();
    path
}

fn seed_rich_codex_conversation(
    home: &std::path::Path,
) -> (std::path::PathBuf, String, std::path::PathBuf) {
    let attachment = home.join("attachments/screenshot.png");
    std::fs::create_dir_all(attachment.parent().unwrap()).unwrap();
    std::fs::write(&attachment, test_png_bytes()).unwrap();
    let missing = home.join("attachments/missing.pdf");
    let large_output = format!("{}FULL-END", "large tool output\n".repeat(400));
    let records = [
        serde_json::json!({
            "type": "session_meta",
            "timestamp": "2026-08-24T00:00:00Z",
            "payload": {"id": "rich-1", "cwd": home, "title": "富内容会话"}
        }),
        serde_json::json!({
            "type": "response_item",
            "timestamp": "2026-08-24T00:00:01Z",
            "payload": {
                "type": "message",
                "role": "user",
                "content": [
                    {"type": "input_text", "text": "查看附件"},
                    {"type": "input_image", "file_path": attachment, "name": "screenshot.png", "mime_type": "image/png"},
                    {"type": "input_file", "file_path": missing, "name": "missing.pdf", "mime_type": "application/pdf"}
                ]
            }
        }),
        serde_json::json!({
            "type": "response_item",
            "timestamp": "2026-08-24T00:00:02Z",
            "payload": {
                "type": "message",
                "role": "assistant",
                "content": [{
                    "type": "output_text",
                    "text": "# 结果\n\n|列|值|\n|-|-|\n|状态|完成|\n\n```rust\nfn main() {}\n```\n\n<iframe src=\"https://unsafe.invalid\"></iframe>\n\n[危险](javascript:alert(1))"
                }]
            }
        }),
        serde_json::json!({
            "type": "response_item",
            "timestamp": "2026-08-24T00:00:03Z",
            "payload": {"type": "function_call_output", "call_id": "call-rich", "output": large_output}
        }),
    ];
    let transcript = records
        .iter()
        .map(serde_json::Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    let path = home.join(".codex/sessions/2026/08/rollout-rich-1.jsonl");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, format!("{transcript}\n")).unwrap();
    (path, large_output, missing)
}

#[test]
fn codex_conversation_detail_defers_large_tool_results_until_requested() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let (_, large_output, _) = seed_rich_codex_conversation(home);
    let conn = store::open_memory().unwrap();

    crate::conversation::refresh_codex(&conn, home).unwrap();
    let detail = crate::conversation::load_parsed_detail(&conn, home, "codex", "rich-1").unwrap();
    let event = detail
        .events
        .iter()
        .find(|event| event.sequence == 3)
        .unwrap();

    assert_eq!(
        event.content_status,
        ConversationEventContentStatus::Deferred
    );
    assert!(event
        .text
        .as_ref()
        .unwrap()
        .starts_with("large tool output"));
    assert!(!event.text.as_ref().unwrap().contains("FULL-END"));
    assert!(event.details.get("output").is_none());

    let full =
        crate::conversation::load_event_content(&conn, home, "codex", "rich-1", &event.event_id)
            .unwrap();
    assert_eq!(full.event_id, event.event_id);
    assert_eq!(full.text.as_deref(), Some(large_output.as_str()));
    assert_eq!(full.details["output"], large_output);
}

#[test]
fn codex_conversation_detail_reports_attachments_and_loads_images_on_demand() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let (_, _, missing_path) = seed_rich_codex_conversation(home);
    let conn = store::open_memory().unwrap();

    crate::conversation::refresh_codex(&conn, home).unwrap();
    let detail = crate::conversation::load_parsed_detail(&conn, home, "codex", "rich-1").unwrap();
    let event = detail
        .events
        .iter()
        .find(|event| event.sequence == 1)
        .unwrap();

    assert!(
        event.details.get("content").is_none(),
        "message details must not eagerly return attachment bodies"
    );
    assert_eq!(event.attachments.len(), 2);
    assert!(event.attachments[0].id.starts_with(&event.event_id));
    assert_eq!(event.attachments[0].kind, ConversationAttachmentKind::Image);
    assert_eq!(
        event.attachments[0].status,
        ConversationAttachmentStatus::Available
    );
    assert_eq!(
        event.attachments[0].size_bytes,
        Some(test_png_bytes().len() as u64)
    );
    assert_eq!(event.attachments[1].name, "missing.pdf");
    assert_eq!(
        event.attachments[1].original_path,
        missing_path.to_string_lossy()
    );
    assert_eq!(
        event.attachments[1].status,
        ConversationAttachmentStatus::Missing
    );

    let thumbnail = crate::conversation::load_attachment_thumbnail(
        &conn,
        home,
        "codex",
        "rich-1",
        &event.attachments[0].id,
    )
    .unwrap();
    assert_eq!(thumbnail.attachment, event.attachments[0]);
    let thumbnail_bytes = BASE64
        .decode(
            thumbnail
                .data_url
                .strip_prefix("data:image/png;base64,")
                .unwrap(),
        )
        .unwrap();
    let decoded_thumbnail = image::load_from_memory(&thumbnail_bytes).unwrap();
    assert_eq!(
        (decoded_thumbnail.width(), decoded_thumbnail.height()),
        (2, 2)
    );

    let image = crate::conversation::load_attachment(
        &conn,
        home,
        "codex",
        "rich-1",
        &event.attachments[0].id,
    )
    .unwrap();
    assert_eq!(image.attachment, event.attachments[0]);
    assert_eq!(
        image.data_url,
        format!("data:image/png;base64,{}", BASE64.encode(test_png_bytes()))
    );

    assert_eq!(detail.events.len(), 4, "缺失附件不应阻断其余事件");
}

#[test]
fn read_source_line_rejects_an_out_of_range_index() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("session.jsonl");
    std::fs::write(&path, "first\nsecond\n").unwrap();

    let error = crate::conversation::read_source_line(&path, 2).unwrap_err();
    assert!(
        error.contains("未找到第 3 行"),
        "out-of-range must error instead of returning another line: {error}"
    );
    assert_eq!(
        crate::conversation::read_source_line(&path, 0).unwrap(),
        "first"
    );
    assert_eq!(
        crate::conversation::read_source_line(&path, 1).unwrap(),
        "second"
    );
}

#[test]
fn read_source_line_rejects_an_empty_file() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("empty.jsonl");
    std::fs::write(&path, "").unwrap();

    let error = crate::conversation::read_source_line(&path, 0).unwrap_err();
    assert!(
        error.contains("未找到第 1 行"),
        "empty file must error: {error}"
    );
}

#[test]
fn read_source_line_rejects_a_missing_file() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("missing.jsonl");

    let error = crate::conversation::read_source_line(&path, 0).unwrap_err();
    assert!(
        error.contains("读取原始文件失败"),
        "missing file must use the existing Chinese error: {error}"
    );
}

#[test]
fn codex_conversation_attachment_loader_rejects_unrelated_source_siblings() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let (source_path, _, _) = seed_rich_codex_conversation(home);
    let project = home.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let outside_image = source_path.parent().unwrap().join("unrelated.png");
    std::fs::write(&outside_image, test_png_bytes()).unwrap();
    let mut records = std::fs::read_to_string(&source_path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    records[0]["payload"]["cwd"] = serde_json::json!(project);
    records[1]["payload"]["content"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "type": "input_image",
            "file_path": outside_image,
            "name": "outside.png",
            "mime_type": "image/png"
        }));
    let content = records
        .iter()
        .map(serde_json::Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&source_path, format!("{content}\n")).unwrap();
    let conn = store::open_memory().unwrap();

    crate::conversation::refresh_codex(&conn, home).unwrap();
    let detail = crate::conversation::load_parsed_detail(&conn, home, "codex", "rich-1").unwrap();
    let attachment_id = detail
        .events
        .iter()
        .flat_map(|event| &event.attachments)
        .find(|attachment| attachment.name == "outside.png")
        .unwrap()
        .id
        .clone();
    let error =
        crate::conversation::load_attachment(&conn, home, "codex", "rich-1", &attachment_id)
            .unwrap_err();

    assert!(error.contains("允许的目录"), "unexpected error: {error}");
}

#[test]
fn codex_conversation_exports_markdown_and_raw_json_from_the_current_source_file() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let (source_path, _, missing_path) = seed_rich_codex_conversation(home);
    let conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();

    let changed = std::fs::read_to_string(&source_path)
        .unwrap()
        .replace("# 结果", "# 导出后的结果");
    std::fs::write(&source_path, &changed).unwrap();

    let markdown = crate::conversation::build_export(
        &conn,
        home,
        "codex",
        "rich-1",
        ConversationExportFormat::Markdown,
    )
    .unwrap();
    assert_eq!(markdown.default_name, "富内容会话.md");
    let markdown_text = String::from_utf8(markdown.content.clone()).unwrap();
    assert!(markdown_text.contains("# 导出后的结果"));
    assert!(markdown_text.contains("FULL-END"));
    assert!(markdown_text.contains(&missing_path.to_string_lossy().to_string()));
    assert!(markdown_text.contains("附件缺失"));
    let markdown_path = home.join("exported.md");
    crate::user_files::write_export(&markdown_path, &markdown.content, None).unwrap();
    assert_eq!(std::fs::read(&markdown_path).unwrap(), markdown.content);
    let error =
        crate::user_files::write_export(&markdown_path, b"replacement export\n", None).unwrap_err();
    assert!(error.contains("已存在"));
    assert_eq!(std::fs::read(&markdown_path).unwrap(), markdown.content);
    let rejected_path = home.join("exported.txt");
    let error = crate::user_files::write_export(&rejected_path, b"not allowed", None).unwrap_err();
    assert!(error.contains("可写名单"));
    assert!(!rejected_path.exists());

    let raw_json = crate::conversation::build_export(
        &conn,
        home,
        "codex",
        "rich-1",
        ConversationExportFormat::Json,
    )
    .unwrap();
    assert_eq!(raw_json.default_name, "富内容会话.jsonl");
    assert_eq!(raw_json.content, changed.as_bytes());
    assert!(String::from_utf8_lossy(&raw_json.content).contains("FULL-END"));
    let json_path = home.join("exported.jsonl");
    crate::user_files::write_export(&json_path, &raw_json.content, None).unwrap();
    assert_eq!(std::fs::read(json_path).unwrap(), changed.as_bytes());
}

#[test]
fn streaming_markdown_export_matches_full_parse_for_indexed_and_unindexed_sessions() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_rich_codex_conversation(home);
    let conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();
    let prepared = crate::conversation::prepare_detail(&conn, "codex", "rich-1").unwrap();
    assert!(crate::conversation::event_index_ready(&conn, home, &prepared).unwrap());

    let streamed = crate::conversation::build_export(
        &conn,
        home,
        "codex",
        "rich-1",
        ConversationExportFormat::Markdown,
    )
    .unwrap();
    let parsed = crate::conversation::parsed_export(
        &conn,
        home,
        "codex",
        "rich-1",
        ConversationExportFormat::Markdown,
    )
    .unwrap();
    assert_eq!(streamed.default_name, parsed.default_name);
    assert_eq!(streamed.content, parsed.content);
    let streamed_text = String::from_utf8(streamed.content.clone()).unwrap();
    assert!(streamed_text.contains("FULL-END"));
    let export_path = home.join("streamed.md");
    crate::conversation::write_conversation_export(
        &conn,
        home,
        "codex",
        "rich-1",
        ConversationExportFormat::Markdown,
        &export_path,
        None,
    )
    .unwrap();
    assert_eq!(std::fs::read(&export_path).unwrap(), parsed.content);

    conn.execute(
        "DELETE FROM conversation_events WHERE source = 'codex' AND session_id = 'rich-1'",
        [],
    )
    .unwrap();
    conn.execute(
        r#"
        UPDATE conversation_sessions
        SET adapter_version = 0, event_index_generation = NULL
        WHERE source = 'codex' AND session_id = 'rich-1'
        "#,
        [],
    )
    .unwrap();
    let prepared = crate::conversation::prepare_detail(&conn, "codex", "rich-1").unwrap();
    assert!(!crate::conversation::event_index_ready(&conn, home, &prepared).unwrap());
    let fallback = crate::conversation::build_export(
        &conn,
        home,
        "codex",
        "rich-1",
        ConversationExportFormat::Markdown,
    )
    .unwrap();
    assert_eq!(fallback.content, parsed.content);
}

#[test]
fn streaming_export_failure_does_not_leave_partial_target() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("partial.md");
    let error = crate::user_files::write_export_with(&path, None, |writer| {
        writer.write_all(b"# partial\n").unwrap();
        Err("模拟中途失败".to_string())
    })
    .unwrap_err();
    assert!(error.contains("模拟中途失败"));
    assert!(!path.exists());
    let leftovers: Vec<_> = std::fs::read_dir(temp.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn conversation_detail_prepared_context_loads_after_connection_is_dropped() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_codex_conversation(home);
    let conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();

    let prepared = crate::conversation::prepare_detail(&conn, "codex", "conv-1").unwrap();
    drop(conn);

    let detail = crate::conversation::load_prepared_parsed(home, prepared).unwrap();
    assert_eq!(detail.session.session_id, "conv-1");
    assert!(!message_texts(&detail).is_empty());
}

#[test]
fn conversation_detail_consistent_snapshot_stops_after_three_changed_attempts() {
    use std::cell::Cell;
    use std::collections::VecDeque;

    let revisions = std::cell::RefCell::new(VecDeque::from([
        "before-1", "after-1", "before-2", "after-2", "before-3", "after-3",
    ]));
    let attempts = Cell::new(0);
    let error = crate::conversation::read_consistent_snapshot(
        || Ok(revisions.borrow_mut().pop_front().unwrap().to_string()),
        || {
            attempts.set(attempts.get() + 1);
            Err::<(), _>("JSON EOF".to_string())
        },
    )
    .unwrap_err();

    assert_eq!(attempts.get(), 3);
    assert!(error.contains("持续变化"), "unexpected error: {error}");
}

#[test]
fn conversation_detail_file_revision_maps_canonicalize_and_metadata_not_found_to_unavailable() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join(".codex/sessions");
    std::fs::create_dir_all(&root).unwrap();
    let canonical_root = std::fs::canonicalize(&root).unwrap();

    let missing_during_canonicalize = crate::conversation::checked_detail_file_revision(
        std::slice::from_ref(&root),
        || Err(std::io::Error::from(std::io::ErrorKind::NotFound)),
        |_| Ok("unused".to_string()),
    )
    .unwrap();
    assert_eq!(missing_during_canonicalize, None);

    let missing_during_metadata = crate::conversation::checked_detail_file_revision(
        std::slice::from_ref(&root),
        || Ok(canonical_root.clone()),
        |_| Err(std::io::Error::from(std::io::ErrorKind::NotFound)),
    )
    .unwrap();
    assert_eq!(missing_during_metadata, None);
}

#[test]
fn conversation_detail_revision_uses_modified_nanoseconds_and_size() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let path = seed_codex_conversation(home);
    let conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();

    let detail = crate::conversation::load_parsed_detail(&conn, home, "codex", "conv-1").unwrap();
    let metadata = std::fs::metadata(path).unwrap();
    let modified_ns = metadata
        .modified()
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();

    assert_eq!(detail.revision, format!("{modified_ns}:{}", metadata.len()));
}

#[test]
fn conversation_detail_rejects_newline_terminated_invalid_json() {
    use std::io::Write;

    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let path = seed_codex_conversation(home);
    let conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();
    writeln!(
        std::fs::OpenOptions::new().append(true).open(path).unwrap(),
        "{{\"type\":"
    )
    .unwrap();

    let error = crate::conversation::load_detail(&conn, home, "codex", "conv-1").unwrap_err();
    assert!(error.contains("JSON 无效"), "unexpected error: {error}");
}

#[test]
fn conversation_detail_rejects_unterminated_trailing_json_syntax_error() {
    use std::io::Write;

    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let path = seed_codex_conversation(home);
    let conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();
    write!(
        std::fs::OpenOptions::new().append(true).open(path).unwrap(),
        "{{\"type\": nope}}"
    )
    .unwrap();

    let error = crate::conversation::load_detail(&conn, home, "codex", "conv-1").unwrap_err();
    assert!(error.contains("JSON 无效"), "unexpected error: {error}");
}

#[test]
fn conversation_detail_state_detects_append_delete_and_restore_without_refresh() {
    use std::io::Write;

    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let path = seed_codex_conversation(home);
    let original = std::fs::read(&path).unwrap();
    let conn = store::open_memory().unwrap();

    crate::conversation::refresh_codex(&conn, home).unwrap();
    let initial = crate::conversation::load_parsed_detail(&conn, home, "codex", "conv-1").unwrap();
    assert!(!initial.revision.is_empty());

    let unchanged =
        crate::conversation::detail_state(&conn, home, "codex", "conv-1", &initial.revision)
            .unwrap();
    assert_eq!(unchanged.revision, initial.revision);
    assert!(!unchanged.changed);
    assert!(unchanged.file_available);

    let initial_message_count = message_texts(&initial).len();
    let initial_event_count = initial.events.len();
    writeln!(
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap(),
        r#"{{"type":"response_item","timestamp":"2026-08-20T00:04:00Z","payload":{{"type":"message","role":"assistant","content":[{{"type":"output_text","text":"follow-up"}}]}}}}"#
    )
    .unwrap();

    let changed =
        crate::conversation::detail_state(&conn, home, "codex", "conv-1", &initial.revision)
            .unwrap();
    assert!(changed.changed);
    assert!(changed.file_available);
    assert_ne!(changed.revision, initial.revision);

    let updated = crate::conversation::load_parsed_detail(&conn, home, "codex", "conv-1").unwrap();
    let updated_messages = message_texts(&updated);
    assert_eq!(updated_messages.len(), initial_message_count + 1);
    assert_eq!(updated.events.len(), initial_event_count + 1);
    assert_eq!(
        updated_messages.last().map(String::as_str),
        Some("follow-up")
    );
    assert_eq!(updated.revision, changed.revision);

    std::fs::remove_file(&path).unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();
    let deleted =
        crate::conversation::detail_state(&conn, home, "codex", "conv-1", &updated.revision)
            .unwrap();
    assert!(!deleted.file_available);

    std::fs::write(&path, original).unwrap();
    let restored =
        crate::conversation::detail_state(&conn, home, "codex", "conv-1", &updated.revision)
            .unwrap();
    assert!(restored.file_available);
    assert!(restored.changed);

    let restored_detail =
        crate::conversation::load_parsed_detail(&conn, home, "codex", "conv-1").unwrap();
    assert!(restored_detail.session.file_available);
}

#[test]
fn conversation_detail_state_reads_metadata_without_parsing_body() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let path = seed_codex_conversation(home);
    let conn = store::open_memory().unwrap();

    crate::conversation::refresh_codex(&conn, home).unwrap();
    let initial = crate::conversation::load_parsed_detail(&conn, home, "codex", "conv-1").unwrap();
    std::fs::write(&path, b"this is not valid jsonl").unwrap();

    let changed =
        crate::conversation::detail_state(&conn, home, "codex", "conv-1", &initial.revision)
            .unwrap();
    assert!(changed.changed);
    assert!(changed.file_available);

    let unchanged =
        crate::conversation::detail_state(&conn, home, "codex", "conv-1", &changed.revision)
            .unwrap();
    assert!(!unchanged.changed);
    assert!(unchanged.file_available);
}

#[test]
fn conversation_detail_state_tracks_an_incomplete_trailing_jsonl_line_until_completion() {
    use std::io::Write;

    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let path = seed_codex_conversation(home);
    let conn = store::open_memory().unwrap();

    crate::conversation::refresh_codex(&conn, home).unwrap();
    let initial = crate::conversation::load_parsed_detail(&conn, home, "codex", "conv-1").unwrap();
    let initial_message_count = message_texts(&initial).len();
    let initial_event_count = initial.events.len();
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    file.write_all(
        br#"{"type":"response_item","timestamp":"2026-08-20T00:04:00Z","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"stream"#,
    )
    .unwrap();
    file.flush().unwrap();

    let partial = crate::conversation::load_parsed_detail(&conn, home, "codex", "conv-1").unwrap();
    assert_eq!(message_texts(&partial).len(), initial_message_count);
    assert_eq!(partial.events.len(), initial_event_count);
    assert_ne!(partial.revision, initial.revision);

    file.write_all(br#"ed"}]}}"#).unwrap();
    file.write_all(b"\n").unwrap();
    file.flush().unwrap();
    drop(file);

    let completed_state =
        crate::conversation::detail_state(&conn, home, "codex", "conv-1", &partial.revision)
            .unwrap();
    assert!(completed_state.changed);
    assert!(completed_state.file_available);

    let completed =
        crate::conversation::load_parsed_detail(&conn, home, "codex", "conv-1").unwrap();
    let completed_messages = message_texts(&completed);
    assert_eq!(completed_messages.len(), initial_message_count + 1);
    assert_eq!(completed.events.len(), initial_event_count + 1);
    assert_eq!(
        completed_messages.last().map(String::as_str),
        Some("streamed")
    );
    assert_eq!(completed.revision, completed_state.revision);
}

#[test]
fn conversation_detail_state_rejects_indexed_path_outside_source_root() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_codex_conversation(home);
    let outside = home.join("outside.jsonl");
    std::fs::write(&outside, fixture("codex-conversation.jsonl")).unwrap();
    let conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();
    conn.execute(
        "UPDATE conversation_sessions SET source_file = ?1 WHERE source = 'codex' AND session_id = 'conv-1'",
        rusqlite::params![outside.to_string_lossy().to_string()],
    )
    .unwrap();

    let error =
        crate::conversation::detail_state(&conn, home, "codex", "conv-1", "known").unwrap_err();
    assert!(
        error.contains("允许的扫描目录"),
        "unexpected error: {error}"
    );
}

#[test]
fn codex_conversation_detail_merges_streamed_text_and_filters_protocol_noise() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_codex_fixture(
        home,
        "rollout-semantic-1.jsonl",
        "codex-semantic-events.jsonl",
    );
    let conn = store::open_memory().unwrap();

    crate::conversation::refresh_codex(&conn, home).unwrap();
    let detail =
        crate::conversation::load_parsed_detail(&conn, home, "codex", "semantic-1").unwrap();

    let message_events = detail
        .events
        .iter()
        .filter(|event| event.kind == ConversationEventKind::Message)
        .collect::<Vec<_>>();
    assert_eq!(message_events.len(), 2);
    assert_eq!(message_events[0].actor, Some(ConversationEventActor::User));
    assert_eq!(message_events[0].text.as_deref(), Some("实现语义时间线"));
    assert_eq!(
        message_events[1].actor,
        Some(ConversationEventActor::Assistant)
    );
    assert_eq!(
        message_events[1].text.as_deref(),
        Some("我先检查现有实现。")
    );
    assert_eq!(message_events[1].sequence, 3);
    assert!(detail
        .events
        .iter()
        .all(|event| { !matches!(event.name.as_deref(), Some("token_count" | "heartbeat")) }));
}

#[test]
fn codex_conversation_detail_deduplicates_final_messages_across_protocol_channels() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_codex_fixture(
        home,
        "rollout-duplicates-1.jsonl",
        "codex-duplicate-messages.jsonl",
    );
    let conn = store::open_memory().unwrap();

    crate::conversation::refresh_codex(&conn, home).unwrap();
    let detail =
        crate::conversation::load_parsed_detail(&conn, home, "codex", "duplicates-1").unwrap();
    let messages = detail
        .events
        .iter()
        .filter(|event| event.kind == ConversationEventKind::Message)
        .map(|event| {
            (
                event.actor.map(ConversationEventActor::as_str),
                event.text.as_deref(),
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        messages,
        vec![
            (Some("user"), Some("同一条用户消息")),
            (Some("assistant"), Some("同一条助手消息")),
            (Some("user"), Some("同一条用户消息")),
        ]
    );
    assert_eq!(message_texts(&detail).len(), 3);
}

#[test]
fn codex_conversation_detail_orders_by_timestamp_then_source_sequence() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_codex_fixture(
        home,
        "rollout-ordered-1.jsonl",
        "codex-out-of-order-events.jsonl",
    );
    let conn = store::open_memory().unwrap();

    crate::conversation::refresh_codex(&conn, home).unwrap();
    let detail =
        crate::conversation::load_parsed_detail(&conn, home, "codex", "ordered-1").unwrap();
    let order = detail
        .events
        .iter()
        .map(|event| (event.kind.as_str(), event.sequence, event.source_sequence))
        .collect::<Vec<_>>();

    assert_eq!(
        order,
        vec![
            ("system_status", 0, 0),
            ("plan", 1, 2),
            ("error", 2, 1),
            ("unadapted", 3, 3),
        ]
    );
    assert_eq!(detail.events[3].occurred_at, None);
}

#[test]
fn codex_conversation_detail_projects_semantic_events_and_preserves_unknown_events() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_codex_fixture(
        home,
        "rollout-semantic-1.jsonl",
        "codex-semantic-events.jsonl",
    );
    let conn = store::open_memory().unwrap();

    crate::conversation::refresh_codex(&conn, home).unwrap();
    let detail =
        crate::conversation::load_parsed_detail(&conn, home, "codex", "semantic-1").unwrap();
    let kinds = detail
        .events
        .iter()
        .map(|event| event.kind.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        kinds,
        vec![
            "system_status",
            "model_change",
            "message",
            "message",
            "plan",
            "tool_call",
            "tool_result",
            "model_change",
            "error",
            "unadapted",
        ]
    );
    assert!(detail
        .events
        .windows(2)
        .all(|pair| pair[0].sequence < pair[1].sequence));
    let plan = &detail.events[4];
    assert_eq!(plan.text.as_deref(), Some("按层实现"));
    assert_eq!(plan.details["plan"][0]["step"], "后端事件投影");
    let call = &detail.events[5];
    assert_eq!(call.name.as_deref(), Some("read_file"));
    assert_eq!(call.details["call_id"], "call-1");
    assert_eq!(detail.events[6].text.as_deref(), Some("fn main() {}"));
    assert_eq!(detail.events[7].name.as_deref(), Some("gpt-5.7-codex"));
    assert_eq!(detail.events[8].text.as_deref(), Some("工具执行失败"));
    let unknown = &detail.events[9];
    assert_eq!(unknown.name.as_deref(), Some("future_event"));
    assert_eq!(unknown.occurred_at, None);
    assert_eq!(
        unknown.capability_status,
        ConversationEventCapabilityStatus::UnadaptedMissingTimestamp
    );
    assert_eq!(unknown.details["payload"]["phase"], "next");
}

#[test]
fn codex_conversation_merges_duplicate_session_files_in_stable_order() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let meta = serde_json::json!({
        "type": "session_meta",
        "timestamp": "2026-08-21T00:00:00Z",
        "payload": {"id": "split-1", "cwd": "/workspace/split", "title": "Split session"}
    });
    let duplicate = serde_json::json!({
        "type": "response_item",
        "timestamp": "2026-08-21T00:00:02Z",
        "payload": {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "shared"}]}
    });
    seed_codex_records(
        home,
        "rollout-split-a.jsonl",
        &[
            meta.clone(),
            duplicate.clone(),
            serde_json::json!({
                "type": "response_item",
                "timestamp": "2026-08-21T00:00:04Z",
                "payload": {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "late"}]}
            }),
        ],
    );
    let second_path = seed_codex_records(
        home,
        "rollout-split-b.jsonl",
        &[
            meta,
            serde_json::json!({
                "type": "response_item",
                "timestamp": "2026-08-21T00:00:01Z",
                "payload": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "early"}]}
            }),
            duplicate,
        ],
    );
    let conn = store::open_memory().unwrap();

    crate::conversation::refresh_codex(&conn, home).unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();
    let page =
        crate::conversation::sessions_page(&conn, &crate::domain::ConversationQuery::default())
            .unwrap();
    let detail = crate::conversation::load_parsed_detail(&conn, home, "codex", "split-1").unwrap();

    assert_eq!(page.total, 1);
    assert_eq!(page.rows[0].source_files.len(), 2);
    assert_eq!(detail.session.source_files, page.rows[0].source_files);
    assert_eq!(
        std::path::Path::new(&page.rows[0].source_file)
            .file_name()
            .and_then(|name| name.to_str()),
        Some("rollout-split-a.jsonl")
    );
    let texts = detail
        .events
        .iter()
        .filter_map(|event| event.text.as_deref())
        .collect::<Vec<_>>();
    assert_eq!(texts, vec!["early", "shared", "late"]);
    assert!(detail
        .events
        .windows(2)
        .all(|pair| pair[0].sequence < pair[1].sequence));
    let event_ids = detail
        .events
        .iter()
        .map(|event| event.event_id.clone())
        .collect::<Vec<_>>();
    crate::conversation::refresh_codex(&conn, home).unwrap();
    let refreshed =
        crate::conversation::load_parsed_detail(&conn, home, "codex", "split-1").unwrap();
    assert_eq!(
        refreshed
            .events
            .iter()
            .map(|event| event.event_id.clone())
            .collect::<Vec<_>>(),
        event_ids
    );
    conn.execute(
        "UPDATE conversation_sessions SET title = 'cached-title' WHERE source = 'codex' AND session_id = 'split-1'",
        [],
    )
    .unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();
    let unchanged =
        crate::conversation::sessions_page(&conn, &crate::domain::ConversationQuery::default())
            .unwrap();
    assert_eq!(unchanged.rows[0].title, "cached-title");

    let known_revision = refreshed.revision;
    use std::io::Write as _;
    writeln!(
        std::fs::OpenOptions::new()
            .append(true)
            .open(second_path)
            .unwrap(),
        "{}",
        serde_json::json!({
            "type": "response_item",
            "timestamp": "2026-08-21T00:00:05Z",
            "payload": {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "new tail"}]}
        })
    )
    .unwrap();
    let changed =
        crate::conversation::detail_state(&conn, home, "codex", "split-1", &known_revision)
            .unwrap();
    assert!(changed.changed);
    assert!(changed.file_available);
    assert!(crate::conversation::build_export(
        &conn,
        home,
        "codex",
        "split-1",
        crate::domain::ConversationExportFormat::Json,
    )
    .unwrap_err()
    .contains("多个原始文件"));
}

#[test]
fn codex_conversation_partial_file_loss_preserves_last_good_aggregate_until_restore() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let meta = serde_json::json!({
        "type": "session_meta",
        "timestamp": "2026-08-21T00:00:00Z",
        "payload": {"id": "partial-loss-1", "cwd": "/workspace/partial-loss"}
    });
    seed_codex_records(
        home,
        "rollout-partial-loss-a.jsonl",
        &[
            meta.clone(),
            serde_json::json!({
                "type": "response_item",
                "timestamp": "2026-08-21T00:00:01Z",
                "payload": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "first"}]}
            }),
        ],
    );
    let second_path = seed_codex_records(
        home,
        "rollout-partial-loss-b.jsonl",
        &[
            meta,
            serde_json::json!({
                "type": "response_item",
                "timestamp": "2026-08-21T00:00:02Z",
                "payload": {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "second"}]}
            }),
        ],
    );
    let second_contents = std::fs::read_to_string(&second_path).unwrap();
    let conn = store::open_memory().unwrap();

    crate::conversation::refresh_codex(&conn, home).unwrap();
    std::fs::remove_file(&second_path).unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();

    let missing =
        crate::conversation::sessions_page(&conn, &crate::domain::ConversationQuery::default())
            .unwrap();
    assert_eq!(missing.total, 1);
    assert_eq!(missing.rows[0].ended_at, "2026-08-21T00:00:02Z");
    assert_eq!(missing.rows[0].source_files.len(), 2);
    assert!(!missing.rows[0].file_available);
    assert!(
        crate::conversation::load_parsed_detail(&conn, home, "codex", "partial-loss-1")
            .unwrap_err()
            .contains("详情不可读取")
    );

    std::fs::write(&second_path, second_contents).unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();

    let restored =
        crate::conversation::sessions_page(&conn, &crate::domain::ConversationQuery::default())
            .unwrap();
    assert_eq!(restored.total, 1);
    assert!(restored.rows[0].file_available);
    assert_eq!(restored.rows[0].source_files.len(), 2);
    let detail =
        crate::conversation::load_parsed_detail(&conn, home, "codex", "partial-loss-1").unwrap();
    assert_eq!(
        detail
            .events
            .iter()
            .filter_map(|event| event.text.as_deref())
            .collect::<Vec<_>>(),
        vec!["first", "second"]
    );
}

#[test]
fn codex_conversation_partial_file_loss_isolated_from_unrelated_parse_failure() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let meta = serde_json::json!({
        "type": "session_meta",
        "timestamp": "2026-08-21T00:00:00Z",
        "payload": {"id": "isolated-loss-1", "cwd": "/workspace/isolated-loss"}
    });
    seed_codex_records(
        home,
        "rollout-isolated-loss-a.jsonl",
        &[
            meta.clone(),
            serde_json::json!({
                "type": "response_item",
                "timestamp": "2026-08-21T00:00:01Z",
                "payload": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "first"}]}
            }),
        ],
    );
    let missing_path = seed_codex_records(
        home,
        "rollout-isolated-loss-b.jsonl",
        &[
            meta,
            serde_json::json!({
                "type": "response_item",
                "timestamp": "2026-08-21T00:00:02Z",
                "payload": {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "second"}]}
            }),
        ],
    );
    let failed_path = seed_codex_records(
        home,
        "rollout-unrelated-failure.jsonl",
        &[
            serde_json::json!({
                "type": "session_meta",
                "timestamp": "2026-08-21T00:00:03Z",
                "payload": {"id": "unrelated-failure-1", "cwd": "/workspace/unrelated"}
            }),
            serde_json::json!({
                "type": "response_item",
                "timestamp": "2026-08-21T00:00:04Z",
                "payload": {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "unrelated"}]}
            }),
        ],
    );
    let conn = store::open_memory().unwrap();

    crate::conversation::refresh_codex(&conn, home).unwrap();
    std::fs::remove_file(missing_path).unwrap();
    std::fs::write(failed_path, "{not-json}\n").unwrap();
    let issues = crate::conversation::refresh_codex(&conn, home).unwrap();

    assert_eq!(issues.len(), 1);
    let page =
        crate::conversation::sessions_page(&conn, &crate::domain::ConversationQuery::default())
            .unwrap();
    let preserved = page
        .rows
        .iter()
        .find(|row| row.session_id == "isolated-loss-1")
        .unwrap();
    assert_eq!(preserved.ended_at, "2026-08-21T00:00:02Z");
    assert_eq!(preserved.source_files.len(), 2);
    assert!(!preserved.file_available);
}

#[test]
fn codex_conversation_parse_failure_preserves_the_last_good_multi_file_aggregate() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let meta = serde_json::json!({
        "type": "session_meta",
        "timestamp": "2026-08-21T00:00:00Z",
        "payload": {"id": "last-good-1", "cwd": "/workspace/last-good"}
    });
    let first_path = seed_codex_records(
        home,
        "rollout-last-good-a.jsonl",
        &[
            meta.clone(),
            serde_json::json!({
                "type": "response_item",
                "timestamp": "2026-08-21T00:00:01Z",
                "payload": {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "first"}]}
            }),
        ],
    );
    let second_path = seed_codex_records(
        home,
        "rollout-last-good-b.jsonl",
        &[
            meta,
            serde_json::json!({
                "type": "response_item",
                "timestamp": "2026-08-21T00:00:02Z",
                "payload": {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "second"}]}
            }),
        ],
    );
    let conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();

    let mut first = std::fs::read_to_string(&first_path).unwrap();
    first.push_str(
        &serde_json::json!({
            "type": "response_item",
            "timestamp": "2026-08-21T00:00:09Z",
            "payload": {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "partial update"}]}
        })
        .to_string(),
    );
    first.push('\n');
    std::fs::write(first_path, first).unwrap();
    std::fs::write(second_path, "{not-json}\n").unwrap();

    let issues = crate::conversation::refresh_codex(&conn, home).unwrap();
    let page =
        crate::conversation::sessions_page(&conn, &crate::domain::ConversationQuery::default())
            .unwrap();

    assert_eq!(issues.len(), 1);
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.rows[0].ended_at, "2026-08-21T00:00:02Z");
    assert_eq!(page.rows[0].source_files.len(), 2);
}

#[test]
fn codex_conversation_links_structured_child_agents_and_preserves_launch_events() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_codex_records(
        home,
        "rollout-parent.jsonl",
        &[
            serde_json::json!({
                "type": "session_meta",
                "timestamp": "2026-08-21T00:00:00Z",
                "payload": {"id": "parent-1", "cwd": "/workspace/agents", "title": "Parent"}
            }),
            serde_json::json!({
                "type": "response_item",
                "timestamp": "2026-08-21T00:00:01Z",
                "payload": {"type": "function_call", "name": "spawn_agent", "call_id": "spawn-1", "arguments": "{\"message\":\"Inspect child work\"}"}
            }),
            serde_json::json!({
                "type": "response_item",
                "timestamp": "2026-08-21T00:00:02Z",
                "payload": {"type": "function_call_output", "call_id": "spawn-1", "output": "{\"agent_id\":\"child-1\"}"}
            }),
        ],
    );
    seed_codex_records(
        home,
        "rollout-child.jsonl",
        &[
            serde_json::json!({
                "type": "session_meta",
                "timestamp": "2026-08-21T00:00:03Z",
                "payload": {"id": "child-1", "parent_id": "parent-1", "cwd": "/workspace/agents", "title": "Child"}
            }),
            serde_json::json!({
                "type": "response_item",
                "timestamp": "2026-08-21T00:00:04Z",
                "payload": {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "child result"}]}
            }),
        ],
    );
    let conn = store::open_memory().unwrap();

    crate::conversation::refresh_codex(&conn, home).unwrap();
    let parent = crate::conversation::load_parsed_detail(&conn, home, "codex", "parent-1").unwrap();
    let child = crate::conversation::load_parsed_detail(&conn, home, "codex", "child-1").unwrap();

    let launch = parent
        .events
        .iter()
        .find(|event| event.name.as_deref() == Some("spawn_agent"))
        .unwrap();
    assert_eq!(parent.agent_relations.children.len(), 1);
    let child_link = &parent.agent_relations.children[0];
    assert_eq!(
        child_link.status,
        crate::domain::ConversationAgentLinkStatus::Linked
    );
    assert_eq!(
        child_link.launch_event_id.as_deref(),
        Some(launch.event_id.as_str())
    );
    assert_eq!(child_link.session.as_ref().unwrap().session_id, "child-1");
    assert_eq!(
        child.events.last().unwrap().text.as_deref(),
        Some("child result")
    );
    assert_eq!(
        child
            .agent_relations
            .parent
            .as_ref()
            .and_then(|link| link.session.as_ref())
            .map(|session| session.session_id.as_str()),
        Some("parent-1")
    );
}

#[test]
fn codex_conversation_rejects_fuzzy_child_merging_and_reports_unavailable_linkage() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_codex_records(
        home,
        "rollout-unresolved-parent.jsonl",
        &[
            serde_json::json!({
                "type": "session_meta",
                "timestamp": "2026-08-21T00:00:00Z",
                "payload": {"id": "unresolved-parent", "cwd": "/workspace/same", "title": "Same title"}
            }),
            serde_json::json!({
                "type": "response_item",
                "timestamp": "2026-08-21T00:00:01Z",
                "payload": {"type": "function_call", "name": "spawn_agent", "call_id": "spawn-plain", "arguments": "{}"}
            }),
            serde_json::json!({
                "type": "response_item",
                "timestamp": "2026-08-21T00:00:02Z",
                "payload": {"type": "function_call_output", "call_id": "spawn-plain", "output": "agent_id: possible-child"}
            }),
        ],
    );
    seed_codex_records(
        home,
        "rollout-possible-child.jsonl",
        &[serde_json::json!({
            "type": "session_meta",
            "timestamp": "2026-08-21T00:00:02Z",
            "payload": {"id": "possible-child", "cwd": "/workspace/same", "title": "Same title"}
        })],
    );
    let conn = store::open_memory().unwrap();

    crate::conversation::refresh_codex(&conn, home).unwrap();
    let page =
        crate::conversation::sessions_page(&conn, &crate::domain::ConversationQuery::default())
            .unwrap();
    let parent =
        crate::conversation::load_parsed_detail(&conn, home, "codex", "unresolved-parent").unwrap();
    let candidate =
        crate::conversation::load_parsed_detail(&conn, home, "codex", "possible-child").unwrap();

    assert_eq!(page.total, 2);
    assert_eq!(parent.agent_relations.children.len(), 1);
    assert_eq!(
        parent.agent_relations.capability_status,
        crate::domain::ConversationAgentCapabilityStatus::Unavailable
    );
    assert_eq!(
        parent.agent_relations.children[0].status,
        crate::domain::ConversationAgentLinkStatus::Unresolved
    );
    assert!(parent.agent_relations.children[0].session.is_none());
    assert!(candidate.agent_relations.parent.is_none());
}

#[test]
fn codex_conversation_detail_links_existing_usage_by_exact_source_and_session_id() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_codex_fixture(
        home,
        "rollout-semantic-1.jsonl",
        "codex-semantic-events.jsonl",
    );
    let conn = store::open_memory().unwrap();
    let mut early = rec(
        "2026-08-21T00:00:05Z",
        Source::Codex,
        "gpt-5.6-sol",
        "openai",
        "/workspace/semantic-project",
        "semantic-1",
        110,
    );
    early.output_tokens = 10;
    let mut early_copy = early.clone();
    early_copy.source_file = "duplicate-channel.jsonl".to_string();
    let late = rec(
        "2026-08-21T00:01:00Z",
        Source::Codex,
        "gpt-5.7-codex",
        "openai",
        "/workspace/semantic-project",
        "semantic-1",
        220,
    );
    let wrong_source = rec(
        "2026-08-21T00:02:00Z",
        Source::Claude,
        "claude-sonnet-5",
        "anthropic",
        "/workspace/semantic-project",
        "semantic-1",
        330,
    );
    let wrong_session = rec(
        "2026-08-21T00:03:00Z",
        Source::Codex,
        "gpt-5.7-codex",
        "openai",
        "/workspace/semantic-project",
        "semantic-2",
        440,
    );
    store::insert_records(
        &conn,
        &[late, wrong_source, early_copy, early, wrong_session],
    )
    .unwrap();

    crate::conversation::refresh_codex(&conn, home).unwrap();
    let detail =
        crate::conversation::load_parsed_detail(&conn, home, "codex", "semantic-1").unwrap();
    assert_eq!(detail.session.session_id, "semantic-1");

    let usage = usage_rows(&conn, "codex", "semantic-1");
    assert_eq!(usage.len(), 2);
    assert_eq!(usage[0].occurred_at, "2026-08-21T00:00:05Z");
    assert_eq!(usage[0].output_tokens, 10);
    assert_eq!(usage[1].occurred_at, "2026-08-21T00:01:00Z");
    assert!(usage
        .iter()
        .all(|record| record.source == Source::Codex && record.session_id == "semantic-1"));
}

#[test]
fn conversation_usage_records_page_returns_the_first_page() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_codex_fixture(
        home,
        "rollout-semantic-1.jsonl",
        "codex-semantic-events.jsonl",
    );
    let conn = store::open_memory().unwrap();
    store::insert_records(
        &conn,
        &[
            rec(
                "2026-08-21T00:00:01Z",
                Source::Codex,
                "gpt-5.6-sol",
                "openai",
                "/workspace/semantic-project",
                "semantic-1",
                10,
            ),
            rec(
                "2026-08-21T00:00:02Z",
                Source::Codex,
                "gpt-5.6-sol",
                "openai",
                "/workspace/semantic-project",
                "semantic-1",
                20,
            ),
            rec(
                "2026-08-21T00:00:03Z",
                Source::Codex,
                "gpt-5.6-sol",
                "openai",
                "/workspace/semantic-project",
                "semantic-1",
                30,
            ),
            rec(
                "2026-08-21T00:00:04Z",
                Source::Codex,
                "gpt-5.6-sol",
                "openai",
                "/workspace/semantic-project",
                "semantic-1",
                40,
            ),
            rec(
                "2026-08-21T00:00:05Z",
                Source::Codex,
                "gpt-5.6-sol",
                "openai",
                "/workspace/semantic-project",
                "semantic-1",
                50,
            ),
        ],
    )
    .unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();

    let page = crate::conversation::usage_records_page(&conn, "codex", "semantic-1", 1, 2).unwrap();

    assert_eq!(page.total, 5);
    assert_eq!(page.rows.len(), 2);
    assert_eq!(page.rows[0].occurred_at, "2026-08-21T00:00:01Z");
    assert_eq!(page.rows[0].total_tokens, 10);
    assert_eq!(page.rows[1].occurred_at, "2026-08-21T00:00:02Z");
    assert_eq!(page.rows[1].total_tokens, 20);
}

fn seed_five_codex_usage_records(home: &std::path::Path) -> rusqlite::Connection {
    seed_codex_fixture(
        home,
        "rollout-semantic-1.jsonl",
        "codex-semantic-events.jsonl",
    );
    let conn = store::open_memory().unwrap();
    store::insert_records(
        &conn,
        &(1..=5)
            .map(|index| {
                rec(
                    &format!("2026-08-21T00:00:0{index}Z"),
                    Source::Codex,
                    "gpt-5.6-sol",
                    "openai",
                    "/workspace/semantic-project",
                    "semantic-1",
                    index * 10,
                )
            })
            .collect::<Vec<_>>(),
    )
    .unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();
    conn
}

#[test]
fn conversation_usage_records_page_returns_the_last_partial_page() {
    let temp = tempfile::tempdir().unwrap();
    let conn = seed_five_codex_usage_records(temp.path());

    let page = crate::conversation::usage_records_page(&conn, "codex", "semantic-1", 3, 2).unwrap();

    assert_eq!(page.total, 5);
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.rows[0].occurred_at, "2026-08-21T00:00:05Z");
    assert_eq!(page.rows[0].total_tokens, 50);
}

#[test]
fn conversation_usage_records_page_past_the_end_is_empty() {
    let temp = tempfile::tempdir().unwrap();
    let conn = seed_five_codex_usage_records(temp.path());

    let page = crate::conversation::usage_records_page(&conn, "codex", "semantic-1", 4, 2).unwrap();

    assert_eq!(page.total, 5);
    assert!(page.rows.is_empty());
}

#[test]
fn conversation_usage_records_page_is_empty_when_the_session_has_no_records() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_codex_conversation(home);
    let conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();

    let page = crate::conversation::usage_records_page(&conn, "codex", "conv-1", 1, 20).unwrap();

    assert_eq!(page.total, 0);
    assert!(page.rows.is_empty());
}

#[test]
fn conversation_catalog_attaches_usage_totals_by_exact_source_and_session_id() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_codex_fixture(
        home,
        "rollout-semantic-1.jsonl",
        "codex-semantic-events.jsonl",
    );
    let conn = store::open_memory().unwrap();
    let mut early = rec(
        "2026-08-21T00:00:05Z",
        Source::Codex,
        "gpt-5.6-sol",
        "openai",
        "/workspace/semantic-project",
        "semantic-1",
        110,
    );
    early.native_cost = Some(0.10);
    let mut late = rec(
        "2026-08-21T00:01:00Z",
        Source::Codex,
        "gpt-5.7-codex",
        "openai",
        "/workspace/semantic-project",
        "semantic-1",
        220,
    );
    late.native_cost = Some(0.20);
    let wrong_source = rec(
        "2026-08-21T00:02:00Z",
        Source::Claude,
        "claude-sonnet-5",
        "anthropic",
        "/workspace/semantic-project",
        "semantic-1",
        330,
    );
    let wrong_session = rec(
        "2026-08-21T00:03:00Z",
        Source::Codex,
        "gpt-5.7-codex",
        "openai",
        "/workspace/semantic-project",
        "semantic-2",
        440,
    );
    store::insert_records(&conn, &[late, wrong_source, early, wrong_session]).unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();

    let page =
        crate::conversation::sessions_page(&conn, &crate::domain::ConversationQuery::default())
            .unwrap();
    let row = page
        .rows
        .iter()
        .find(|row| row.session_id == "semantic-1")
        .expect("indexed conversation");
    assert_eq!(row.total_tokens, 330);
    assert!((row.cost.unwrap_or_default() - 0.30).abs() < 1e-9);
    assert!(!row.unpriced);
}

#[test]
fn codex_conversation_catalog_indexes_and_loads_messages_without_caching_body() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let source_file = seed_codex_conversation(home);
    let conn = store::open_memory().unwrap();

    crate::conversation::refresh_codex(&conn, home).unwrap();
    let page =
        crate::conversation::sessions_page(&conn, &crate::domain::ConversationQuery::default())
            .unwrap();

    assert_eq!(page.total, 1);
    assert_eq!(page.rows.len(), 1);
    let row = &page.rows[0];
    assert_eq!(row.source, "codex");
    assert_eq!(row.session_id, "conv-1");
    assert_eq!(row.title, "发布 Tray 客户端版本支持图片编辑透传");
    assert_eq!(row.project, "/workspace/example-project");
    assert_eq!(row.model, "gpt-5.6-sol");
    assert_eq!(row.started_at, "2026-08-20T00:00:00Z");
    assert_eq!(row.ended_at, "2026-08-20T00:03:00Z");
    assert_eq!(row.capabilities, vec!["messages", "events", "usage"]);
    assert_eq!(row.support_status, "experimental");
    assert!(row.file_available);

    let detail = crate::conversation::load_parsed_detail(&conn, home, "codex", "conv-1").unwrap();
    assert_eq!(detail.session, *row);
    let messages = message_events(&detail);
    assert_eq!(messages.len(), 3);
    assert_eq!(
        messages[0].actor.map(ConversationEventActor::as_str),
        Some("user")
    );
    assert_eq!(
        messages[0].text.as_deref(),
        Some("发布 Tray 客户端版本支持图片编辑透传")
    );
    assert_eq!(
        messages[1].actor.map(ConversationEventActor::as_str),
        Some("assistant")
    );
    assert_eq!(messages[1].text.as_deref(), Some("我先检查现有实现。"));
    assert_eq!(messages[2].text.as_deref(), Some("已完成提交。"));

    std::fs::remove_file(source_file).unwrap();
    let error = crate::conversation::load_detail(&conn, home, "codex", "conv-1").unwrap_err();
    assert!(error.contains("原文件已删除"), "unexpected error: {error}");
    assert!(error.contains("详情不可读取"), "unexpected error: {error}");
}

#[test]
fn codex_conversation_catalog_searches_only_indexed_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_codex_conversation(home);
    let conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();

    for search in [
        "Tray",
        "codex",
        "example-project",
        "gpt-5.6-sol",
        "conv-1",
        "2026-08-20",
    ] {
        let page = crate::conversation::sessions_page(
            &conn,
            &crate::domain::ConversationQuery {
                search: Some(search.to_string()),
                page: Some(1),
                page_size: Some(10),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(page.total, 1, "search should match: {search}");
    }

    let missing = crate::conversation::sessions_page(
        &conn,
        &crate::domain::ConversationQuery {
            search: Some("我先检查现有实现".to_string()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(missing.total, 0, "正文不应进入元数据搜索索引");
}

#[test]
fn conversation_catalog_filters_by_source_and_project() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_conversation_fixture(
        home,
        ".claude/projects/-workspace-claude-app/claude-parent-1.jsonl",
        "claude-conversation.jsonl",
    );
    seed_conversation_fixture(
        home,
        ".pi/agent/sessions/pi-session-1.jsonl",
        "pi-conversation.jsonl",
    );
    let overrides = crate::ingest::PathOverrides::from([
        ("CLAUDE_CONFIG_DIR", vec![home.join(".claude")]),
        ("PI_AGENT_DIR", vec![home.join(".pi/agent/sessions")]),
    ]);
    let conn = store::open_memory().unwrap();
    crate::ingest::ingest_all_with_overrides(&conn, home, &overrides).unwrap();

    let all =
        crate::conversation::sessions_page(&conn, &crate::domain::ConversationQuery::default())
            .unwrap();
    assert_eq!(all.total, 2);

    let claude = crate::conversation::sessions_page(
        &conn,
        &crate::domain::ConversationQuery {
            sources: vec!["claude".into()],
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(claude.total, 1);
    assert_eq!(claude.rows[0].source, "claude");
    assert_eq!(claude.rows[0].session_id, "claude-parent-1");

    let pi_project = crate::conversation::sessions_page(
        &conn,
        &crate::domain::ConversationQuery {
            projects: vec!["/workspace/pi-app".into()],
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(pi_project.total, 1);
    assert_eq!(pi_project.rows[0].session_id, "pi-session-1");

    let miss = crate::conversation::sessions_page(
        &conn,
        &crate::domain::ConversationQuery {
            sources: vec!["claude".into()],
            projects: vec!["/workspace/pi-app".into()],
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(miss.total, 0);
}

#[test]
fn conversation_catalog_filters_by_model_provider_and_range() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_codex_conversation(home);
    let conn = store::open_memory().unwrap();
    store::insert_records(
        &conn,
        &[rec(
            "2026-08-20T00:01:00Z",
            Source::Codex,
            "gpt-5.6-sol",
            "openai",
            "/workspace/example-project",
            "conv-1",
            10,
        )],
    )
    .unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();

    let by_model = crate::conversation::sessions_page(
        &conn,
        &crate::domain::ConversationQuery {
            models: vec!["gpt-5.6-sol".into()],
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(by_model.total, 1);

    let miss_model = crate::conversation::sessions_page(
        &conn,
        &crate::domain::ConversationQuery {
            models: vec!["claude-sonnet-test".into()],
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(miss_model.total, 0);

    let by_provider = crate::conversation::sessions_page(
        &conn,
        &crate::domain::ConversationQuery {
            providers: vec!["openai".into()],
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(by_provider.total, 1);

    let miss_provider = crate::conversation::sessions_page(
        &conn,
        &crate::domain::ConversationQuery {
            providers: vec!["anthropic".into()],
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(miss_provider.total, 0);

    let in_range = crate::conversation::sessions_page(
        &conn,
        &crate::domain::ConversationQuery {
            from: Some("2026-08-19T00:00:00Z".into()),
            to: Some("2026-08-21T00:00:00Z".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(in_range.total, 1);

    let out_of_range = crate::conversation::sessions_page(
        &conn,
        &crate::domain::ConversationQuery {
            from: Some("2026-08-21T00:00:00Z".into()),
            to: Some("2026-08-22T00:00:00Z".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(out_of_range.total, 0);
}

#[test]
fn codex_conversation_refresh_tombstones_deleted_files_and_revives_the_same_session() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let first = seed_codex_conversation(home);
    let second = home.join(".codex/sessions/2026/08/rollout-conv-2.jsonl");
    std::fs::write(
        &second,
        fixture("codex-conversation.jsonl").replace("conv-1", "conv-2"),
    )
    .unwrap();
    let conn = store::open_memory().unwrap();

    crate::conversation::refresh_codex(&conn, home).unwrap();
    assert_eq!(
        crate::conversation::sessions_page(&conn, &crate::domain::ConversationQuery::default())
            .unwrap()
            .total,
        2
    );

    std::fs::remove_file(&second).unwrap();
    let issues = crate::conversation::refresh_codex(&conn, home).unwrap();
    assert!(issues.is_empty());
    let page =
        crate::conversation::sessions_page(&conn, &crate::domain::ConversationQuery::default())
            .unwrap();
    assert_eq!(page.total, 2, "删除源文件后必须保留目录索引");
    let deleted = page
        .rows
        .iter()
        .find(|row| row.session_id == "conv-2")
        .unwrap();
    assert!(!deleted.file_available);
    let error = crate::conversation::load_detail(&conn, home, "codex", "conv-2").unwrap_err();
    assert!(error.contains("原文件已删除"), "unexpected error: {error}");
    assert!(error.contains("详情不可读取"), "unexpected error: {error}");

    std::fs::write(
        &second,
        fixture("codex-conversation.jsonl").replace("conv-1", "conv-2"),
    )
    .unwrap();
    assert!(crate::conversation::refresh_codex(&conn, home)
        .unwrap()
        .is_empty());
    let revived =
        crate::conversation::sessions_page(&conn, &crate::domain::ConversationQuery::default())
            .unwrap();
    assert_eq!(revived.total, 2, "恢复原路径不得生成重复目录项");
    let revived_row = revived
        .rows
        .iter()
        .find(|row| row.session_id == "conv-2")
        .unwrap();
    assert!(revived_row.file_available);
    crate::conversation::load_parsed_detail(&conn, home, "codex", "conv-2").unwrap();

    assert!(first.exists());
}

#[test]
fn codex_conversation_refresh_skips_unchanged_available_files() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_codex_conversation(home);
    let conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();
    conn.execute(
        "UPDATE conversation_sessions SET title = 'cached-title' WHERE source = 'codex' AND session_id = 'conv-1'",
        [],
    )
    .unwrap();

    assert!(crate::conversation::refresh_codex(&conn, home)
        .unwrap()
        .is_empty());
    let page =
        crate::conversation::sessions_page(&conn, &crate::domain::ConversationQuery::default())
            .unwrap();
    assert_eq!(page.rows[0].title, "cached-title");
}

#[test]
fn rebuilding_source_reparses_unchanged_conversation_index() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_codex_conversation(home);
    let conn = store::open_memory().unwrap();
    ingest::ingest_all(&conn, home).unwrap();
    conn.execute(
        "UPDATE conversation_sessions SET title = 'cached-title' WHERE source = 'codex' AND session_id = 'conv-1'",
        [],
    )
    .unwrap();

    let report = ingest::rebuild_cache(&conn, home, Some(Source::Codex)).unwrap();
    let page =
        crate::conversation::sessions_page(&conn, &crate::domain::ConversationQuery::default())
            .unwrap();

    assert!(report.conversation_issues.is_empty());
    assert_eq!(page.rows[0].title, "发布 Tray 客户端版本支持图片编辑透传");
}

#[test]
fn conversation_adapter_version_change_defers_reparse_to_backfill() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_codex_conversation(home);
    let conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();
    conn.execute_batch(
        r#"
        UPDATE conversation_sessions
        SET title = 'cached-title', adapter_version = 8
        WHERE source = 'codex' AND session_id = 'conv-1';
        UPDATE conversation_session_files
        SET adapter_version = 8
        WHERE source = 'codex' AND session_id = 'conv-1';
        "#,
    )
    .unwrap();

    crate::conversation::refresh_codex(&conn, home).unwrap();
    let page =
        crate::conversation::sessions_page(&conn, &crate::domain::ConversationQuery::default())
            .unwrap();
    assert_eq!(page.rows[0].title, "cached-title");
    assert_eq!(
        crate::conversation::event_index_progress(&conn).unwrap(),
        crate::domain::ConversationIndexProgressDto {
            indexed: 0,
            total: 1,
        }
    );

    crate::conversation::backfill_event_index(&conn, home).unwrap();
    let page =
        crate::conversation::sessions_page(&conn, &crate::domain::ConversationQuery::default())
            .unwrap();
    let versions = conn
        .query_row(
            r#"
            SELECT sessions.adapter_version, files.adapter_version
            FROM conversation_sessions AS sessions
            JOIN conversation_session_files AS files
              ON files.source = sessions.source AND files.session_id = sessions.session_id
            WHERE sessions.source = 'codex' AND sessions.session_id = 'conv-1'
            "#,
            [],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .unwrap();

    assert_eq!(page.rows[0].title, "发布 Tray 客户端版本支持图片编辑透传");
    assert_eq!(
        versions,
        (
            crate::conversation::CONVERSATION_ADAPTER_VERSION,
            crate::conversation::CONVERSATION_ADAPTER_VERSION,
        )
    );
}

#[test]
fn codex_conversation_refresh_reparses_same_millisecond_nanosecond_change() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_codex_conversation(home);
    let conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();
    conn.execute_batch(
        r#"
        UPDATE conversation_sessions
        SET title = 'cached-title'
        WHERE source = 'codex' AND session_id = 'conv-1';
        UPDATE conversation_session_files
        SET source_file_mtime_ns =
                (source_file_mtime_ns / 1000000) * 1000000
                + CASE source_file_mtime_ns % 1000000 WHEN 1 THEN 2 ELSE 1 END
        WHERE source = 'codex' AND session_id = 'conv-1';
        "#,
    )
    .unwrap();

    assert!(crate::conversation::refresh_codex(&conn, home)
        .unwrap()
        .is_empty());
    let page =
        crate::conversation::sessions_page(&conn, &crate::domain::ConversationQuery::default())
            .unwrap();
    assert_eq!(page.rows[0].title, "发布 Tray 客户端版本支持图片编辑透传");
}

#[test]
fn codex_conversation_refresh_reparses_when_file_size_changes() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let path = seed_codex_conversation(home);
    let conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();
    let original_size = std::fs::metadata(&path).unwrap().len();
    let updated_title = "发布 Tray 客户端版本支持图片编辑透传并记录更长标题";
    let updated = fixture("codex-conversation.jsonl")
        .replace("发布 Tray 客户端版本支持图片编辑透传", updated_title);
    std::fs::write(&path, updated).unwrap();
    let updated_size = std::fs::metadata(&path).unwrap().len();
    assert_ne!(updated_size, original_size);

    assert!(crate::conversation::refresh_codex(&conn, home)
        .unwrap()
        .is_empty());

    let page =
        crate::conversation::sessions_page(&conn, &crate::domain::ConversationQuery::default())
            .unwrap();
    assert_eq!(page.rows[0].title, updated_title);
    let cached_size: i64 = conn
        .query_row(
            "SELECT source_file_size FROM conversation_sessions WHERE source = 'codex' AND session_id = 'conv-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(cached_size, updated_size as i64);
}

#[test]
fn codex_conversation_refresh_reparses_an_ambiguous_shared_path() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_codex_conversation(home);
    let conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();
    conn.execute(
        "UPDATE conversation_sessions SET title = 'cached-title' WHERE source = 'codex' AND session_id = 'conv-1'",
        [],
    )
    .unwrap();
    conn.execute_batch(
        r#"
        INSERT INTO conversation_sessions(
            source, session_id, title, project, model, started_at, ended_at,
            source_file, capabilities_json, support_status, file_available,
            source_file_mtime_ms, source_file_size
        )
        SELECT source, 'aaa-history', 'history', project, model, started_at,
               '9999-01-01T00:00:00Z', source_file, capabilities_json, support_status, 1,
               source_file_mtime_ms, source_file_size
        FROM conversation_sessions
        WHERE source = 'codex' AND session_id = 'conv-1';
        "#,
    )
    .unwrap();

    assert!(crate::conversation::refresh_codex(&conn, home)
        .unwrap()
        .is_empty());

    let page =
        crate::conversation::sessions_page(&conn, &crate::domain::ConversationQuery::default())
            .unwrap();
    let current = page
        .rows
        .iter()
        .find(|row| row.session_id == "conv-1")
        .unwrap();
    let history = page
        .rows
        .iter()
        .find(|row| row.session_id == "aaa-history")
        .unwrap();
    assert!(current.file_available);
    assert_eq!(current.title, "发布 Tray 客户端版本支持图片编辑透传");
    assert!(!history.file_available);
}

#[test]
fn codex_conversation_parse_failure_preserves_metadata_and_reports_safe_location() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let path = seed_codex_conversation(home);
    let removed = home.join(".codex/sessions/2026/08/rollout-conv-2.jsonl");
    std::fs::write(
        &removed,
        fixture("codex-conversation.jsonl").replace("conv-1", "conv-2"),
    )
    .unwrap();
    let conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();
    let before =
        crate::conversation::sessions_page(&conn, &crate::domain::ConversationQuery::default())
            .unwrap()
            .rows[0]
            .clone();
    let secret = "PRIVATE_PROMPT_MUST_NOT_APPEAR";
    std::fs::write(
        &path,
        format!(
            "{}\n{{not-json \"secret\":\"{secret}\"}}\n",
            fixture("codex-conversation.jsonl").trim_end()
        ),
    )
    .unwrap();
    std::fs::remove_file(removed).unwrap();

    let report = ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();

    let page =
        crate::conversation::sessions_page(&conn, &crate::domain::ConversationQuery::default())
            .unwrap();
    assert_eq!(page.total, 2, "解析失败时不得执行墓碑对账");
    let after = page
        .rows
        .iter()
        .find(|row| row.session_id == "conv-1")
        .unwrap();
    assert_eq!(after.title, before.title);
    assert_eq!(after.project, before.project);
    assert_eq!(after.model, before.model);
    assert!(
        page.rows
            .iter()
            .find(|row| row.session_id == "conv-2")
            .unwrap()
            .file_available
    );
    assert_eq!(report.conversation_issues.len(), 1);
    let issue = serde_json::to_value(&report.conversation_issues[0]).unwrap();
    assert_eq!(issue["event_type"], "json_line");
    assert_eq!(issue["line"], 8);
    assert!(!issue["message"].as_str().unwrap().contains(secret));
    assert!(!issue.to_string().contains(secret));
}

#[test]
fn conversation_schema_migrates_lifecycle_columns_for_old_caches() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("old-cache.sqlite");
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE conversation_sessions (
                source TEXT NOT NULL,
                session_id TEXT NOT NULL,
                title TEXT NOT NULL,
                project TEXT NOT NULL,
                model TEXT NOT NULL,
                started_at TEXT NOT NULL,
                ended_at TEXT NOT NULL,
                source_file TEXT NOT NULL,
                capabilities_json TEXT NOT NULL DEFAULT '[]',
                support_status TEXT NOT NULL DEFAULT 'experimental',
                PRIMARY KEY(source, session_id)
            );
            INSERT INTO conversation_sessions(
                source, session_id, title, project, model, started_at, ended_at, source_file
            ) VALUES('codex', 'legacy', '旧索引', '', '', '', '', 'legacy.jsonl');
            "#,
        )
        .unwrap();
    }

    let conn = store::open_db(db_path.to_str().unwrap()).unwrap();
    let lifecycle = conn
        .query_row(
            "SELECT file_available, source_file_mtime_ms, source_file_mtime_ns, source_file_size, adapter_version FROM conversation_sessions WHERE session_id = 'legacy'",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(lifecycle, (1, 0, 0, 0, 0));
    let indexes: Vec<String> = conn
        .prepare("PRAGMA index_list(conversation_sessions)")
        .unwrap()
        .query_map([], |row| row.get(1))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(indexes.contains(&"idx_conversation_sessions_source_file".to_string()));
}

#[test]
fn codex_conversation_detail_rejects_indexed_path_outside_source_root() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_codex_conversation(home);
    let outside = home.join("outside.jsonl");
    std::fs::write(&outside, fixture("codex-conversation.jsonl")).unwrap();
    let conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();
    conn.execute(
        "UPDATE conversation_sessions SET source_file = ?1 WHERE source = 'codex' AND session_id = 'conv-1'",
        rusqlite::params![outside.to_string_lossy().to_string()],
    )
    .unwrap();

    let error = crate::conversation::load_detail(&conn, home, "codex", "conv-1").unwrap_err();
    assert!(
        error.contains("允许的扫描目录"),
        "unexpected error: {error}"
    );
}

#[test]
fn ingest_all_refreshes_codex_conversation_catalog_without_usage_records() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_codex_conversation(home);
    let conn = store::open_memory().unwrap();

    let report = ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    assert_eq!(report.files_failed, 0);
    let records = store::load_all(&conn).unwrap();
    assert!(records.is_empty(), "unexpected usage records: {records:?}");

    let page =
        crate::conversation::sessions_page(&conn, &crate::domain::ConversationQuery::default())
            .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.rows[0].title, "发布 Tray 客户端版本支持图片编辑透传");
    assert_eq!(page.rows[0].total_tokens, 0);
    assert_eq!(page.rows[0].cost, None);
    assert!(!page.rows[0].unpriced);
}

#[test]
fn configured_claude_pi_and_gemini_roots_feed_the_unified_conversation_services() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let claude_root = home.join(".claude");
    let pi_root = home.join(".pi/agent/sessions");
    let gemini_root = home.join(".gemini/tmp");
    seed_conversation_fixture(
        home,
        ".claude/projects/-workspace-claude-app/claude-parent-1.jsonl",
        "claude-conversation.jsonl",
    );
    seed_conversation_fixture(
        home,
        ".claude/projects/-workspace-claude-app/claude-parent-1/subagents/agent-claude-child-1.jsonl",
        "claude-subagent-conversation.jsonl",
    );
    seed_conversation_fixture(
        home,
        ".claude/projects/-workspace-claude-app/claude-parent-1/subagents/agent-claude-child-2.jsonl",
        "claude-subagent-conversation-2.jsonl",
    );
    seed_conversation_fixture(
        home,
        ".claude/projects/-workspace-claude-empty/claude-no-usage.jsonl",
        "claude-conversation-no-usage.jsonl",
    );
    seed_conversation_fixture(
        home,
        ".pi/agent/sessions/pi-session-1.jsonl",
        "pi-conversation.jsonl",
    );
    seed_conversation_fixture(
        home,
        ".pi/agent/sessions/pi-no-usage.jsonl",
        "pi-conversation-no-usage.jsonl",
    );
    seed_conversation_fixture(
        home,
        ".gemini/tmp/gemini-project/chats/session-gemini-session-1.json",
        "gemini-conversation.json",
    );
    seed_conversation_fixture(
        home,
        ".gemini/tmp/gemini-empty/chats/session-gemini-no-usage.json",
        "gemini-conversation-no-usage.json",
    );
    let overrides = crate::ingest::PathOverrides::from([
        ("CLAUDE_CONFIG_DIR", vec![claude_root]),
        ("PI_AGENT_DIR", vec![pi_root]),
        ("GEMINI_DATA_DIR", vec![gemini_root]),
    ]);
    let conn = store::open_memory().unwrap();

    let report = crate::ingest::ingest_all_with_overrides(&conn, home, &overrides).unwrap();
    let page =
        crate::conversation::sessions_page(&conn, &crate::domain::ConversationQuery::default())
            .unwrap();

    assert!(report.conversation_issues.is_empty());
    assert_eq!(page.total, 6);
    assert!(page
        .rows
        .iter()
        .all(|row| row.support_status == "experimental"));
    let identities = page
        .rows
        .iter()
        .map(|row| (row.source.as_str(), row.session_id.as_str()))
        .collect::<std::collections::BTreeSet<_>>();
    assert!(identities.contains(&("claude", "claude-parent-1")));
    assert!(identities.contains(&("claude", "claude-no-usage")));
    assert!(identities.contains(&("pi", "pi-session-1")));
    assert!(identities.contains(&("pi", "pi-no-usage")));
    assert!(identities.contains(&("gemini", "gemini-session-1")));
    assert!(identities.contains(&("gemini", "gemini-no-usage")));
    assert!(!identities
        .iter()
        .any(|(_, session_id)| { matches!(*session_id, "claude-child-1" | "claude-child-2") }));

    let claude =
        crate::conversation::load_parsed_detail(&conn, home, "claude", "claude-parent-1").unwrap();
    assert_eq!(usage_rows(&conn, "claude", "claude-parent-1").len(), 1);
    assert!(claude.events.iter().any(|event| {
        event.kind == crate::domain::ConversationEventKind::ToolCall
            && event.name.as_deref() == Some("Agent")
    }));
    assert_eq!(claude.agent_relations.children.len(), 2);
    let child_links = claude
        .agent_relations
        .children
        .iter()
        .filter_map(|link| {
            link.session
                .as_ref()
                .map(|session| (session.session_id.as_str(), link.launch_event_id.as_deref()))
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(child_links.len(), 2);
    assert!(child_links["claude-child-1"].is_some());
    assert!(child_links["claude-child-2"].is_none());
    let child =
        crate::conversation::load_parsed_detail(&conn, home, "claude", "claude-child-1").unwrap();
    let child_usage = usage_rows(&conn, "claude", "claude-child-1");
    assert_eq!(child_usage.len(), 1);
    assert_eq!(child_usage[0].session_id, "claude-child-1");
    assert!(child
        .events
        .iter()
        .any(|event| event.text.as_deref() == Some("The audit is complete.")));
    let parallel_child =
        crate::conversation::load_parsed_detail(&conn, home, "claude", "claude-child-2").unwrap();
    assert!(parallel_child
        .events
        .iter()
        .any(|event| event.text.as_deref() == Some("The test audit is complete.")));
    assert!(!parallel_child
        .events
        .iter()
        .any(|event| event.text.as_deref() == Some("The audit is complete.")));

    let pi = crate::conversation::load_parsed_detail(&conn, home, "pi", "pi-session-1").unwrap();
    assert_eq!(usage_rows(&conn, "pi", "pi-session-1").len(), 1);
    assert!(pi.events.iter().any(|event| {
        event.kind == crate::domain::ConversationEventKind::ModelChange
            && event.name.as_deref() == Some("pi-model-test")
    }));
    assert!(pi.events.iter().any(|event| {
        event.kind == crate::domain::ConversationEventKind::ToolCall
            && event.name.as_deref() == Some("read")
    }));
    assert!(pi
        .events
        .iter()
        .any(|event| event.kind == crate::domain::ConversationEventKind::ToolResult));
    let pi_no_usage =
        crate::conversation::load_parsed_detail(&conn, home, "pi", "pi-no-usage").unwrap();
    assert!(usage_rows(&conn, "pi", "pi-no-usage").is_empty());
    assert!(pi_no_usage.session.model.is_empty());
    assert!(pi_no_usage
        .session
        .capabilities
        .contains(&"usage".to_string()));
    assert_eq!(message_texts(&pi_no_usage).len(), 2);
    assert_eq!(
        pi.events
            .iter()
            .map(|event| event.event_id.as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        pi.events.len()
    );

    let gemini =
        crate::conversation::load_parsed_detail(&conn, home, "gemini", "gemini-session-1").unwrap();
    assert_eq!(usage_rows(&conn, "gemini", "gemini-session-1").len(), 1);
    assert!(gemini.events.iter().any(|event| {
        event.kind == crate::domain::ConversationEventKind::ToolCall
            && event.name.as_deref() == Some("read_file")
    }));
    let gemini_result = gemini
        .events
        .iter()
        .find(|event| event.kind == crate::domain::ConversationEventKind::ToolResult)
        .unwrap();
    assert!(gemini_result
        .text
        .as_deref()
        .is_some_and(|text| text.contains("dependency result")));
    assert!(gemini
        .events
        .iter()
        .any(|event| event.kind == crate::domain::ConversationEventKind::Plan));
    assert_eq!(
        gemini
            .events
            .iter()
            .map(|event| event.event_id.as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        gemini.events.len()
    );
    let event_content = crate::conversation::load_event_content(
        &conn,
        home,
        "gemini",
        "gemini-session-1",
        &gemini_result.event_id,
    )
    .unwrap();
    assert!(event_content
        .text
        .as_deref()
        .is_some_and(|text| text.contains("dependency result")));
    let raw_export = crate::conversation::build_export(
        &conn,
        home,
        "gemini",
        "gemini-session-1",
        crate::domain::ConversationExportFormat::Json,
    )
    .unwrap();
    assert!(raw_export.default_name.ends_with(".json"));
    let gemini_no_usage =
        crate::conversation::load_parsed_detail(&conn, home, "gemini", "gemini-no-usage").unwrap();
    assert!(usage_rows(&conn, "gemini", "gemini-no-usage").is_empty());
    assert!(gemini_no_usage.session.model.is_empty());
    assert_eq!(message_texts(&gemini_no_usage).len(), 2);
}

#[test]
fn conversation_index_issues_do_not_change_usage_ingest_failure_counts() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let path = home.join(".codex/sessions/missing-id.jsonl");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "{}\n").unwrap();
    let conn = store::open_memory().unwrap();

    let report = ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();

    assert_eq!(report.files_failed, 0);
    assert!(report.issues.is_empty());
    assert!(report.partial_success);
    assert_eq!(report.conversation_issues.len(), 1);
    assert_eq!(report.conversation_issues[0].source, "codex");
    assert_eq!(
        std::path::PathBuf::from(&report.conversation_issues[0].path),
        path
    );
    assert!(report.conversation_issues[0].message.contains("会话 ID"));
    let issue = serde_json::to_value(&report.conversation_issues[0]).unwrap();
    assert_eq!(issue["event_type"], "session_meta");
    assert!(issue["line"].is_null());
    assert!(!issue.to_string().contains("{}"));
}
