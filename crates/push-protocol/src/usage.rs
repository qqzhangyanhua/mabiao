use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// 各口径 token。与桌面端 `UsageRecord` 的五个口径加合计一一对应。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageTokens {
    pub input: i64,
    pub output: i64,
    pub cache_read: i64,
    pub cache_creation: i64,
    pub reasoning: i64,
    pub total: i64,
}

/// 费用快照的定价来源。服务端按团队价目重算，这个字段只说明客户端当时怎么算的。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PricingSource {
    /// 来源自带 `native_cost`。
    Native,
    /// 价目表按 model 精确匹配。
    Exact,
    /// model 兜底或 LiteLLM 快照。
    Fallback,
    Unpriced,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageRecordPayload {
    /// 见 [`usage_fingerprint`]。服务端按（远程账号, 设备, 指纹）去重。
    pub fingerprint: String,
    pub occurred_at: String,
    pub source: String,
    pub model: String,
    pub provider: String,
    pub project: String,
    pub session_id: String,
    pub source_file: String,
    pub tokens: UsageTokens,
    pub native_cost: Option<f64>,
    /// 推送当时本机算出的费用；未定价时为空。
    pub cost_snapshot: Option<f64>,
    pub pricing_source: PricingSource,
}

const FINGERPRINT_TAG: &[u8] = b"mabiao.usage.fingerprint.v1";

fn put_str(hasher: &mut Sha256, value: &str) {
    hasher.update((value.len() as u64).to_le_bytes());
    hasher.update(value.as_bytes());
}

/// 消耗记录本机只有自增 id，没有稳定主键，客户端按内容算指纹给服务端去重。
///
/// 字符串字段带长度前缀、数字定宽，不同字段拼接不会撞出同一字节流。
/// `tokens.total` 必须是桌面端 `UsageRecord::with_total` 之后的值，否则两端会算出不同指纹。
/// 同一源文件同一时刻 model 与各口径 token 全相同的两条会得到同一指纹，服务端会当重复丢掉。
pub fn usage_fingerprint(
    source: &str,
    source_file: &str,
    occurred_at: &str,
    model: &str,
    tokens: &UsageTokens,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(FINGERPRINT_TAG);
    put_str(&mut hasher, source);
    put_str(&mut hasher, source_file);
    put_str(&mut hasher, occurred_at);
    put_str(&mut hasher, model);
    for n in [
        tokens.input,
        tokens.output,
        tokens.cache_read,
        tokens.cache_creation,
        tokens.reasoning,
        tokens.total,
    ] {
        hasher.update(n.to_le_bytes());
    }
    hasher
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut out, byte| {
            out.push_str(&format!("{byte:02x}"));
            out
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens() -> UsageTokens {
        UsageTokens {
            input: 100,
            output: 20,
            cache_read: 5,
            cache_creation: 3,
            reasoning: 7,
            total: 135,
        }
    }

    fn fp(source: &str, file: &str, at: &str, model: &str, t: &UsageTokens) -> String {
        usage_fingerprint(source, file, at, model, t)
    }

    #[test]
    fn fingerprint_is_deterministic_lowercase_sha256_hex() {
        let a = fp(
            "codex",
            "/a.jsonl",
            "2026-01-01T00:00:00Z",
            "gpt-5",
            &tokens(),
        );
        let b = fp(
            "codex",
            "/a.jsonl",
            "2026-01-01T00:00:00Z",
            "gpt-5",
            &tokens(),
        );
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
        assert!(a
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)));
    }

    #[test]
    fn fingerprint_is_pinned_so_client_and_server_cannot_drift() {
        let value = fp(
            "codex",
            "/a.jsonl",
            "2026-01-01T00:00:00Z",
            "gpt-5",
            &tokens(),
        );
        assert_eq!(
            value,
            "16dd737050410e368ff1eb42ddfb725e954f148971f7f0e55fe58babacc958ff"
        );
    }

    #[test]
    fn every_input_changes_the_fingerprint() {
        let base = fp("codex", "/a", "t", "m", &tokens());
        let mut seen = vec![base.clone()];
        let variants = [
            fp("claude", "/a", "t", "m", &tokens()),
            fp("codex", "/b", "t", "m", &tokens()),
            fp("codex", "/a", "u", "m", &tokens()),
            fp("codex", "/a", "t", "n", &tokens()),
            fp(
                "codex",
                "/a",
                "t",
                "m",
                &UsageTokens {
                    input: 101,
                    ..tokens()
                },
            ),
            fp(
                "codex",
                "/a",
                "t",
                "m",
                &UsageTokens {
                    output: 21,
                    ..tokens()
                },
            ),
            fp(
                "codex",
                "/a",
                "t",
                "m",
                &UsageTokens {
                    cache_read: 6,
                    ..tokens()
                },
            ),
            fp(
                "codex",
                "/a",
                "t",
                "m",
                &UsageTokens {
                    cache_creation: 4,
                    ..tokens()
                },
            ),
            fp(
                "codex",
                "/a",
                "t",
                "m",
                &UsageTokens {
                    reasoning: 8,
                    ..tokens()
                },
            ),
            fp(
                "codex",
                "/a",
                "t",
                "m",
                &UsageTokens {
                    total: 136,
                    ..tokens()
                },
            ),
        ];
        for variant in variants {
            assert!(!seen.contains(&variant), "指纹撞了");
            seen.push(variant);
        }
    }

    #[test]
    fn field_boundaries_cannot_be_shifted() {
        let t = tokens();
        assert_ne!(fp("ab", "c", "t", "m", &t), fp("a", "bc", "t", "m", &t));
        assert_ne!(fp("a", "b", "t", "m", &t), fp("a", "", "bt", "m", &t));
    }

    #[test]
    fn pricing_source_wire_names_are_snake_case() {
        let json = serde_json::to_string(&[
            PricingSource::Native,
            PricingSource::Exact,
            PricingSource::Fallback,
            PricingSource::Unpriced,
        ])
        .unwrap();
        assert_eq!(json, r#"["native","exact","fallback","unpriced"]"#);
    }
}
