//! 取源码行与附件读取、编码、路径许可。
//!
//! 缩略图尺寸与附件目录约束都在这里；不碰 sqlite、合并语义或路径白名单。

use std::fs;
use std::io::{BufRead, BufReader, Cursor};
use std::path::Path;

use base64::prelude::*;
use serde_json::Value;

use crate::domain::{ConversationAttachmentStatus as AttachmentStatus, Source};

use super::toolbox::AttachmentCandidate;

pub(crate) const THUMBNAIL_MAX_WIDTH: u32 = 320;
pub(crate) const THUMBNAIL_MAX_HEIGHT: u32 = 240;

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

pub(super) fn read_source_payload(
    source: Source,
    path: &Path,
    sequence: u32,
) -> Result<Value, String> {
    if source == Source::Gemini {
        let file = fs::File::open(path).map_err(|error| format!("读取原始文件失败：{error}"))?;
        let root: Value = serde_json::from_reader(BufReader::new(file))
            .map_err(|error| format!("附件所在事件 JSON 无效：{error}"))?;
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

pub(super) fn ensure_attachment_path_allowed(
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

pub(crate) fn attachment_data_url(candidate: &AttachmentCandidate) -> Result<String, String> {
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

pub(crate) fn attachment_thumbnail_data_url(
    candidate: &AttachmentCandidate,
) -> Result<String, String> {
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

pub(crate) fn attachment_bytes(candidate: &AttachmentCandidate) -> Result<Vec<u8>, String> {
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
