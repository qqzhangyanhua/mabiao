//! 推送前对正文与注入原文打码常见密钥（ADR 0026「脱敏」）。
//!
//! 只认固定格式，首版不开放自定义规则。绝对路径与项目名不打码：复盘需要它们。
//! 打码是兜底，没被规则命中的密钥仍会离开本机，预览里的那句提示就是告知这件事。

use std::sync::OnceLock;

use regex::{Captures, Regex};

pub const PLACEHOLDER: &str = "[REDACTED]";

struct Rules {
    private_key: Regex,
    /// 整串替换：这些格式本身就是密钥。
    whole_token: Vec<Regex>,
    bearer: Regex,
    /// `password=xxx`、`GITHUB_TOKEN=xxx`、`"api_key": "xxx"`：保留键名，只替换值。
    assignment: Regex,
}

fn rules() -> &'static Rules {
    static RULES: OnceLock<Rules> = OnceLock::new();
    RULES.get_or_init(|| Rules {
        private_key: compile(
            r"(?s)-----BEGIN [A-Z ]*PRIVATE KEY-----.*?(?:-----END [A-Z ]*PRIVATE KEY-----|\z)",
        ),
        whole_token: [
            // OpenAI / Anthropic 等：sk-、sk-ant-、sk-proj-
            r"\bsk-[A-Za-z0-9_\-]{16,}",
            r"\bgh[pousr]_[A-Za-z0-9]{30,}",
            r"\bgithub_pat_[A-Za-z0-9_]{20,}",
            r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b",
            r"\bxox[abprs]-[A-Za-z0-9\-]{10,}",
            r"\bAIza[0-9A-Za-z_\-]{35}\b",
            r"\beyJ[A-Za-z0-9_\-]{8,}\.eyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]+",
        ]
        .into_iter()
        .map(compile)
        .collect(),
        bearer: compile(r"(?i)\b(Bearer)\s+[A-Za-z0-9._~+/=\-]{16,}"),
        // 值的几种写法各占一个捕获组（3 转义引号、4 双引号、5 单引号、6 已是占位符、7 裸值），
        // 好让替换保留原来的引号。转义引号是为了处理嵌在 JSON 字符串里的 JSON。
        assignment: compile(
            r#"(?i)\b([A-Za-z0-9_\-]*(?:password|passwd|secret|api[_-]?key|access[_-]?token|auth[_-]?token|token))((?:\\?["'])?\s*[=:]\s*)(?:(\\")[^"\\\n]+\\"|(")[^"\n]+"|(')[^'\n]+'|(\[REDACTED\])|[^\s"',;&\)\]\}\\]+)"#,
        ),
    })
}

fn compile(pattern: &str) -> Regex {
    Regex::new(pattern).expect("内置打码规则必须是合法正则")
}

/// 返回打码后的文本与打码处数。
pub fn redact(text: &str) -> (String, u32) {
    let rules = rules();
    let mut count = 0u32;
    let mut current = text.to_string();

    current = replace_counted(&rules.private_key, &current, &mut count, |_| {
        PLACEHOLDER.to_string()
    });
    for rule in &rules.whole_token {
        current = replace_counted(rule, &current, &mut count, |_| PLACEHOLDER.to_string());
    }
    current = replace_counted(&rules.bearer, &current, &mut count, |caps| {
        format!("{} {PLACEHOLDER}", &caps[1])
    });
    current = replace_counted(&rules.assignment, &current, &mut count, |caps| {
        if caps.get(6).is_some() {
            return caps[0].to_string();
        }
        let quote = (3..=5)
            .find_map(|group| caps.get(group))
            .map_or("", |m| m.as_str());
        format!("{}{}{quote}{PLACEHOLDER}{quote}", &caps[1], &caps[2])
    });
    (current, count)
}

fn replace_counted(
    rule: &Regex,
    text: &str,
    count: &mut u32,
    replacement: impl Fn(&Captures<'_>) -> String,
) -> String {
    rule.replace_all(text, |caps: &Captures<'_>| {
        let replaced = replacement(caps);
        // 值本来就是占位符的赋值不算新的一处，否则二次打码会虚增数字。
        if replaced != caps[0] {
            *count += 1;
        }
        replaced
    })
    .into_owned()
}
