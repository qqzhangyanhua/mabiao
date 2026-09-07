//! 无 schema 的 protobuf wire 工具：varint 与「字段号路径 → 值」。
//!
//! 官方额度与 agy Adapter 共用同一套读取，避免两套 varint 各自漂移。
//! 不引入 protobuf 运行时；形状对不上时返回 `None`，由调用方降级。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WireValue<'a> {
    Varint(u64),
    Bytes(&'a [u8]),
    Fixed64,
    Fixed32,
}

/// 从 `i` 起读一个 base-128 varint，返回 `(值, 下一字节下标)`。
///
/// 超过 10 字节或位移溢出视为畸形，返回 `None`（与原先额度模块一致）。
pub(crate) fn read_varint(buf: &[u8], mut i: usize) -> Option<(u64, usize)> {
    let mut result = 0u64;
    let mut shift = 0u32;
    loop {
        let byte = *buf.get(i)?;
        i += 1;
        result |= u64::from(byte & 0x7f).checked_shl(shift)?;
        if byte & 0x80 == 0 {
            return Some((result, i));
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }
}

/// `read_varint` 的游标形式，给 grok gRPC 扫描用。
pub(crate) fn read_varint_from(bytes: &[u8], index: &mut usize) -> Option<u64> {
    let (value, next) = read_varint(bytes, *index)?;
    *index = next;
    Some(value)
}

pub(crate) fn next_field(buf: &[u8], i: usize) -> Option<(u64, WireValue<'_>, usize)> {
    let (tag, after_tag) = read_varint(buf, i)?;
    let field = tag >> 3;
    if field == 0 {
        return None;
    }
    match tag & 7 {
        0 => {
            let (value, next) = read_varint(buf, after_tag)?;
            Some((field, WireValue::Varint(value), next))
        }
        1 => {
            let end = after_tag.checked_add(8)?;
            let _ = buf.get(after_tag..end)?;
            Some((field, WireValue::Fixed64, end))
        }
        2 => {
            let (len, start) = read_varint(buf, after_tag)?;
            let len = usize::try_from(len).ok()?;
            let end = start.checked_add(len)?;
            let bytes = buf.get(start..end)?;
            Some((field, WireValue::Bytes(bytes), end))
        }
        5 => {
            let end = after_tag.checked_add(4)?;
            let _ = buf.get(after_tag..end)?;
            Some((field, WireValue::Fixed32, end))
        }
        _ => None,
    }
}

/// 按字段号路径取值。中间节点必须是 length-delimited 子消息；
/// 同号字段取最后一个（proto3）。路径不存在或 wire type 不符返回 `None`。
pub(crate) fn value_at_path<'a>(buf: &'a [u8], path: &[u32]) -> Option<WireValue<'a>> {
    let (first, rest) = path.split_first()?;
    let mut found = None;
    let mut i = 0;
    while i < buf.len() {
        let Some((field, value, next)) = next_field(buf, i) else {
            break;
        };
        i = next;
        if field == u64::from(*first) {
            found = Some(value);
        }
    }
    let value = found?;
    if rest.is_empty() {
        return Some(value);
    }
    match value {
        WireValue::Bytes(inner) => value_at_path(inner, rest),
        _ => None,
    }
}

pub(crate) fn varint_at_path(buf: &[u8], path: &[u32]) -> Option<u64> {
    match value_at_path(buf, path)? {
        WireValue::Varint(value) => Some(value),
        _ => None,
    }
}

pub(crate) fn bytes_at_path<'a>(buf: &'a [u8], path: &[u32]) -> Option<&'a [u8]> {
    match value_at_path(buf, path)? {
        WireValue::Bytes(bytes) => Some(bytes),
        _ => None,
    }
}

pub(crate) fn text_at_path<'a>(buf: &'a [u8], path: &[u32]) -> Option<&'a str> {
    let bytes = bytes_at_path(buf, path)?;
    let text = std::str::from_utf8(bytes).ok()?.trim();
    (!text.is_empty()).then_some(text)
}

/// 遍历顶层 length-delimited 字段，给包装消息「按形状找」用。
pub(crate) fn for_each_bytes_field(buf: &[u8], mut visit: impl FnMut(u64, &[u8])) {
    let mut i = 0;
    while i < buf.len() {
        let Some((field, value, next)) = next_field(buf, i) else {
            break;
        };
        i = next;
        if let WireValue::Bytes(bytes) = value {
            visit(field, bytes);
        }
    }
}
