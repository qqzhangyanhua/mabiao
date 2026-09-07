use std::path::{Path, PathBuf};

/// 手写 hex：字段路径契约。空白分隔，便于对照字段号。
///
/// `steps.metadata` = `CortexStepMetadata`
///   1 → Timestamp { 1 seconds, 2 nanos }
///   9 → ModelUsageStats { 2 input, 3 output, 4 cache_write, 5 cache_read,
///                         9 thinking, 10 response, 11 request id }
///
/// `gen_metadata.data` 包装一层后是 `ChatModelMetadata`
///   4 → 客户端估算用量（不进消耗记录）
///   19 → 模型名
pub fn decode_hex(hex: &str) -> Vec<u8> {
    hex.split_whitespace()
        .map(|byte| u8::from_str_radix(byte, 16).expect("agy fixture hex"))
        .collect()
}

/// 2026-04-05T08:00:00Z = 1775376000。
/// `0a 08` = 字段 1 / Timestamp；`08 80 ad c8 ce 06` = seconds；`10 00` = nanos 0。
/// `4a 13` = 字段 9 / 19 字节用量：input 100、output 50、cache_write 7、
/// cache_read 20、thinking 30、response 20、request id `req-a`。
pub const STEP_SIX_TUPLE: &str = "\
0a 08 08 80 ad c8 ce 06 10 00 \
4a 13 10 64 18 32 20 07 28 14 48 1e 50 14 5a 05 72 65 71 2d 61";

/// 同一 request id，较小 output（10），供去重取 max。
pub const STEP_DUP_LOW: &str = "\
0a 08 08 80 ad c8 ce 06 10 00 \
4a 13 10 0a 18 0a 20 00 28 00 48 06 50 04 5a 05 72 65 71 2d 61";

/// 同一 request id，较大 output（50）。
pub const STEP_DUP_HIGH: &str = "\
0a 08 08 80 ad c8 ce 06 10 00 \
4a 13 10 50 18 32 20 03 28 0c 48 1e 50 14 5a 05 72 65 71 2d 61";

/// 不同 request id `req-b`。
pub const STEP_REQ_B: &str = "\
0a 08 08 80 ad c8 ce 06 10 00 \
4a 13 10 05 18 08 20 00 28 00 48 03 50 05 5a 05 72 65 71 2d 62";

/// 缺 input / cache：只含 output 50、thinking 30、response 20、`req-miss`。
pub const STEP_MISSING_FIELDS: &str = "\
0a 08 08 80 ad c8 ce 06 10 00 \
4a 10 18 32 48 1e 50 14 5a 08 72 65 71 2d 6d 69 73 73";

/// 字段 2 误用 length-delimited（`12 02 78 78`），其余仍可读。
pub const STEP_WRONG_WIRE: &str = "\
0a 08 08 80 ad c8 ce 06 10 00 \
4a 10 12 02 78 78 18 09 5a 08 72 65 71 2d 77 69 72 65";

/// 字段 1 声称 6 字节但立刻截断。
pub const STEP_TRUNCATED: &str = "0a 06";

/// 只有时间戳，没有字段 9 用量路径。
pub const STEP_MISSING_PATH: &str = "0a 08 08 80 ad c8 ce 06 10 00";

/// 包装字段 1 → ChatModelMetadata：字段 4 是客户端估算（input 9999），
/// 字段 19 = `gemini-3.8-flash-high`，request id 与 `STEP_SIX_TUPLE` 对齐。
pub const GEN_HIGH: &str = "\
0a 26 22 0c 10 8f 4e 18 01 5a 05 72 65 71 2d 61 \
9a 01 15 67 65 6d 69 6e 69 2d 33 2e 38 2d 66 6c 61 73 68 2d 68 69 67 68";

/// 同上，模型名为 `claude-sonnet-4-6`。
pub const GEN_CLAUDE: &str = "\
0a 22 22 0c 10 8f 4e 18 01 5a 05 72 65 71 2d 61 \
9a 01 11 63 6c 61 75 64 65 2d 73 6f 6e 6e 65 74 2d 34 2d 36";

/// 只有 request id，没有字段 19 模型名。
pub const GEN_NO_MODEL: &str = "0a 09 22 07 5a 05 72 65 71 2d 61";

pub fn write_agy_conversation_db(path: &Path, steps: &[&str], generations: &[&str]) -> PathBuf {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create agy fixture dir");
    }
    let db = rusqlite::Connection::open(path).expect("open agy fixture db");
    db.execute_batch(
        r#"
        CREATE TABLE steps (idx INTEGER PRIMARY KEY, metadata BLOB);
        CREATE TABLE gen_metadata (idx INTEGER PRIMARY KEY, data BLOB);
        "#,
    )
    .expect("create agy fixture tables");
    for (idx, hex) in steps.iter().enumerate() {
        db.execute(
            "INSERT INTO steps (idx, metadata) VALUES (?1, ?2)",
            rusqlite::params![idx as i64, decode_hex(hex)],
        )
        .expect("insert agy step");
    }
    for (idx, hex) in generations.iter().enumerate() {
        db.execute(
            "INSERT INTO gen_metadata (idx, data) VALUES (?1, ?2)",
            rusqlite::params![idx as i64, decode_hex(hex)],
        )
        .expect("insert agy generation");
    }
    path.to_path_buf()
}
