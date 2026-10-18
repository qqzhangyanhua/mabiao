//! LiteLLM 价目快照：把社区维护的 `model_prices_and_context_window.json` 归一成
//! 「按模型兜底单价」，作为费用推导的兜底层（用户单价、来源自带费用优先）。
//!
//! - 内置一份快照（`assets/litellm_prices.json`，由 `litellm_snapshot` bin 生成）随二进制发布，保证离线可用。
//! - 用户可在设置页「可选刷新」：webview 拉取上游原始 JSON，交给 [`parse_litellm_raw`] 归一后落盘覆盖。
//! - 归一后的条目 `provider` 一律为空，只参与 `query.rs` 里「按模型兜底」的那一路 JOIN。

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;

use serde_json::Value;

use crate::domain::{PriceEntry, PriceOrigin, PriceSnapshot, PriceTable};

/// 上游原始价目文件地址（webview 刷新时拉取）。
pub const SOURCE_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";

/// 快照来源标识。
pub const SOURCE_NAME: &str = "litellm";

/// 随二进制内置的默认快照（归一后的 `PriceSnapshot` JSON）。
const BUNDLED_JSON: &str = include_str!("../assets/litellm_prices.json");

/// 上游中非「按 token 计费的对话类」模型的 mode，跳过以免污染兜底单价。
const SKIP_MODES: &[&str] = &[
    "embedding",
    "image_generation",
    "audio_transcription",
    "audio_speech",
    "moderation",
    "moderations",
    "rerank",
];

/// 解析内置快照；解析失败时返回空快照（不至于让应用启动失败）。
pub fn bundled_snapshot() -> PriceSnapshot {
    serde_json::from_str(BUNDLED_JSON).unwrap_or_default()
}

/// 载入生效的快照：优先用户联网刷新后的本地缓存，否则回落到内置快照。
/// 返回 `(快照, 是否为内置)`。
pub fn load_snapshot(cache_path: &Path) -> (PriceSnapshot, bool) {
    if let Ok(text) = std::fs::read_to_string(cache_path) {
        if let Ok(snapshot) = serde_json::from_str::<PriceSnapshot>(&text) {
            if !snapshot.entries.is_empty() {
                return (snapshot, false);
            }
        }
    }
    (bundled_snapshot(), true)
}

/// 把归一后的快照写入本地缓存。
pub fn save_snapshot(cache_path: &Path, snapshot: &PriceSnapshot) -> Result<(), String> {
    if let Some(parent) = cache_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(snapshot).map_err(|e| e.to_string())?;
    std::fs::write(cache_path, text).map_err(|e| e.to_string())
}

/// 解析上游 LiteLLM 原始 JSON（模型名 → 字段）为归一化快照。
/// `as_of` 由调用方给定（通常是抓取当天的日期）。
pub fn parse_litellm_raw(raw: &str, as_of: &str) -> Result<PriceSnapshot, String> {
    let map: serde_json::Map<String, Value> =
        serde_json::from_str(raw).map_err(|e| format!("LiteLLM 价目 JSON 解析失败：{e}"))?;

    // 同一模型可能在多个 provider 前缀下出现（如 anthropic/、bedrock/、vertex_ai/），
    // 归一后 provider 为空只保留一条：优先「裸键」（无 `/` 前缀，通常是官方直连的规范名）。
    let mut chosen: HashMap<String, (bool, PriceEntry)> = HashMap::new();
    for (key, value) in map {
        if key == "sample_spec" {
            continue;
        }
        let obj = match value.as_object() {
            Some(obj) => obj,
            None => continue,
        };
        if let Some(mode) = obj.get("mode").and_then(|v| v.as_str()) {
            if SKIP_MODES.contains(&mode) {
                continue;
            }
        }
        let input = num(obj, "input_cost_per_token");
        let output = num(obj, "output_cost_per_token");
        // 至少要有一个非零单价，否则一律当作「未定价」，避免用 $0 兜底把未知费用伪装成已核算。
        if input.unwrap_or(0.0) <= 0.0 && output.unwrap_or(0.0) <= 0.0 {
            continue;
        }
        let bare = !key.contains('/');
        let model = key.rsplit('/').next().unwrap_or(key.as_str()).to_string();
        if model.is_empty() {
            continue;
        }
        let entry = PriceEntry {
            model: model.clone(),
            provider: None,
            input: input.unwrap_or(0.0),
            output: output.unwrap_or(0.0),
            cache_read: num(obj, "cache_read_input_token_cost").unwrap_or(0.0),
            cache_creation: num(obj, "cache_creation_input_token_cost").unwrap_or(0.0),
            cache_creation_1h: num(obj, "cache_creation_input_token_cost_above_1hr").unwrap_or(0.0),
            input_above_200k: num(obj, "input_cost_per_token_above_200k_tokens").unwrap_or(0.0),
            output_above_200k: num(obj, "output_cost_per_token_above_200k_tokens").unwrap_or(0.0),
            cache_read_above_200k: num(obj, "cache_read_input_token_cost_above_200k_tokens")
                .unwrap_or(0.0),
            cache_creation_above_200k: num(
                obj,
                "cache_creation_input_token_cost_above_200k_tokens",
            )
            .unwrap_or(0.0),
            cache_creation_1h_above_200k: num(
                obj,
                "cache_creation_input_token_cost_above_1hr_above_200k_tokens",
            )
            .unwrap_or(0.0),
            origin: PriceOrigin::Snapshot,
        };
        match chosen.get(&model) {
            // 已有裸键条目、当前是带前缀的：保留已有。
            Some((true, _)) if !bare => {}
            _ => {
                chosen.insert(model, (bare, entry));
            }
        }
    }

    let mut entries: Vec<PriceEntry> = chosen.into_values().map(|(_, entry)| entry).collect();
    entries.sort_by(|a, b| a.model.cmp(&b.model));
    Ok(PriceSnapshot {
        as_of: as_of.to_string(),
        source: SOURCE_NAME.to_string(),
        entries,
    })
}

/// 合并出「生效单价表」：用户单价始终优先，快照只补齐用户完全没有配置过的模型。
/// 只要用户为某模型配置了任意单价（精确或按模型），该模型就完全交给用户，不再引入快照兜底。
pub fn merge(user: &PriceTable, snapshot: &PriceSnapshot) -> PriceTable {
    let priced: HashSet<&str> = user.prices.iter().map(|p| p.model.as_str()).collect();
    let mut prices: Vec<PriceEntry> = user
        .prices
        .iter()
        .cloned()
        .map(|mut entry| {
            entry.origin = PriceOrigin::User;
            entry
        })
        .collect();
    for entry in &snapshot.entries {
        if !priced.contains(entry.model.as_str()) {
            let mut fallback = entry.clone();
            fallback.origin = PriceOrigin::Snapshot;
            prices.push(fallback);
        }
    }
    PriceTable { prices }
}

fn num(obj: &serde_json::Map<String, Value>, key: &str) -> Option<f64> {
    obj.get(key).and_then(|v| {
        v.as_f64()
            .or_else(|| v.as_i64().map(|n| n as f64))
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
    })
}
