//! Grok 会话首轮注入快照。
//!
//! 读会话目录里的 `prompt_context.json`（`agents_md_files[]`）和 `events.jsonl`
//! 的 MCP 四类记录（配置解析 / 连接成功 / 连接失败 / 初始化完成）。
//! 与对话记录适配器隔离：不解析 `updates.jsonl`、不写 `conversation_events`、
//! 不把注入正文或 MCP 错误全文送进任何缓存。体积只保留字符数。

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::domain::{
    ConversationContextInjectionStatus, ConversationContextItem, ConversationContextKind,
    ConversationContextLayer, ConversationContextLoadMode, ConversationSessionRow,
};

const PROMPT_CONTEXT: &str = "prompt_context.json";
const EVENTS: &str = "events.jsonl";

#[derive(Default)]
pub(crate) struct GrokSnapshot {
    pub items: Vec<ConversationContextItem>,
    pub mcp_init_summary: Option<String>,
    pub called_mcp: BTreeSet<String>,
}

#[derive(Deserialize)]
struct PromptContext {
    #[serde(default)]
    agents_md_files: Vec<AgentsMdFile>,
}

#[derive(Deserialize)]
struct AgentsMdFile {
    #[serde(default)]
    file_name: String,
    #[serde(default)]
    file_path: String,
    #[serde(default)]
    content: String,
}

struct McpServerAcc {
    name: String,
    status: Option<ConversationContextInjectionStatus>,
    tools: Vec<String>,
    error_type: Option<String>,
}

pub(crate) fn from_session(session: &ConversationSessionRow) -> GrokSnapshot {
    let Some(dir) = session_dir(&session.source_file) else {
        return GrokSnapshot::default();
    };
    let mut items = from_prompt_context(&dir.join(PROMPT_CONTEXT));
    let mcp = from_events(&dir.join(EVENTS));
    items.extend(mcp.items);
    GrokSnapshot {
        items,
        mcp_init_summary: mcp.mcp_init_summary,
        called_mcp: mcp.called_mcp,
    }
}

fn session_dir(source_file: &str) -> Option<PathBuf> {
    if source_file.is_empty() {
        return None;
    }
    Some(Path::new(source_file).parent()?.to_path_buf())
}

fn from_prompt_context(path: &Path) -> Vec<ConversationContextItem> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(parsed) = serde_json::from_str::<PromptContext>(&text) else {
        return Vec::new();
    };
    let mut items = Vec::new();
    let mut seen = BTreeSet::new();
    for file in parsed.agents_md_files {
        let file_path = file.file_path.trim();
        if file_path.is_empty() || !seen.insert(file_path.to_string()) {
            continue;
        }
        let label = file_label(&file.file_name, file_path);
        items.push(ConversationContextItem {
            layer: ConversationContextLayer::Injected,
            kind: ConversationContextKind::Instruction,
            id: file_path.to_string(),
            label,
            path: Some(file_path.to_string()),
            load_mode: Some(ConversationContextLoadMode::Always),
            injection_status: None,
            char_count: Some(file.content.chars().count() as u64),
            is_noise: false,
            meta: None,
        });
    }
    items
}

fn from_events(path: &Path) -> GrokSnapshot {
    let Ok(file) = File::open(path) else {
        return GrokSnapshot::default();
    };
    let mut order: Vec<String> = Vec::new();
    let mut servers: BTreeMap<String, McpServerAcc> = BTreeMap::new();
    let mut called = BTreeSet::new();
    let mut summary: Option<String> = None;
    let mut saw_mcp = false;

    for line in BufReader::new(file).lines() {
        let Ok(line) = line else {
            continue;
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        let Some(kind) = value.get("type").and_then(Value::as_str) else {
            continue;
        };
        match kind {
            "mcp_config_resolved" => {
                saw_mcp = true;
                for name in json_names(value.get("servers")) {
                    ensure_server(&mut order, &mut servers, &name);
                }
                for name in json_names(value.get("disabled")) {
                    let server = ensure_server(&mut order, &mut servers, &name);
                    if server.status.is_none() {
                        server.status = Some(ConversationContextInjectionStatus::Disabled);
                    }
                }
            }
            "mcp_server_connected" => {
                saw_mcp = true;
                let Some(name) = json_nonempty(&value, "server_name") else {
                    continue;
                };
                let server = ensure_server(&mut order, &mut servers, &name);
                server.status = Some(ConversationContextInjectionStatus::Connected);
                server.tools = json_names(value.get("tools"));
                server.error_type = None;
            }
            "mcp_server_failed" => {
                saw_mcp = true;
                let Some(name) = json_nonempty(&value, "server_name") else {
                    continue;
                };
                let error_type = json_nonempty(&value, "error_type");
                let server = ensure_server(&mut order, &mut servers, &name);
                server.status = Some(if error_type.as_deref() == Some("auth_required") {
                    ConversationContextInjectionStatus::AuthRequired
                } else {
                    ConversationContextInjectionStatus::Failed
                });
                server.tools.clear();
                server.error_type = error_type;
            }
            "mcp_init_completed" => {
                saw_mcp = true;
                summary = Some(mcp_summary_line(
                    json_u64(&value, "total_servers"),
                    json_u64(&value, "succeeded"),
                    json_u64(&value, "failed"),
                    json_u64(&value, "total_tools"),
                ));
            }
            "mcp_tool_call_started" | "mcp_tool_call_completed" => {
                let server = json_nonempty(&value, "server_name");
                let tool = json_nonempty(&value, "tool_name");
                if let Some(server) = server.as_ref() {
                    called.insert(server.clone());
                }
                if let Some(tool) = tool.as_ref() {
                    called.insert(tool.clone());
                }
                if let (Some(server), Some(tool)) = (server, tool) {
                    called.insert(format!("{server}__{tool}"));
                }
            }
            _ => {}
        }
    }

    if !saw_mcp {
        return GrokSnapshot::default();
    }

    let items: Vec<ConversationContextItem> = order
        .iter()
        .filter_map(|name| servers.remove(name).map(mcp_item))
        .collect();
    let mcp_init_summary = summary.or_else(|| computed_mcp_summary(&items));
    GrokSnapshot {
        items,
        mcp_init_summary,
        called_mcp: called,
    }
}

fn ensure_server<'a>(
    order: &mut Vec<String>,
    servers: &'a mut BTreeMap<String, McpServerAcc>,
    name: &str,
) -> &'a mut McpServerAcc {
    if !servers.contains_key(name) {
        order.push(name.to_string());
        servers.insert(
            name.to_string(),
            McpServerAcc {
                name: name.to_string(),
                status: None,
                tools: Vec::new(),
                error_type: None,
            },
        );
    }
    servers.get_mut(name).expect("server just inserted")
}

fn mcp_item(server: McpServerAcc) -> ConversationContextItem {
    let connected = server.status == Some(ConversationContextInjectionStatus::Connected);
    let char_count = if connected {
        Some(server.tools.join(",").chars().count() as u64)
    } else {
        None
    };
    let mut meta = Map::new();
    if connected {
        meta.insert("tool_count".into(), json!(server.tools.len() as u64));
        if !server.tools.is_empty() {
            meta.insert("tools".into(), json!(server.tools));
        }
    }
    if let Some(error_type) = server.error_type {
        meta.insert("error_type".into(), json!(error_type));
    }
    ConversationContextItem {
        layer: ConversationContextLayer::Injected,
        kind: ConversationContextKind::McpServer,
        id: server.name.clone(),
        label: server.name,
        path: None,
        load_mode: if connected {
            Some(ConversationContextLoadMode::Always)
        } else {
            None
        },
        injection_status: server.status,
        char_count,
        is_noise: false,
        meta: if meta.is_empty() {
            None
        } else {
            Some(Value::Object(meta))
        },
    }
}

fn computed_mcp_summary(items: &[ConversationContextItem]) -> Option<String> {
    if items.is_empty() {
        return None;
    }
    let configured = items.len() as u64;
    let connected = items
        .iter()
        .filter(|item| item.injection_status == Some(ConversationContextInjectionStatus::Connected))
        .count() as u64;
    let failed = items
        .iter()
        .filter(|item| {
            matches!(
                item.injection_status,
                Some(
                    ConversationContextInjectionStatus::Failed
                        | ConversationContextInjectionStatus::AuthRequired
                )
            )
        })
        .count() as u64;
    let total_tools: u64 = items
        .iter()
        .filter_map(|item| {
            item.meta
                .as_ref()
                .and_then(|meta| meta.get("tool_count"))
                .and_then(Value::as_u64)
        })
        .sum();
    Some(mcp_summary_line(configured, connected, failed, total_tools))
}

fn mcp_summary_line(configured: u64, connected: u64, failed: u64, total_tools: u64) -> String {
    format!("配置 {configured} 台 / 连上 {connected} 台 / 失败 {failed} 台 / 共注入 {total_tools} 个工具")
}

fn json_names(value: Option<&Value>) -> Vec<String> {
    let Some(Value::Array(entries)) = value else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            entry
                .as_str()
                .map(str::to_string)
                .or_else(|| json_nonempty(entry, "name"))
        })
        .filter(|name| !name.is_empty())
        .collect()
}

fn json_nonempty(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn json_u64(value: &Value, key: &str) -> u64 {
    value
        .get(key)
        .and_then(Value::as_u64)
        .or_else(|| {
            value
                .get(key)
                .and_then(Value::as_i64)
                .map(|n| n.max(0) as u64)
        })
        .unwrap_or(0)
}

fn file_label(file_name: &str, file_path: &str) -> String {
    let trimmed = file_name.trim();
    if !trimmed.is_empty() {
        return trimmed.to_string();
    }
    Path::new(file_path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(file_path)
        .to_string()
}
