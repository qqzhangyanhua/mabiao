use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde_json::Value;

use super::session_store::{ensure_matching_session, load_trusted_session_files};
use super::toolbox::ParsedConversation;
use super::{
    event_index, event_index_ready, line_direct, parse_conversation_files, prepare_detail, qwen,
    raw_export_extension, rebuild_events_from_line, MAX_PAGE_SIZE,
};
use crate::domain::{
    ConversationAttachmentStatus as AttachmentStatus, ConversationEvent, ConversationEventAnchor,
    ConversationExportDto, ConversationExportFormat, ConversationSessionRow, Source,
};
use crate::user_files;

pub fn build_export(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
    format: ConversationExportFormat,
) -> Result<ConversationExportDto, String> {
    let mut content = Vec::new();
    let default_name = stream_export(conn, home, source, session_id, format, &mut content)?;
    Ok(ConversationExportDto {
        default_name,
        content,
    })
}

#[cfg(test)]
pub fn parsed_export(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
    format: ConversationExportFormat,
) -> Result<ConversationExportDto, String> {
    let (source, session, paths) = prepare_export(conn, home, source, session_id, format)?;
    let content = match format {
        ConversationExportFormat::Json => raw_json_bytes(source, &session.session_id, &paths)?,
        ConversationExportFormat::Markdown => {
            let mut content = Vec::new();
            write_parsed_markdown(source, &session, &paths, &mut content)?;
            content
        }
    };
    Ok(ConversationExportDto {
        default_name: export_file_name(&session, source, format)?,
        content,
    })
}

pub fn export_default_name(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
    format: ConversationExportFormat,
) -> Result<String, String> {
    let (source, session, _) = prepare_export(conn, home, source, session_id, format)?;
    export_file_name(&session, source, format)
}

fn stream_export(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
    format: ConversationExportFormat,
    writer: &mut dyn Write,
) -> Result<String, String> {
    let (source, session, paths) = prepare_export(conn, home, source, session_id, format)?;
    match format {
        ConversationExportFormat::Json => {
            stream_raw_json(source, &session.session_id, &paths, writer)?
        }
        ConversationExportFormat::Markdown => {
            stream_markdown(conn, home, source, &session, &paths, writer)?;
        }
    }
    export_file_name(&session, source, format)
}

pub fn write_conversation_export(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
    format: ConversationExportFormat,
    path: &Path,
    expected_mtime: Option<&str>,
) -> Result<(), String> {
    user_files::write_export_with(path, expected_mtime, |writer| {
        stream_export(conn, home, source, session_id, format, writer).map(|_| ())
    })?;
    Ok(())
}

fn prepare_export(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
    format: ConversationExportFormat,
) -> Result<(Source, ConversationSessionRow, Vec<PathBuf>), String> {
    let (source, session, paths) = load_trusted_session_files(conn, home, source, session_id)?;
    validate_export_format(source, &paths, format)?;
    Ok((source, session, paths))
}

fn validate_export_format(
    source: Source,
    paths: &[PathBuf],
    format: ConversationExportFormat,
) -> Result<(), String> {
    match format {
        ConversationExportFormat::Json if raw_export_extension(source)?.is_none() => {
            Err("该来源不支持导出单一原始对话文件".to_string())
        }
        ConversationExportFormat::Json if paths.len() > 1 => {
            Err("该会话包含多个原始文件，无法导出为单一原始 JSONL".to_string())
        }
        _ => Ok(()),
    }
}

fn export_file_name(
    session: &ConversationSessionRow,
    source: Source,
    format: ConversationExportFormat,
) -> Result<String, String> {
    let base_name = safe_export_name(&session.title, &session.session_id);
    Ok(match format {
        ConversationExportFormat::Markdown => format!("{base_name}.md"),
        ConversationExportFormat::Json if source == Source::Qwen => format!("{base_name}.json"),
        ConversationExportFormat::Json => format!(
            "{base_name}.{}",
            raw_export_extension(source)?.unwrap_or("jsonl")
        ),
    })
}

fn stream_markdown(
    conn: &Connection,
    home: &Path,
    source: Source,
    session: &ConversationSessionRow,
    paths: &[PathBuf],
    writer: &mut dyn Write,
) -> Result<(), String> {
    let prepared = prepare_detail(conn, source.as_str(), &session.session_id)?;
    if event_index_ready(conn, home, &prepared)? && line_direct::source_maps_line_to_events(source)
    {
        stream_indexed_markdown(conn, source, session, writer)
    } else {
        write_parsed_markdown(source, session, paths, writer)
    }
}

fn stream_indexed_markdown(
    conn: &Connection,
    source: Source,
    session: &ConversationSessionRow,
    writer: &mut dyn Write,
) -> Result<(), String> {
    write_markdown_header(writer, session)?;
    let mut anchor = ConversationEventAnchor::First;
    loop {
        let page = event_index::indexed_events_page(
            conn,
            source.as_str(),
            &session.session_id,
            &anchor,
            MAX_PAGE_SIZE,
        )?;
        if page.events.is_empty() {
            break;
        }
        for event in &page.events {
            write_markdown_event(writer, &hydrate_export_event(source, session, event)?)?;
        }
        if !page.has_more_after {
            break;
        }
        let Some(last) = page.events.last() else {
            break;
        };
        anchor = ConversationEventAnchor::After {
            sequence: last.sequence,
        };
    }
    Ok(())
}

fn write_parsed_markdown(
    source: Source,
    session: &ConversationSessionRow,
    paths: &[PathBuf],
    writer: &mut dyn Write,
) -> Result<(), String> {
    let parsed = parsed_conversation(source, session, paths)?;
    write_markdown_header(writer, &parsed.session)?;
    for event in &parsed.events {
        write_markdown_event(writer, event)?;
    }
    Ok(())
}

fn parsed_conversation(
    source: Source,
    session: &ConversationSessionRow,
    paths: &[PathBuf],
) -> Result<ParsedConversation, String> {
    let parsed = parse_conversation_files(source, paths, &session.session_id, true)?;
    ensure_matching_session(&parsed, session)?;
    Ok(parsed)
}

fn hydrate_export_event(
    source: Source,
    session: &ConversationSessionRow,
    indexed: &ConversationEvent,
) -> Result<ConversationEvent, String> {
    let rebuilt = rebuild_events_from_line(
        source,
        Path::new(&indexed.source_file),
        &session.session_id,
        indexed.source_sequence,
        true,
    )?;
    let mut event = rebuilt
        .into_iter()
        .find(|event| event.event_id == indexed.event_id)
        .ok_or_else(|| "原始文件中未找到该事件".to_string())?;
    event.sequence = indexed.sequence;
    Ok(event)
}

fn stream_raw_json(
    source: Source,
    session_id: &str,
    paths: &[PathBuf],
    writer: &mut dyn Write,
) -> Result<(), String> {
    if source == Source::Qwen {
        return writer
            .write_all(&qwen::export_session_records(&paths[0], session_id)?)
            .map_err(write_error);
    }
    let mut source_file =
        fs::File::open(&paths[0]).map_err(|error| format!("读取原始文件失败：{error}"))?;
    io::copy(&mut source_file, writer).map_err(|error| format!("导出原始文件失败：{error}"))?;
    Ok(())
}

#[cfg(test)]
fn raw_json_bytes(source: Source, session_id: &str, paths: &[PathBuf]) -> Result<Vec<u8>, String> {
    let mut content = Vec::new();
    stream_raw_json(source, session_id, paths, &mut content)?;
    Ok(content)
}

fn write_markdown_header(
    writer: &mut dyn Write,
    session: &ConversationSessionRow,
) -> Result<(), String> {
    write_fmt(
        writer,
        format_args!(
            "# {}\n\n- 来源：{}\n- 会话 ID：`{}`\n- 项目：{}\n- 模型：{}\n- 开始：{}\n- 结束：{}\n\n",
            session.title,
            session.source,
            session.session_id,
            explicit_value(&session.project),
            explicit_value(&session.model),
            explicit_value(&session.started_at),
            explicit_value(&session.ended_at),
        ),
    )
}

fn write_markdown_event(writer: &mut dyn Write, event: &ConversationEvent) -> Result<(), String> {
    write_fmt(
        writer,
        format_args!(
            "---\n\n## {} · {}\n\n- 时间：{}\n",
            event.sequence,
            event.kind.as_str(),
            event.occurred_at.as_deref().unwrap_or("时间缺失")
        ),
    )?;
    if let Some(actor) = event.actor {
        write_fmt(writer, format_args!("- 角色：{}\n", actor.as_str()))?;
    }
    if let Some(name) = &event.name {
        write_fmt(writer, format_args!("- 名称：`{name}`\n"))?;
    }
    if let Some(text) = &event.text {
        write_str(writer, "\n")?;
        write_str(writer, text)?;
        write_str(writer, "\n")?;
    }
    if !event.attachments.is_empty() {
        write_str(writer, "\n### 附件\n\n")?;
        for attachment in &event.attachments {
            let status = match attachment.status {
                AttachmentStatus::Available => "可用",
                AttachmentStatus::Missing => "附件缺失",
                AttachmentStatus::Embedded => "内嵌",
                AttachmentStatus::Unsupported => "不支持应用内加载",
            };
            let size = attachment
                .size_bytes
                .map(|size| format!("{size} bytes"))
                .unwrap_or_else(|| "大小未知".to_string());
            write_fmt(
                writer,
                format_args!(
                    "- **{}** · `{}` · {} · {} · {}\n",
                    attachment.name, attachment.original_path, attachment.media_type, size, status
                ),
            )?;
        }
    }
    if let Some(details) = export_details(&event.details) {
        write_str(
            writer,
            "\n<details><summary>原始事件数据</summary>\n\n```json\n",
        )?;
        write_str(writer, &details)?;
        write_str(writer, "\n```\n\n</details>\n")?;
    }
    Ok(())
}

fn safe_export_name(title: &str, session_id: &str) -> String {
    let source = if title.trim().is_empty() {
        session_id
    } else {
        title.trim()
    };
    let name: String = source
        .chars()
        .map(|character| match character {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ => character,
        })
        .take(100)
        .collect();
    if name.is_empty() {
        "conversation".to_string()
    } else {
        name
    }
}

fn explicit_value(value: &str) -> &str {
    if value.is_empty() {
        "缺失"
    } else {
        value
    }
}

fn export_details(details: &Value) -> Option<String> {
    let mut details = details.clone();
    if let Value::Object(object) = &mut details {
        object.remove("content");
        object.remove("message");
        object.remove("output");
        object.remove("result");
        if object.is_empty() {
            return None;
        }
    } else if details.is_null() {
        return None;
    }
    serde_json::to_string_pretty(&details).ok()
}

fn write_fmt(writer: &mut dyn Write, args: std::fmt::Arguments<'_>) -> Result<(), String> {
    writer.write_fmt(args).map_err(write_error)
}

fn write_str(writer: &mut dyn Write, text: &str) -> Result<(), String> {
    writer.write_all(text.as_bytes()).map_err(write_error)
}

fn write_error(error: io::Error) -> String {
    format!("写入导出文件失败：{error}")
}
