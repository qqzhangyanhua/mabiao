use super::*;

fn project_from_path(path: &Path) -> String {
    path.parent()
        .and_then(Path::parent)
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_string()
}

pub(super) fn parse(
    path: &Path,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    // 直接从文件流反序列化，不额外攥一份完整原始文本：单个 Gemini 会话文件会随对话
    // 增长，避免「原始文本 + 解析后的 Value 树」同时常驻可以省下一半峰值内存。
    let file = fs::File::open(path).map_err(|error| format!("读取原始文件失败：{error}"))?;
    let root: Value = serde_json::from_reader(BufReader::new(file))
        .map_err(|error| format!("JSON 无效：{error}"))?;
    let mut session_id = first_text(&root, &["sessionId", "session_id", "id"]);
    if session_id.is_empty() {
        session_id = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("")
            .trim_start_matches("session-")
            .to_string();
    }
    let project = {
        let explicit = first_text(&root, &["cwd", "projectPath", "projectHash"]);
        if explicit.is_empty() {
            project_from_path(path)
        } else {
            explicit
        }
    };
    let mut model = String::new();
    let mut started_at = String::new();
    let mut ended_at = String::new();
    let mut messages = Vec::new();
    let mut events = Vec::new();
    let values = root
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| "缺少 Gemini messages 数组".to_string())?;

    for (index, value) in values.iter().enumerate() {
        let timestamp = text_field(value, "timestamp");
        update_time_bounds(&timestamp, &mut started_at, &mut ended_at);
        let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
        let next_model = first_text(value, &["model"]);
        if !next_model.is_empty() && next_model != model {
            model = next_model.clone();
            events.push(semantic_event(
                index,
                EventKind::ModelChange,
                &timestamp,
                None,
                Some(next_model),
                None,
                value.clone(),
            ));
        }
        if matches!(kind, "user" | "gemini" | "assistant") {
            let role = if kind == "user" { "user" } else { "assistant" };
            push_projected_message(
                index,
                &timestamp,
                role,
                value.get("content").unwrap_or(&Value::Null),
                value.clone(),
                &mut messages,
                &mut events,
            );
        }
        if matches!(kind, "error" | "info" | "warning") {
            events.push(semantic_event(
                index,
                if kind == "error" {
                    EventKind::Error
                } else {
                    EventKind::SystemStatus
                },
                &timestamp,
                None,
                optional_text(value, &["status", "type"]),
                optional_text(value, &["content", "message", "error"]),
                value.clone(),
            ));
        }
        if let Some(thoughts) = value.get("thoughts").and_then(Value::as_array) {
            for thought in thoughts {
                let text = first_text(thought, &["description", "subject", "text"]);
                events.push(semantic_event(
                    index,
                    EventKind::Plan,
                    &timestamp,
                    Some(EventActor::Assistant),
                    optional_text(thought, &["subject"]),
                    (!text.is_empty()).then_some(text),
                    thought.clone(),
                ));
            }
        }
        if let Some(tool_calls) = value.get("toolCalls").and_then(Value::as_array) {
            for call in tool_calls {
                events.push(semantic_event(
                    index,
                    EventKind::ToolCall,
                    &timestamp,
                    Some(EventActor::Assistant),
                    optional_text(call, &["name", "toolName"]),
                    call.get("args")
                        .or_else(|| call.get("arguments"))
                        .map(Value::to_string),
                    normalize_tool_call_details(call),
                ));
                if call.get("result").is_some_and(|result| !result.is_null()) {
                    let details = normalize_tool_result_details(call);
                    events.push(tool_result_event(
                        index,
                        &timestamp,
                        &details,
                        include_deferred_content,
                    ));
                }
            }
        }
        if matches!(kind, "tool" | "tool_result" | "toolResult") {
            events.push(tool_result_event(
                index,
                &timestamp,
                &normalize_tool_result_details(value),
                include_deferred_content,
            ));
        }
        if let Some(tool_results) = value
            .get("toolResponses")
            .or_else(|| value.get("toolResults"))
            .and_then(Value::as_array)
        {
            for result in tool_results {
                events.push(tool_result_event(
                    index,
                    &timestamp,
                    &normalize_tool_result_details(result),
                    include_deferred_content,
                ));
            }
        }
    }
    finish_source_conversation(
        Source::Gemini,
        path,
        session_id,
        String::new(),
        project,
        model,
        started_at,
        ended_at,
        messages,
        events,
        true,
    )
}
