//! 对话附件的统一发现、内嵌 base64 体积估算、路径解析、安全校验与字节/缩略图读取。
//!
//! 源码行流式读取 `read_source_line` 同步保留在此供按行直取与测试复用。

use std::fs;
use std::io::{BufRead, BufReader, Cursor};
use std::path::{Path, PathBuf};

use base64::prelude::*;
use rusqlite::Connection;
use serde_json::Value;

use crate::domain::{
    ConversationAttachment, ConversationAttachmentContentDto,
    ConversationAttachmentKind as AttachmentKind, ConversationAttachmentStatus as AttachmentStatus,
    ConversationEvent, Source,
};

use super::event_store::read_indexed_event_if_ready;
use super::normalize::{first_text, optional_text};
use super::session_store::{ensure_matching_session, load_trusted_session_files};
use super::{parse_conversation_file, parse_conversation_files};

const THUMBNAIL_MAX_WIDTH: u32 = 320;
const THUMBNAIL_MAX_HEIGHT: u32 = 240;

struct AttachmentCandidate {
    attachment: ConversationAttachment,
    source: String,
    resolved_path: Option<PathBuf>,
}

pub fn load_attachment(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
    attachment_id: &str,
) -> Result<ConversationAttachmentContentDto, String> {
    let candidate = resolve_attachment(conn, home, source, session_id, attachment_id)?;
    let data_url = attachment_data_url(&candidate)?;
    Ok(ConversationAttachmentContentDto {
        attachment: candidate.attachment,
        data_url,
    })
}

pub fn load_attachment_thumbnail(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
    attachment_id: &str,
) -> Result<ConversationAttachmentContentDto, String> {
    let candidate = resolve_attachment(conn, home, source, session_id, attachment_id)?;
    let data_url = attachment_thumbnail_data_url(&candidate)?;
    Ok(ConversationAttachmentContentDto {
        attachment: candidate.attachment,
        data_url,
    })
}

pub(crate) fn populate_attachments(events: &mut [ConversationEvent], project: &str) {
    for event in events {
        event.attachments = attachment_candidates(event.sequence, &event.details, project)
            .into_iter()
            .map(|candidate| candidate.attachment)
            .collect();
    }
}

pub(crate) fn payload_has_local_media(value: &Value) -> bool {
    match value {
        Value::Array(items) => items.iter().any(payload_has_local_media),
        Value::Object(_) => {
            if attachment_candidate(value, "").is_some() {
                return true;
            }
            ["content", "attachments", "parts"]
                .iter()
                .any(|key| value.get(*key).is_some_and(payload_has_local_media))
        }
        _ => false,
    }
}

/// 按行号流式读取源文件的一行。行号从 0 计；只保留当前行，不把整份文件读进内存。
pub(crate) fn read_source_line(path: &Path, line_index: u32) -> Result<String, String> {
    let file = fs::File::open(path).map_err(|error| format!("读取原始文件失败：{error}"))?;
    let mut reader = BufReader::new(file);
    let mut buffer = Vec::new();
    let mut index = 0u32;
    loop {
        buffer.clear();
        let bytes = reader
            .read_until(b'\n', &mut buffer)
            .map_err(|error| format!("读取原始文件失败：{error}"))?;
        if bytes == 0 {
            return Err(format!("原始文件中未找到第 {} 行", line_index + 1));
        }
        if index == line_index {
            let line = std::str::from_utf8(&buffer)
                .map_err(|error| format!("读取原始文件失败：{error}"))?;
            let line = line.strip_suffix('\n').unwrap_or(line);
            let line = line.strip_suffix('\r').unwrap_or(line);
            return Ok(line.to_string());
        }
        index += 1;
    }
}

fn resolve_attachment(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
    attachment_id: &str,
) -> Result<AttachmentCandidate, String> {
    if let Some(candidate) = try_resolve_attachment(conn, home, source, session_id, attachment_id)?
    {
        return Ok(candidate);
    }
    let (source, session, paths) = load_trusted_session_files(conn, home, source, session_id)?;
    let parsed = parse_conversation_files(source, &paths, session_id, true)?;
    ensure_matching_session(&parsed, &session)?;
    let event = parsed
        .events
        .iter()
        .find(|event| {
            event
                .attachments
                .iter()
                .any(|attachment| attachment.id == attachment_id)
        })
        .ok_or_else(|| "原始文件中未找到该附件".to_string())?;
    let attachment_index = event
        .attachments
        .iter()
        .position(|attachment| attachment.id == attachment_id)
        .ok_or_else(|| "原始文件中未找到该附件".to_string())?;
    let attachment = event.attachments[attachment_index].clone();
    if attachment.kind != AttachmentKind::Image {
        return Err("该附件不是可预览的图片".to_string());
    }
    let source_path = PathBuf::from(&event.source_file);
    let source_fragment = parse_conversation_file(source, &source_path, session_id, true)?;
    let payload = read_source_payload(source, &source_path, event.source_sequence)?;
    let mut candidate = attachment_candidates(
        event.source_sequence,
        &payload,
        &source_fragment.session.project,
    )
    .into_iter()
    .nth(attachment_index)
    .ok_or_else(|| "原始文件中未找到该附件".to_string())?;
    candidate.attachment = attachment;
    ensure_attachment_path_allowed(&candidate, &source_fragment.session.project)?;
    Ok(candidate)
}

fn try_resolve_attachment(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
    attachment_id: &str,
) -> Result<Option<AttachmentCandidate>, String> {
    let Some((event_id, attachment_index)) = split_attachment_id(attachment_id) else {
        return Ok(None);
    };
    let Some((prepared, indexed)) =
        read_indexed_event_if_ready(conn, home, source, session_id, event_id)?
    else {
        return Ok(None);
    };
    let Some(attachment) = indexed.attachments.get(attachment_index).cloned() else {
        return Ok(None);
    };
    if attachment.kind != AttachmentKind::Image {
        return Err("该附件不是可预览的图片".to_string());
    }
    let source_path = PathBuf::from(&indexed.source_file);
    let Ok(payload) = read_source_payload(prepared.source, &source_path, indexed.source_sequence)
    else {
        return Ok(None);
    };
    let Some(mut candidate) =
        attachment_candidates(indexed.source_sequence, &payload, &prepared.session.project)
            .into_iter()
            .nth(attachment_index)
    else {
        return Ok(None);
    };
    candidate.attachment = attachment;
    ensure_attachment_path_allowed(&candidate, &prepared.session.project)?;
    Ok(Some(candidate))
}

fn split_attachment_id(attachment_id: &str) -> Option<(&str, usize)> {
    let (event_id, index) = attachment_id.rsplit_once(':')?;
    Some((event_id, index.parse().ok()?))
}

fn attachment_candidates(
    sequence: u32,
    payload: &Value,
    project: &str,
) -> Vec<AttachmentCandidate> {
    let mut values = Vec::new();
    for key in ["content", "attachments", "parts"] {
        match payload.get(key) {
            Some(Value::Array(items)) => values.extend(items),
            Some(value @ Value::Object(_)) => values.push(value),
            _ => {}
        }
    }
    values
        .into_iter()
        .filter_map(|value| attachment_candidate(value, project))
        .enumerate()
        .map(|(index, mut candidate)| {
            candidate.attachment.id = format!("{sequence}:{index}");
            candidate
        })
        .collect()
}

fn attachment_candidate(value: &Value, project: &str) -> Option<AttachmentCandidate> {
    let object = value.as_object()?;
    if let Some(inline) = object
        .get("inlineData")
        .or_else(|| object.get("inline_data"))
    {
        return embedded_inline_candidate(value, inline);
    }
    let raw_type = object.get("type").and_then(Value::as_str).unwrap_or("");
    let kind = if raw_type.contains("image")
        || object
            .get("mimeType")
            .and_then(Value::as_str)
            .is_some_and(|mime| mime.starts_with("image/"))
        || object
            .get("mime_type")
            .and_then(Value::as_str)
            .is_some_and(|mime| mime.starts_with("image/"))
    {
        AttachmentKind::Image
    } else if raw_type.contains("file") || raw_type.contains("attachment") {
        AttachmentKind::File
    } else {
        return None;
    };
    let source = ["file_path", "path", "url", "image_url", "uri"]
        .iter()
        .find_map(|key| object.get(*key).and_then(attachment_source_value))?;
    finish_attachment_candidate(value, kind, source, project)
}

fn embedded_inline_candidate(value: &Value, inline: &Value) -> Option<AttachmentCandidate> {
    let mime = first_text(inline, &["mimeType", "mime_type", "mime"]);
    let mime = if mime.is_empty() {
        first_text(value, &["mimeType", "mime_type", "mime"])
    } else {
        mime
    };
    let mut data = first_text(inline, &["data"]);
    if data.is_empty() {
        data = inline
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("")
            .to_string();
    }
    if data.is_empty() {
        return None;
    }
    if data.starts_with("http://") || data.starts_with("https://") {
        return finish_attachment_candidate(value, AttachmentKind::Image, data, "");
    }
    let media_type = if mime.starts_with("image/") {
        mime
    } else if mime.is_empty() {
        "image/png".to_string()
    } else {
        mime
    };
    let kind = if media_type.starts_with("image/") {
        AttachmentKind::Image
    } else {
        AttachmentKind::File
    };
    let source = if data.starts_with("data:") {
        data
    } else {
        format!("data:{media_type};base64,{data}")
    };
    let mut candidate = finish_attachment_candidate(value, kind, source, "")?;
    candidate.attachment.media_type = media_type;
    candidate.attachment.size_bytes = embedded_size_bytes(&candidate.source);
    Some(candidate)
}

fn finish_attachment_candidate(
    value: &Value,
    kind: AttachmentKind,
    source: String,
    project: &str,
) -> Option<AttachmentCandidate> {
    let embedded = source.starts_with("data:");
    let remote = source.starts_with("http://") || source.starts_with("https://");
    let resolved_path = if embedded || remote {
        None
    } else {
        let path = PathBuf::from(&source);
        Some(if path.is_absolute() || project.is_empty() {
            path
        } else {
            PathBuf::from(project).join(path)
        })
    };
    let metadata = resolved_path
        .as_ref()
        .and_then(|path| fs::metadata(path).ok());
    let status = if embedded {
        AttachmentStatus::Embedded
    } else if remote {
        AttachmentStatus::Unsupported
    } else if metadata.is_some() {
        AttachmentStatus::Available
    } else {
        AttachmentStatus::Missing
    };
    let original_path = if embedded {
        "内嵌图片数据".to_string()
    } else {
        source.clone()
    };
    let name = first_text(value, &["name", "file_name", "filename"]);
    let name = if name.is_empty() {
        Path::new(&original_path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(if kind == AttachmentKind::Image {
                "image"
            } else {
                "attachment"
            })
            .to_string()
    } else {
        name
    };
    let media_type = optional_text(value, &["mime_type", "media_type", "mimeType", "mime"])
        .unwrap_or_else(|| infer_media_type(&name, kind));
    Some(AttachmentCandidate {
        attachment: ConversationAttachment {
            id: String::new(),
            kind,
            name,
            original_path,
            media_type,
            size_bytes: metadata.map(|metadata| metadata.len()).or_else(|| {
                if embedded {
                    embedded_size_bytes(&source)
                } else {
                    None
                }
            }),
            status,
        },
        source,
        resolved_path,
    })
}

fn embedded_size_bytes(source: &str) -> Option<u64> {
    let encoded = source.split_once(',')?.1;
    let padding = encoded
        .bytes()
        .rev()
        .take_while(|byte| *byte == b'=')
        .count();
    Some(((encoded.len() * 3 / 4).saturating_sub(padding)) as u64)
}

fn attachment_source_value(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_string)
        .or_else(|| value.get("url").and_then(Value::as_str).map(str::to_string))
}

fn infer_media_type(name: &str, kind: AttachmentKind) -> String {
    let extension = Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        "json" => "application/json",
        "md" => "text/markdown",
        "txt" => "text/plain",
        _ if kind == AttachmentKind::Image => "image/*",
        _ => "application/octet-stream",
    }
    .to_string()
}

fn read_source_payload(source: Source, path: &Path, sequence: u32) -> Result<Value, String> {
    if source == Source::Gemini {
        let root = if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("jsonl"))
        {
            super::gemini::fold_gemini_jsonl(path)?
        } else {
            let file =
                fs::File::open(path).map_err(|error| format!("读取原始文件失败：{error}"))?;
            serde_json::from_reader(BufReader::new(file))
                .map_err(|error| format!("附件所在事件 JSON 无效：{error}"))?
        };
        return root
            .get("messages")
            .and_then(Value::as_array)
            .and_then(|messages| messages.get(sequence as usize))
            .cloned()
            .ok_or_else(|| "原始文件中未找到附件所在事件".to_string());
    }
    let raw = read_source_line(path, sequence)?;
    let value: Value =
        serde_json::from_str(&raw).map_err(|error| format!("附件所在事件 JSON 无效：{error}"))?;
    Ok(value.get("payload").cloned().unwrap_or(value))
}

fn ensure_attachment_path_allowed(
    candidate: &AttachmentCandidate,
    project: &str,
) -> Result<(), String> {
    if candidate.attachment.status != AttachmentStatus::Available {
        return Ok(());
    }
    let path = candidate
        .resolved_path
        .as_ref()
        .ok_or_else(|| "附件路径不可用".to_string())?;
    let canonical_path =
        fs::canonicalize(path).map_err(|_| "原附件已不存在，无法加载图片".to_string())?;
    let project_path = Path::new(project);
    if !project_path.is_absolute() {
        return Err("附件路径不在会话项目允许的目录内".to_string());
    }
    let project_root =
        fs::canonicalize(project_path).map_err(|_| "会话项目目录不可用".to_string())?;
    if project_root.parent().is_some() && canonical_path.starts_with(project_root) {
        Ok(())
    } else {
        Err("附件路径不在会话项目允许的目录内".to_string())
    }
}

fn attachment_data_url(candidate: &AttachmentCandidate) -> Result<String, String> {
    if candidate.attachment.status == AttachmentStatus::Embedded {
        if candidate.source.starts_with("data:image/") {
            return Ok(candidate.source.clone());
        }
        return Err("内嵌附件不是可预览的图片".to_string());
    }
    let bytes = attachment_bytes(candidate)?;
    Ok(format!(
        "data:{};base64,{}",
        candidate.attachment.media_type,
        BASE64_STANDARD.encode(bytes)
    ))
}

fn attachment_thumbnail_data_url(candidate: &AttachmentCandidate) -> Result<String, String> {
    let bytes = attachment_bytes(candidate)?;
    let image =
        image::load_from_memory(&bytes).map_err(|error| format!("图片格式无效：{error}"))?;
    let thumbnail = image.thumbnail(
        image.width().min(THUMBNAIL_MAX_WIDTH),
        image.height().min(THUMBNAIL_MAX_HEIGHT),
    );
    let mut encoded = Cursor::new(Vec::new());
    thumbnail
        .write_to(&mut encoded, image::ImageFormat::Png)
        .map_err(|error| format!("生成图片缩略图失败：{error}"))?;
    Ok(format!(
        "data:image/png;base64,{}",
        BASE64_STANDARD.encode(encoded.into_inner())
    ))
}

fn attachment_bytes(candidate: &AttachmentCandidate) -> Result<Vec<u8>, String> {
    match candidate.attachment.status {
        AttachmentStatus::Embedded => {
            let (metadata, encoded) = candidate
                .source
                .split_once(',')
                .ok_or_else(|| "内嵌图片数据无效".to_string())?;
            if !metadata.starts_with("data:image/") || !metadata.ends_with(";base64") {
                return Err("内嵌附件不是可预览的图片".to_string());
            }
            BASE64_STANDARD
                .decode(encoded)
                .map_err(|error| format!("内嵌图片数据无效：{error}"))
        }
        AttachmentStatus::Missing => Err("原附件已不存在，无法加载图片".to_string()),
        AttachmentStatus::Unsupported => Err("远程附件不在应用内加载".to_string()),
        AttachmentStatus::Available => {
            let path = candidate
                .resolved_path
                .as_ref()
                .ok_or_else(|| "附件路径不可用".to_string())?;
            fs::read(path).map_err(|error| format!("读取原附件失败：{error}"))
        }
    }
}
