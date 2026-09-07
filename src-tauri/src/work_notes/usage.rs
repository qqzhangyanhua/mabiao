use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EngineUsage {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub known: bool,
}

impl EngineUsage {
    pub fn add(&mut self, other: &EngineUsage) {
        if !other.known {
            return;
        }
        self.known = true;
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_read_tokens += other.cache_read_tokens;
        self.cache_creation_tokens += other.cache_creation_tokens;
    }
}

pub struct SplitOutput {
    pub payload: String,
    pub usage: EngineUsage,
}

pub fn split_output(raw: &str) -> SplitOutput {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return SplitOutput {
            payload: String::new(),
            usage: EngineUsage::default(),
        };
    }
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return split_value(&value, trimmed);
    }
    let mut usage = EngineUsage::default();
    let mut payload = String::new();
    for line in trimmed.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        take_usage(&value, &mut usage);
        if let Some(text) = agent_message_text(&value) {
            payload = text;
        } else if let Some(result) = result_text(&value) {
            payload = result;
        } else if let Some(text) = text_field(&value) {
            // grok 有时把结构化输出包在顶层 "text" 字段里（字符串值）
            payload = text;
        }
    }
    if payload.is_empty() {
        payload = trimmed.to_string();
    }
    SplitOutput { payload, usage }
}

fn split_value(value: &Value, raw: &str) -> SplitOutput {
    let mut usage = EngineUsage::default();
    take_usage(value, &mut usage);
    let payload = if looks_like_schema(value) {
        raw.to_string()
    } else if let Some(text) = agent_message_text(value) {
        text
    } else if let Some(result) = result_text(value) {
        result
    } else if let Some(text) = text_field(value) {
        // grok 有时把结构化输出包在顶层 "text" 字段里（字符串值）
        text
    } else {
        raw.to_string()
    };
    SplitOutput { payload, usage }
}

fn looks_like_schema(value: &Value) -> bool {
    value.get("summary").is_some() || value.get("headline").is_some()
}

fn result_text(value: &Value) -> Option<String> {
    match value.get("result") {
        Some(Value::String(text)) => Some(text.clone()),
        Some(object) if object.is_object() => Some(object.to_string()),
        _ => None,
    }
}

fn agent_message_text(value: &Value) -> Option<String> {
    let item = value.get("item")?;
    let kind = item.get("type")?.as_str()?;
    if kind != "agent_message" {
        return None;
    }
    item.get("text")?.as_str().map(str::to_string)
}

/// grok 有时把结构化输出包在顶层 `"text"` 字段里，值为字符串。
fn text_field(value: &Value) -> Option<String> {
    match value.get("text") {
        Some(Value::String(text)) if !text.trim().is_empty() => Some(text.clone()),
        _ => None,
    }
}

fn take_usage(value: &Value, usage: &mut EngineUsage) {
    let Some(node) = value.get("usage") else {
        return;
    };
    let input = int_field(node, &["input_tokens", "inputTokens"]);
    let output = int_field(node, &["output_tokens", "outputTokens"]);
    let cache_read = int_field(
        node,
        &[
            "cached_input_tokens",
            "cache_read_tokens",
            "cachedReadTokens",
            "cache_read_input_tokens",
        ],
    );
    let cache_creation = int_field(node, &["cache_creation_tokens", "cacheCreationTokens"]);
    if input == 0 && output == 0 && cache_read == 0 && cache_creation == 0 {
        return;
    }
    usage.known = true;
    usage.input_tokens += input;
    usage.output_tokens += output;
    usage.cache_read_tokens += cache_read;
    usage.cache_creation_tokens += cache_creation;
}

fn int_field(node: &Value, names: &[&str]) -> i64 {
    for name in names {
        if let Some(value) = node.get(*name) {
            if let Some(int) = value.as_i64() {
                return int;
            }
            if let Some(float) = value.as_f64() {
                return float as i64;
            }
        }
    }
    0
}
