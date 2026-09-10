//! 按需读取注入快照正文。不写 sqlite、不进详情 DTO、不进备份。

use std::path::Path;

use rusqlite::Connection;

use crate::domain::{ConversationContextItemContentDto, ConversationSessionRow, Source};

use super::context_manifest;
use super::cursor_inject;
use super::grok_inject;
use super::session_store::load_session;

pub fn load_context_item_content(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
    item_id: &str,
) -> Result<ConversationContextItemContentDto, String> {
    let source = Source::parse(source).ok_or_else(|| "该来源没有注入快照".to_string())?;
    if !context_manifest::supported_source(source) {
        return Err("该来源没有注入快照".to_string());
    }
    let item_id = item_id.trim();
    if item_id.is_empty() {
        return Err("条目无效".to_string());
    }
    let session = load_session(conn, source.as_str(), session_id)?
        .ok_or_else(|| "未找到该对话记录".to_string())?;
    let content = snapshot_content(home, source, &session, item_id)
        .ok_or_else(|| "注入快照已不在，或该条目没有可查看的正文".to_string())?;
    Ok(ConversationContextItemContentDto {
        item_id: item_id.to_string(),
        content,
    })
}

fn snapshot_content(
    home: &Path,
    source: Source,
    session: &ConversationSessionRow,
    item_id: &str,
) -> Option<String> {
    match source {
        Source::Grok => grok_inject::content_for_item(session, item_id),
        Source::CursorAgent => cursor_inject::content_for_item(home, session, item_id),
        _ => None,
    }
}
