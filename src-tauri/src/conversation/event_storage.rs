//! 事件行在库里的存储表示（ADR 0025）。
//!
//! 两件事：`event_id` 里能由文件路径与行号推导的部分不逐行存；能从源文件某一行无上下文重建
//! 的正文只存位置与指纹，读取时回源文件取。对外的 `ConversationEvent` 不变。

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;

use base64::prelude::*;
use rusqlite::{params, Connection};

use crate::domain::{ConversationEvent, ConversationEventKind as EventKind, Source};
use crate::store::STORAGE_STALE_ADAPTER_VERSION;

use super::codex::is_codex_zst;
use super::line_direct::rebuild_events_from_raw;
use super::toolbox::{event_id_for, FileIndexCursor};
use super::CONVERSATION_ADAPTER_VERSION;

/// 本机各抽 1500 条，按行重建的正文与整份解析逐条一致。Claude 会跨行合并，不在此列。
const TEXT_REFERENCE_SOURCES: &[Source] = &[Source::Codex, Source::Pi, Source::Omp];

/// `tool_call` / `system_status` / `error` 的正文在打开详情时要被上下文清单整场读，留在库里。
const TEXT_REFERENCE_KINDS: &[EventKind] =
    &[EventKind::ToolResult, EventKind::Message, EventKind::Plan];
const ADOPT_BATCH_ROWS: i64 = 10_000;

#[derive(Debug, Clone, Copy)]
pub(super) struct TextRef {
    pub(super) line_offset: i64,
    pub(super) text_hash: i64,
}

pub(super) fn stored_event_id(event: &ConversationEvent) -> String {
    compact_event_id(
        &event.event_id,
        &event_id_for(&event.source_file, event.source_sequence),
    )
}

fn compact_event_id(event_id: &str, derived: &str) -> String {
    match event_id.strip_prefix(derived) {
        Some(rest) if rest.is_empty() || rest.starts_with(':') => rest.to_string(),
        _ => event_id.to_string(),
    }
}

pub(super) fn restore_event_id(stored: String, source_file: &str, source_sequence: u32) -> String {
    if stored.is_empty() || stored.starts_with(':') {
        format!("{}{stored}", event_id_for(source_file, source_sequence))
    } else {
        stored
    }
}

/// 推导形态的 `event_id` 拆回 (源文件路径, 行号, 库内存的后缀)。原生 ID 解不出来时返回 `None`。
pub(super) fn split_derived_event_id(event_id: &str) -> Option<(String, u32, String)> {
    let mut parts = event_id.splitn(3, ':');
    let encoded = parts.next()?;
    let sequence = parts.next()?.parse::<u32>().ok()?;
    let suffix = parts
        .next()
        .map(|rest| format!(":{rest}"))
        .unwrap_or_default();
    let path = String::from_utf8(BASE64_URL_SAFE_NO_PAD.decode(encoded).ok()?).ok()?;
    (event_id_for(&path, sequence) + &suffix == event_id).then_some((path, sequence, suffix))
}

pub(super) fn text_hash(text: &str) -> i64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash as i64
}

/// 压缩文件的字节偏移指不到明文行，正文只能留在库里。
fn references_text(source: Source, event: &ConversationEvent) -> bool {
    TEXT_REFERENCE_SOURCES.contains(&source)
        && TEXT_REFERENCE_KINDS.contains(&event.kind)
        && !is_codex_zst(Path::new(&event.source_file))
        && event.text.as_deref().is_some_and(|text| !text.is_empty())
}

/// 摄取时决定哪些事件的正文外置。`origin` 是这批事件所在文件被解析的起点（后缀增量时不是
/// 文件头）。只有按位置读回那一行、重建出的正文与解析结果一致才外置；读不到或对不上就照旧
/// 存正文，不赌读取时会对上。
pub(super) fn plan_text_refs(
    source: Source,
    session_id: &str,
    events: &[ConversationEvent],
    origin: FileIndexCursor,
) -> Vec<Option<TextRef>> {
    let mut refs = vec![None; events.len()];
    let mut by_file = BTreeMap::<&str, BTreeMap<u32, Vec<usize>>>::new();
    for (index, event) in events.iter().enumerate() {
        if references_text(source, event) {
            by_file
                .entry(event.source_file.as_str())
                .or_default()
                .entry(event.source_sequence)
                .or_default()
                .push(index);
        }
    }
    for (path, lines) in by_file {
        let _ = plan_file(
            source,
            session_id,
            Path::new(path),
            origin,
            &lines,
            events,
            &mut refs,
        );
    }
    refs
}

fn plan_file(
    source: Source,
    session_id: &str,
    path: &Path,
    origin: FileIndexCursor,
    lines: &BTreeMap<u32, Vec<usize>>,
    events: &[ConversationEvent],
    refs: &mut [Option<TextRef>],
) -> Result<(), String> {
    let Some(&last_line) = lines.keys().next_back() else {
        return Ok(());
    };
    let mut file = fs::File::open(path).map_err(|error| error.to_string())?;
    file.seek(SeekFrom::Start(origin.byte_offset.max(0) as u64))
        .map_err(|error| error.to_string())?;
    let mut reader = BufReader::new(file);
    let mut offset = origin.byte_offset.max(0);
    let mut line = origin.line.max(0) as u32;
    let mut buffer = Vec::new();
    while line <= last_line {
        buffer.clear();
        let read = reader
            .read_until(b'\n', &mut buffer)
            .map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        if let Some(indexes) = lines.get(&line) {
            if let Ok(raw) = std::str::from_utf8(trim_line_end(&buffer)) {
                let rebuilt = rebuild_events_from_raw(source, path, session_id, line, raw, false)
                    .unwrap_or_default();
                for &index in indexes {
                    let event = &events[index];
                    if rebuilt.iter().any(|candidate| {
                        candidate.event_id == event.event_id && candidate.text == event.text
                    }) {
                        refs[index] = event.text.as_deref().map(|text| TextRef {
                            line_offset: offset,
                            text_hash: text_hash(text),
                        });
                    }
                }
            }
        }
        offset += read as i64;
        line += 1;
    }
    Ok(())
}

fn trim_line_end(buffer: &[u8]) -> &[u8] {
    let buffer = buffer.strip_suffix(b"\n").unwrap_or(buffer);
    buffer.strip_suffix(b"\r").unwrap_or(buffer)
}

/// 按位置回源文件取外置正文。任何一条读不到或指纹对不上就返回 `false`，调用方整份退回
/// 解析源文件，不拼「部分正文」。
pub(super) fn hydrate_texts(
    source: Source,
    session_id: &str,
    events: &mut [ConversationEvent],
    refs: &[Option<TextRef>],
) -> bool {
    let mut by_file = BTreeMap::<String, Vec<usize>>::new();
    for (index, text_ref) in refs.iter().enumerate() {
        if text_ref.is_some() {
            by_file
                .entry(events[index].source_file.clone())
                .or_default()
                .push(index);
        }
    }
    for (path, mut indexes) in by_file {
        indexes.sort_by_key(|&index| refs[index].map(|text_ref| text_ref.line_offset));
        if hydrate_file(source, session_id, Path::new(&path), &indexes, events, refs).is_err() {
            return false;
        }
    }
    true
}

fn hydrate_file(
    source: Source,
    session_id: &str,
    path: &Path,
    indexes: &[usize],
    events: &mut [ConversationEvent],
    refs: &[Option<TextRef>],
) -> Result<(), String> {
    let mut reader = BufReader::new(fs::File::open(path).map_err(|error| error.to_string())?);
    for &index in indexes {
        let Some(text_ref) = refs[index] else {
            continue;
        };
        let event = &mut events[index];
        let line = ReferencedLine {
            source,
            session_id,
            path,
            event_id: &event.event_id,
            source_sequence: event.source_sequence,
        };
        event.text = Some(read_referenced_text(&mut reader, &line, text_ref)?);
    }
    Ok(())
}

pub(super) struct ReferencedLine<'a> {
    pub(super) source: Source,
    pub(super) session_id: &'a str,
    pub(super) path: &'a Path,
    pub(super) event_id: &'a str,
    pub(super) source_sequence: u32,
}

/// 单条外置正文；读不到或指纹对不上返回 `None`。
pub(super) fn referenced_text(line: &ReferencedLine<'_>, text_ref: TextRef) -> Option<String> {
    let mut reader = BufReader::new(fs::File::open(line.path).ok()?);
    read_referenced_text(&mut reader, line, text_ref).ok()
}

fn read_referenced_text(
    reader: &mut BufReader<fs::File>,
    line: &ReferencedLine<'_>,
    text_ref: TextRef,
) -> Result<String, String> {
    reader
        .seek(SeekFrom::Start(text_ref.line_offset.max(0) as u64))
        .map_err(|error| error.to_string())?;
    let mut buffer = Vec::new();
    reader
        .read_until(b'\n', &mut buffer)
        .map_err(|error| error.to_string())?;
    let raw = std::str::from_utf8(trim_line_end(&buffer)).map_err(|error| error.to_string())?;
    rebuild_events_from_raw(
        line.source,
        line.path,
        line.session_id,
        line.source_sequence,
        raw,
        false,
    )?
    .into_iter()
    .find(|candidate| candidate.event_id == line.event_id)
    .and_then(|candidate| candidate.text)
    .filter(|text| text_hash(text) == text_ref.text_hash)
    .ok_or_else(|| "源文件中的正文已变化".to_string())
}

/// 老库一次性换上新的存储表示：已有行的 `event_id` 改存推导形态；可外置正文来源的会话
/// 标成待补建，补建重写时正文外置。调用方负责事务。
pub fn adopt_text_references(conn: &Connection) -> Result<(), String> {
    compact_stored_event_ids(conn)?;
    let sources = TEXT_REFERENCE_SOURCES
        .iter()
        .map(|source| format!("'{}'", source.as_str()))
        .collect::<Vec<_>>()
        .join(", ");
    conn.execute(
        &format!(
            r#"
            UPDATE conversation_sessions
            SET adapter_version = ?1
            WHERE source IN ({sources})
              AND adapter_version = ?2
              AND event_index_generation IS NOT NULL
            "#
        ),
        params![STORAGE_STALE_ADAPTER_VERSION, CONVERSATION_ADAPTER_VERSION],
    )
    .map(|_| ())
    .map_err(|error| error.to_string())
}

fn conversation_file_paths(conn: &Connection) -> Result<BTreeMap<i64, String>, String> {
    let mut statement = conn
        .prepare("SELECT file_id, path FROM conversation_files")
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<BTreeMap<_, _>, _>>()
        .map_err(|error| error.to_string())
}

fn compact_stored_event_ids(conn: &Connection) -> Result<(), String> {
    let paths = conversation_file_paths(conn)?;
    let mut select = conn
        .prepare(
            r#"
            SELECT rowid, event_id, file_id, source_sequence FROM conversation_events
            WHERE rowid > ?1 ORDER BY rowid LIMIT ?2
            "#,
        )
        .map_err(|error| error.to_string())?;
    let mut update = conn
        .prepare("UPDATE conversation_events SET event_id = ?1 WHERE rowid = ?2")
        .map_err(|error| error.to_string())?;
    let mut after = 0i64;
    loop {
        let batch = select
            .query_map(params![after, ADOPT_BATCH_ROWS], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, u32>(3)?,
                ))
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        let Some(&(last_rowid, ..)) = batch.last() else {
            return Ok(());
        };
        for (rowid, event_id, file_id, source_sequence) in batch {
            let Some(path) = paths.get(&file_id) else {
                continue;
            };
            let compact = compact_event_id(&event_id, &event_id_for(path, source_sequence));
            if compact != event_id {
                update
                    .execute(params![compact, rowid])
                    .map_err(|error| error.to_string())?;
            }
        }
        after = last_rowid;
    }
}
