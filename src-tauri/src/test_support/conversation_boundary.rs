//! 对话记录模块边界门禁的纯检查函数。
//!
//! 只吃「文件名 + 源码文本」，不读盘。读盘与断言留在 `tests::conversation_boundary`。

use std::fmt;

/// 当前已生效的依赖方向规则。后续票可在此枚举上追加变体。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversationBoundaryRule {
    /// 对话记录目录下任何文件不得出现 `use super::*`。
    NoSuperWildcard,
}

impl ConversationBoundaryRule {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoSuperWildcard => "no_super_wildcard",
        }
    }
}

impl fmt::Display for ConversationBoundaryRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationBoundaryViolation {
    pub file: String,
    pub line: usize,
    pub rule: ConversationBoundaryRule,
    pub symbol: String,
}

impl fmt::Display for ConversationBoundaryViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}: {} ({})",
            self.file, self.line, self.rule, self.symbol
        )
    }
}

/// 检查对话记录目录内一组源文件是否违反已生效的边界规则。
pub fn inspect_conversation_boundary<N, S>(
    files: impl IntoIterator<Item = (N, S)>,
) -> Vec<ConversationBoundaryViolation>
where
    N: AsRef<str>,
    S: AsRef<str>,
{
    let mut violations = Vec::new();
    for (file, source) in files {
        inspect_file(file.as_ref(), source.as_ref(), &mut violations);
    }
    violations
}

fn inspect_file(file: &str, source: &str, violations: &mut Vec<ConversationBoundaryViolation>) {
    for (index, line) in source.lines().enumerate() {
        if let Some(symbol) = super_wildcard_symbol(line) {
            violations.push(ConversationBoundaryViolation {
                file: file.to_string(),
                line: index + 1,
                rule: ConversationBoundaryRule::NoSuperWildcard,
                symbol: symbol.to_string(),
            });
        }
    }
}

fn super_wildcard_symbol(line: &str) -> Option<&'static str> {
    let code = match line.find("//") {
        Some(idx) => &line[..idx],
        None => line,
    };
    let tokens: Vec<&str> = code.split_whitespace().collect();
    let use_at = tokens.iter().position(|token| *token == "use")?;
    if use_at > 0 && !is_visibility(tokens[0]) {
        return None;
    }
    match tokens.get(use_at + 1) {
        Some(token) if token.starts_with("super::*") => Some("use super::*"),
        _ => None,
    }
}

fn is_visibility(token: &str) -> bool {
    token == "pub" || token.starts_with("pub(")
}
