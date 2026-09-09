//! 构造最小 Cursor 会话存储库：内容寻址 blobs + repeated bytes root + meta。
//! 测试走真实 protobuf 还原路径，不绕过解析。

use std::path::{Path, PathBuf};

use rusqlite::params;
use serde_json::Value;
use sha2::{Digest, Sha256};

const PROJECT_HASH: &str = "0123456789abcdef0123456789abcdef";

/// 在临时 home 写下 `~/.cursor/chats/<hash>/<session>/store.db`。
/// `messages` 按会话顺序写入；id = 内容 sha256 hex。
pub fn write_cursor_chat_store(home: &Path, session_id: &str, messages: &[Value]) -> PathBuf {
    let dir = home
        .join(".cursor/chats")
        .join(PROJECT_HASH)
        .join(session_id);
    std::fs::create_dir_all(&dir).expect("create cursor chat dir");
    let path = dir.join("store.db");
    let db = rusqlite::Connection::open(&path).expect("open cursor store.db");
    db.execute_batch(
        r#"
        CREATE TABLE blobs (id TEXT PRIMARY KEY, data BLOB NOT NULL);
        CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
        "#,
    )
    .expect("create cursor store tables");

    let mut root = Vec::new();
    for message in messages {
        let data = serde_json::to_vec(message).expect("serialize cursor message blob");
        let digest = Sha256::digest(&data);
        let id = to_hex(digest.as_slice());
        db.execute(
            "INSERT INTO blobs (id, data) VALUES (?1, ?2)",
            params![id, data],
        )
        .expect("insert cursor message blob");
        root.extend(encode_bytes_field(1, digest.as_slice()));
    }

    let root_id = to_hex(&Sha256::digest(&root));
    db.execute(
        "INSERT INTO blobs (id, data) VALUES (?1, ?2)",
        params![root_id, root],
    )
    .expect("insert cursor root blob");

    let meta_json = serde_json::json!({ "latestRootBlobId": root_id });
    let meta_hex = to_hex(meta_json.to_string().as_bytes());
    db.execute(
        "INSERT INTO meta (key, value) VALUES ('0', ?1)",
        params![meta_hex],
    )
    .expect("insert cursor store meta");
    path
}

fn encode_varint(mut value: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            break;
        }
    }
    out
}

fn encode_bytes_field(field: u32, payload: &[u8]) -> Vec<u8> {
    let tag = (u64::from(field) << 3) | 2;
    let mut out = encode_varint(tag);
    out.extend(encode_varint(payload.len() as u64));
    out.extend_from_slice(payload);
    out
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
