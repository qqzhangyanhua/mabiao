//! Antigravity（`agy`）对话记录：一个 `conversations/<id>.db` 就是一场会话。
//!
//! 只读 `steps.step_payload`（`CortexStep`）里字段号已对上的部分，字段号来自本机真实库的
//! 结构探测，不是官方 schema：
//!
//! - `step_type = 14`（用户输入）：字段 19 → 字段 2 是用户原文。
//! - `step_type = 15`（规划器回复）：字段 20 → 字段 1 是回复正文，字段 3 是思考，
//!   字段 7 是重复的工具调用 `{1 id, 2 name, 3 参数 JSON}`。
//! - 时间：字段 5 → 字段 1（`Timestamp`，与 `steps.metadata` 同一份）。
//! - 模型：字段 5 → 字段 9 → 字段 11 是 request id，查 `gen_metadata`（与消耗记录同一条路）。
//!
//! 工具执行步骤（`132` 等）的结果字段随工具类型各异、尚未摸清，这里不解析，
//! 只在有工具调用时声明缺 `tool_result`，不把猜出来的结果当事实。其余步骤类型忽略。

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde_json::Value;

use super::toolbox::*;
use super::{ConversationIndexBatch, ConversationIndexIssue};
use crate::adapters::agy::{load_model_names, load_project, timestamp_at};
use crate::adapters::project::session_id_from_source_file;
use crate::ingest;
use crate::proto_wire;

const STEP_USER_INPUT: i64 = 14;
const STEP_PLANNER_RESPONSE: i64 = 15;

const USER_INPUT: u32 = 19;
const USER_INPUT_TEXT: u32 = 2;

const PLANNER: u32 = 20;
const PLANNER_RESPONSE: u32 = 1;
const PLANNER_THINKING: u32 = 3;
const PLANNER_TOOL_CALL: u64 = 7;
const TOOL_CALL_ID: u32 = 1;
const TOOL_CALL_NAME: u32 = 2;
const TOOL_CALL_ARGS: u32 = 3;

const STEP_METADATA: u32 = 5;
const METADATA_CREATED_AT: u32 = 1;
const METADATA_USAGE: u32 = 9;
const USAGE_REQUEST_ID: u32 = 11;

pub(super) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    crate::adapters::agy::discover(roots)
}

pub(super) fn index(path: &Path) -> Result<ConversationIndexBatch, ConversationIndexIssue> {
    project(path)
}

pub(super) fn detail(
    path: &Path,
    session_id: &str,
    _include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    let batch = project(path).map_err(|issue| issue.message)?;
    batch
        .conversations
        .into_iter()
        .find(|conversation| conversation.session.session_id == session_id)
        .ok_or_else(|| "Antigravity 数据库中未找到该会话".to_string())
}

pub(super) fn source_revision(path: &Path) -> Result<String, String> {
    super::discover::wal_source_revision(path, "Antigravity")
}

fn fatal(path: &Path, message: String) -> ConversationIndexIssue {
    ConversationIndexIssue {
        path: path.to_string_lossy().to_string(),
        message,
        event_type: Some("agy_schema".to_string()),
        line: None,
    }
}

fn project(path: &Path) -> Result<ConversationIndexBatch, ConversationIndexIssue> {
    let db = ingest::open_readonly_uri(path)
        .map_err(|error| fatal(path, format!("只读打开 Antigravity 数据库失败：{error}")))?;
    // 没有 `step_payload` 列的是旧版或非 agy 数据，与消耗记录一样静默排除。
    if !has_step_payload(&db) {
        return Ok(ConversationIndexBatch {
            conversations: Vec::new(),
            diagnostics: Vec::new(),
        });
    }
    let conversation = project_session(path, &db).map_err(|message| fatal(path, message))?;
    Ok(ConversationIndexBatch {
        conversations: vec![conversation],
        diagnostics: Vec::new(),
    })
}

fn has_step_payload(db: &Connection) -> bool {
    db.query_row(
        "SELECT 1 FROM pragma_table_info('steps') WHERE name = 'step_payload'",
        [],
        |_| Ok(()),
    )
    .is_ok()
}

fn project_session(path: &Path, db: &Connection) -> Result<ParsedConversation, String> {
    let models = load_model_names(db);
    let project = load_project(db);
    let session_id = session_id_from_source_file(&path.to_string_lossy());
    let mut statement = db
        .prepare(
            "SELECT idx, step_type, step_payload FROM steps
             WHERE step_payload IS NOT NULL ORDER BY idx",
        )
        .map_err(|error| error.to_string())?;
    let steps = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Vec<u8>>(2)?,
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;

    let mut messages = Vec::new();
    let mut events = Vec::new();
    let mut started_at = String::new();
    let mut ended_at = String::new();
    let mut model = String::new();
    let mut has_tool_call = false;
    let mut last_sequence = 0usize;
    for (idx, step_type, payload) in steps {
        let sequence = usize::try_from(idx).unwrap_or_default();
        last_sequence = last_sequence.max(sequence);
        let occurred_at = timestamp_at(&payload, &[STEP_METADATA, METADATA_CREATED_AT]);
        match step_type {
            STEP_USER_INPUT => {
                let Some(text) = proto_wire::text_at_path(&payload, &[USER_INPUT, USER_INPUT_TEXT])
                else {
                    continue;
                };
                update_time_bounds(&occurred_at, &mut started_at, &mut ended_at);
                push_projected_message(
                    sequence,
                    &occurred_at,
                    "user",
                    &Value::String(text.to_string()),
                    serde_json::json!({ "step_type": step_type }),
                    &mut messages,
                    &mut events,
                );
            }
            STEP_PLANNER_RESPONSE => {
                update_time_bounds(&occurred_at, &mut started_at, &mut ended_at);
                if let Some(request_id) = proto_wire::text_at_path(
                    &payload,
                    &[STEP_METADATA, METADATA_USAGE, USAGE_REQUEST_ID],
                ) {
                    if let Some(name) = models.get(request_id) {
                        model = name.clone();
                    }
                }
                if let Some(thinking) =
                    proto_wire::text_at_path(&payload, &[PLANNER, PLANNER_THINKING])
                {
                    events.push(semantic_event(
                        sequence,
                        EventKind::Plan,
                        &occurred_at,
                        Some(EventActor::Assistant),
                        None,
                        Some(thinking.to_string()),
                        serde_json::json!({ "type": "thinking", "text": thinking }),
                    ));
                }
                if let Some(response) =
                    proto_wire::text_at_path(&payload, &[PLANNER, PLANNER_RESPONSE])
                {
                    push_projected_message(
                        sequence,
                        &occurred_at,
                        "assistant",
                        &Value::String(response.to_string()),
                        serde_json::json!({ "step_type": step_type }),
                        &mut messages,
                        &mut events,
                    );
                }
                if let Some(planner) = proto_wire::bytes_at_path(&payload, &[PLANNER]) {
                    proto_wire::for_each_bytes_field(planner, |field, call| {
                        if field != PLANNER_TOOL_CALL {
                            return;
                        }
                        has_tool_call = true;
                        events.push(tool_call_event(sequence, &occurred_at, call));
                    });
                }
            }
            _ => {}
        }
    }

    let missing = ["tool_result"];
    let degradation = if has_tool_call {
        CapabilityDegradationPrep::Declared {
            sequence: last_sequence + 1,
            missing: &missing,
        }
    } else {
        CapabilityDegradationPrep::None
    };
    finish_source_conversation(
        Source::Agy,
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
        ConversationFinishPrep {
            native_ids: NativeEventIdPrep::Skip,
            degradation,
        },
    )
}

fn tool_call_event(sequence: usize, occurred_at: &str, call: &[u8]) -> ConversationEvent {
    let id = proto_wire::text_at_path(call, &[TOOL_CALL_ID]).unwrap_or_default();
    let name = proto_wire::text_at_path(call, &[TOOL_CALL_NAME]).unwrap_or_default();
    let arguments = proto_wire::text_at_path(call, &[TOOL_CALL_ARGS]).unwrap_or_default();
    let input = serde_json::from_str::<Value>(arguments)
        .unwrap_or_else(|_| Value::String(arguments.to_string()));
    semantic_event(
        sequence,
        EventKind::ToolCall,
        occurred_at,
        Some(EventActor::Assistant),
        (!name.is_empty()).then(|| name.to_string()),
        (!arguments.is_empty()).then(|| arguments.to_string()),
        serde_json::json!({ "call_id": id, "name": name, "input": input }),
    )
}
