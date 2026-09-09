//! Cursor 会话首轮注入快照。
//!
//! 读 `~/.cursor/chats/<hash>/<session>/store.db`：blobs 按内容 sha256 寻址，
//! root blob 的 `repeated bytes` 字段 1 还原有序消息。取下标 1 的 user 消息
//! 分段计量。与对话记录适配器隔离：不写 `conversation_events`、不把注入正文
//! 送进任何缓存。体积只保留字符数。MCP 只计工具名列表，不含 schema。

use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Map, Value};

use crate::domain::{
    ConversationContextInjectionStatus, ConversationContextItem, ConversationContextKind,
    ConversationContextLayer, ConversationContextLoadMode, ConversationSessionRow,
};
use crate::proto_wire;

const UNRECOGNIZED_ID: &str = "unrecognized";
const COMPLETENESS_KEYS: [&str; 9] = [
    "agentSkills",
    "customSubagents",
    "env",
    "gitRepos",
    "gitStatus",
    "mcp",
    "mcpFileSystem",
    "repositoryInfo",
    "rules",
];

#[derive(Default)]
pub(crate) struct CursorSnapshot {
    pub items: Vec<ConversationContextItem>,
    pub found: bool,
    pub incomplete_keys: Option<Vec<String>>,
}

pub(crate) fn from_session(home: &Path, session: &ConversationSessionRow) -> CursorSnapshot {
    let Some(path) = find_store_db(home, &session.session_id) else {
        return CursorSnapshot::default();
    };
    let messages = restore_messages(&path);
    let Some(user) = messages.get(1) else {
        return CursorSnapshot::default();
    };
    if user.get("role").and_then(Value::as_str) != Some("user") {
        return CursorSnapshot::default();
    }
    CursorSnapshot {
        items: json_message_text(user)
            .map(|text| items_from_text(&text))
            .unwrap_or_default(),
        found: true,
        incomplete_keys: incomplete_keys(user),
    }
}

fn find_store_db(home: &Path, session_id: &str) -> Option<PathBuf> {
    if session_id.is_empty()
        || session_id.contains('/')
        || session_id.contains('\\')
        || session_id.contains("..")
    {
        return None;
    }
    let chats = home.join(".cursor/chats");
    let entries = fs::read_dir(&chats).ok()?;
    let mut found = None;
    for entry in entries.flatten() {
        let path = entry.path().join(session_id).join("store.db");
        if path.is_file() {
            if found.is_some() {
                break;
            }
            found = Some(path);
        }
    }
    found
}

fn incomplete_keys(message: &Value) -> Option<Vec<String>> {
    let obj = message
        .get("providerOptions")?
        .get("cursor")?
        .get("requestContextCompleteness")?
        .as_object()?;
    Some(
        COMPLETENESS_KEYS
            .iter()
            .copied()
            .filter(|key| obj.get(*key) == Some(&Value::Bool(false)))
            .map(str::to_string)
            .collect(),
    )
}

fn restore_messages(path: &Path) -> Vec<Value> {
    let Ok(conn) = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY) else {
        return Vec::new();
    };
    let Ok(meta_value) = conn.query_row("SELECT value FROM meta WHERE key = '0'", [], |row| {
        row.get::<_, String>(0)
    }) else {
        return Vec::new();
    };
    let Some(root_id) = latest_root_blob_id(&meta_value) else {
        return Vec::new();
    };
    let Some(root) = blob_data(&conn, &root_id) else {
        return Vec::new();
    };
    let mut messages = Vec::new();
    proto_wire::for_each_bytes_field(&root, |field, bytes| {
        if field != 1 {
            return;
        }
        let Some(id) = blob_id_hex(bytes) else {
            messages.push(Value::Null);
            return;
        };
        match blob_data(&conn, &id).and_then(|data| serde_json::from_slice(&data).ok()) {
            Some(value) => messages.push(value),
            None => messages.push(Value::Null),
        }
    });
    messages
}

fn latest_root_blob_id(meta_value: &str) -> Option<String> {
    let parsed = parse_meta_json(meta_value)?;
    parsed
        .get("latestRootBlobId")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
}

fn parse_meta_json(value: &str) -> Option<Value> {
    if let Some(bytes) = decode_hex(value) {
        if let Ok(parsed) = serde_json::from_slice(&bytes) {
            return Some(parsed);
        }
    }
    serde_json::from_str(value).ok()
}

fn blob_data(conn: &Connection, id: &str) -> Option<Vec<u8>> {
    conn.query_row("SELECT data FROM blobs WHERE id = ?1", [id], |row| {
        row.get::<_, Vec<u8>>(0)
    })
    .ok()
}

fn json_message_text(value: &Value) -> Option<String> {
    let content = value.get("content")?;
    if let Some(text) = content.as_str() {
        return Some(text.to_string());
    }
    let parts = content.as_array()?;
    let mut texts = Vec::new();
    for part in parts {
        if part.get("type").and_then(Value::as_str) != Some("text") {
            continue;
        }
        if let Some(text) = part.get("text").and_then(Value::as_str) {
            texts.push(text);
        }
    }
    if texts.is_empty() {
        None
    } else {
        Some(texts.join("\n"))
    }
}

struct ParsedItem {
    start: usize,
    end: usize,
    item: ConversationContextItem,
}

fn items_from_text(text: &str) -> Vec<ConversationContextItem> {
    let total = text.chars().count() as u64;
    let mut parsed = Vec::new();
    push_tag_items(&mut parsed, text, "agent_skill", parse_skill);
    push_tag_items(
        &mut parsed,
        text,
        "always_applied_workspace_rule",
        parse_workspace_rule,
    );
    push_tag_items(&mut parsed, text, "user_rules", parse_user_rules);
    push_tag_items(&mut parsed, text, "namespace", parse_namespace);
    push_tag_items(&mut parsed, text, "user_info", parse_user_info);
    push_tag_items(
        &mut parsed,
        text,
        "available_subagent_types",
        parse_subagent_types,
    );
    parsed.sort_by_key(|item| item.start);
    let mut accepted = Vec::new();
    for item in parsed {
        if accepted.iter().any(|other: &ParsedItem| {
            ranges_overlap((other.start, other.end), (item.start, item.end))
        }) {
            continue;
        }
        accepted.push(item);
    }
    let accounted: u64 = accepted
        .iter()
        .filter_map(|item| item.item.char_count)
        .sum();
    let mut items: Vec<ConversationContextItem> =
        accepted.into_iter().map(|item| item.item).collect();
    if total > accounted {
        let rest = total - accounted;
        items.push(unrecognized_item(rest));
    }
    items
}

fn push_tag_items(
    out: &mut Vec<ParsedItem>,
    text: &str,
    tag: &str,
    parse: fn(&str, Element<'_>) -> Option<ParsedItem>,
) {
    for element in find_elements(text, tag) {
        if let Some(item) = parse(text, element) {
            out.push(item);
        }
    }
}

fn parse_skill(text: &str, element: Element<'_>) -> Option<ParsedItem> {
    let path = attr(element.open, "fullPath")?;
    if path.is_empty() {
        return None;
    }
    Some(span_item(
        element,
        ConversationContextKind::Skill,
        path,
        path_label(path),
        Some(path.to_string()),
        None,
        element.char_count(text),
    ))
}

fn parse_workspace_rule(text: &str, element: Element<'_>) -> Option<ParsedItem> {
    let path = attr(element.open, "name")?;
    if path.is_empty() {
        return None;
    }
    Some(span_item(
        element,
        ConversationContextKind::Rule,
        path,
        path_label(path),
        Some(path.to_string()),
        None,
        element.char_count(text),
    ))
}

fn parse_user_rules(text: &str, element: Element<'_>) -> Option<ParsedItem> {
    Some(span_item(
        element,
        ConversationContextKind::Rule,
        "user_rules",
        "用户规则",
        None,
        None,
        element.char_count(text),
    ))
}

fn parse_namespace(_text: &str, element: Element<'_>) -> Option<ParsedItem> {
    let name = attr(element.open, "name")?;
    if name.is_empty() {
        return None;
    }
    let tools_raw = attr(element.open, "tools").unwrap_or("");
    let tools: Vec<String> = tools_raw
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect();
    let (start, end) = tools_value_range(element.open_range, element.open, tools_raw)
        .unwrap_or((element.start, element.start));
    let mut meta = Map::new();
    meta.insert("tool_count".into(), json!(tools.len() as u64));
    if !tools.is_empty() {
        meta.insert("tools".into(), json!(tools));
    }
    Some(ParsedItem {
        start,
        end,
        item: ConversationContextItem {
            layer: ConversationContextLayer::Injected,
            kind: ConversationContextKind::McpServer,
            id: name.to_string(),
            label: name.to_string(),
            path: None,
            load_mode: Some(ConversationContextLoadMode::Always),
            injection_status: Some(ConversationContextInjectionStatus::Connected),
            char_count: Some(tools_raw.chars().count() as u64),
            is_noise: false,
            is_unused_install: false,
            meta: Some(Value::Object(meta)),
        },
    })
}

fn parse_user_info(text: &str, element: Element<'_>) -> Option<ParsedItem> {
    Some(span_item(
        element,
        ConversationContextKind::Instruction,
        "user_info",
        "环境信息",
        None,
        None,
        element.char_count(text),
    ))
}

fn parse_subagent_types(text: &str, element: Element<'_>) -> Option<ParsedItem> {
    Some(span_item(
        element,
        ConversationContextKind::Instruction,
        "available_subagent_types",
        "子代理类型",
        None,
        None,
        element.char_count(text),
    ))
}

#[allow(clippy::too_many_arguments)]
fn span_item(
    element: Element<'_>,
    kind: ConversationContextKind,
    id: &str,
    label: impl Into<String>,
    path: Option<String>,
    meta: Option<Value>,
    char_count: u64,
) -> ParsedItem {
    ParsedItem {
        start: element.start,
        end: element.end,
        item: ConversationContextItem {
            layer: ConversationContextLayer::Injected,
            kind,
            id: id.to_string(),
            label: label.into(),
            path,
            load_mode: Some(ConversationContextLoadMode::Always),
            injection_status: None,
            char_count: Some(char_count),
            is_noise: false,
            is_unused_install: false,
            meta,
        },
    }
}

fn unrecognized_item(char_count: u64) -> ConversationContextItem {
    ConversationContextItem {
        layer: ConversationContextLayer::Injected,
        kind: ConversationContextKind::Instruction,
        id: UNRECOGNIZED_ID.to_string(),
        label: format!("未识别 {char_count} 字符"),
        path: None,
        load_mode: None,
        injection_status: None,
        char_count: Some(char_count),
        is_noise: false,
        is_unused_install: false,
        meta: None,
    }
}

#[derive(Clone, Copy)]
struct Element<'a> {
    start: usize,
    end: usize,
    open_range: (usize, usize),
    open: &'a str,
}

impl Element<'_> {
    fn char_count(self, text: &str) -> u64 {
        text.get(self.start..self.end)
            .map(|slice| slice.chars().count() as u64)
            .unwrap_or(0)
    }
}

fn find_elements<'a>(text: &'a str, tag: &str) -> Vec<Element<'a>> {
    let open_pat = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut out = Vec::new();
    let mut search = 0usize;
    while search < text.len() {
        let Some(rel) = text[search..].find(&open_pat) else {
            break;
        };
        let start = search + rel;
        let after_name = start + open_pat.len();
        if !tag_boundary(text.as_bytes().get(after_name).copied()) {
            search = after_name;
            continue;
        }
        let Some(gt_rel) = text[after_name..].find('>') else {
            break;
        };
        let open_end = after_name + gt_rel + 1;
        let Some(open) = text.get(start..open_end) else {
            break;
        };
        if open.as_bytes().get(open.len().saturating_sub(2)) == Some(&b'/') {
            out.push(Element {
                start,
                end: open_end,
                open_range: (start, open_end),
                open,
            });
            search = open_end;
            continue;
        }
        match find_close(text, open_end, &open_pat, &close) {
            Some(end) => {
                out.push(Element {
                    start,
                    end,
                    open_range: (start, open_end),
                    open,
                });
                search = end;
            }
            None => {
                search = open_end;
            }
        }
    }
    out
}

fn find_close(text: &str, mut i: usize, open: &str, close: &str) -> Option<usize> {
    let mut depth = 1u32;
    while depth > 0 && i < text.len() {
        let next_open = text[i..].find(open);
        let next_close = text[i..].find(close);
        match (next_open, next_close) {
            (Some(o), Some(c)) if o < c => {
                let at = i + o;
                let after = at + open.len();
                if tag_boundary(text.as_bytes().get(after).copied()) {
                    depth = depth.saturating_add(1);
                }
                i = after;
            }
            (_, Some(c)) => {
                depth -= 1;
                let at = i + c;
                if depth == 0 {
                    return Some(at + close.len());
                }
                i = at + close.len();
            }
            _ => return None,
        }
    }
    None
}

fn tag_boundary(byte: Option<u8>) -> bool {
    matches!(
        byte,
        Some(b' ' | b'\t' | b'\n' | b'\r' | b'>' | b'/') | None
    )
}

fn attr<'a>(open: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("{name}=");
    let idx = open.find(&key)?;
    let rest = open.get(idx + key.len()..)?;
    let quote = rest.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let inner = rest.get(quote.len_utf8()..)?;
    let end = inner.find(quote)?;
    inner.get(..end)
}

fn tools_value_range(
    open_range: (usize, usize),
    open: &str,
    tools_raw: &str,
) -> Option<(usize, usize)> {
    if tools_raw.is_empty() {
        return None;
    }
    let key = "tools=";
    let rel = open.find(key)?;
    let after = rel + key.len();
    let quote = open.get(after..)?.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let value_start = open_range.0 + after + quote.len_utf8();
    Some((value_start, value_start + tools_raw.len()))
}

fn path_label(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let as_path = Path::new(trimmed);
    if as_path.file_name().and_then(|name| name.to_str()) == Some("SKILL.md") {
        if let Some(name) = as_path
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str())
        {
            return name.to_string();
        }
    }
    as_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(trimmed)
        .to_string()
}

fn ranges_overlap(a: (usize, usize), b: (usize, usize)) -> bool {
    a.0 < b.1 && b.0 < a.1
}

fn blob_id_hex(bytes: &[u8]) -> Option<String> {
    if bytes.len() != 32 {
        return None;
    }
    Some(to_hex(bytes))
}

fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
}

fn decode_hex(text: &str) -> Option<Vec<u8>> {
    let trimmed = text.trim();
    if !trimmed.len().is_multiple_of(2) || trimmed.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(trimmed.len() / 2);
    let bytes = trimmed.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let hi = from_hex_digit(bytes[i])?;
        let lo = from_hex_digit(bytes[i + 1])?;
        out.push((hi << 4) | lo);
        i += 2;
    }
    Some(out)
}

fn from_hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
