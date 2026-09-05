//! 对话记录模块边界门禁的纯检查函数。
//!
//! 只吃「文件名 + 源码文本」，不读盘。读盘与断言留在 `tests::conversation_boundary`。

use std::fmt;

/// 模块根允许定义的编排入口。显式写死，不靠命名前缀推断。
const ORCHESTRATION_ENTRY_FNS: &[&str] = &[
    "refresh_source_in_roots",
    "parse_conversation_file",
    "parse_conversation_files",
    "load_events",
    "prepare_events_read",
    "finish_prepared_events",
    "refresh_codex",
    "conversation_adapter",
    "raw_export_extension",
    "codex_index_for_bench",
    "codex_index_suffix_for_bench",
];

/// 当前已生效的依赖方向规则。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversationBoundaryRule {
    /// 对话记录目录下任何文件不得出现 `use super::*`。
    NoSuperWildcard,
    /// 模块根不得定义白名单之外的 `fn`。re-export 不受此规则约束。
    RootFnWhitelist,
}

impl ConversationBoundaryRule {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoSuperWildcard => "no_super_wildcard",
            Self::RootFnWhitelist => "root_fn_whitelist",
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
        if is_conversation_module_root(file) {
            if let Some(name) = defined_fn_name(line) {
                if !ORCHESTRATION_ENTRY_FNS.contains(&name) {
                    violations.push(ConversationBoundaryViolation {
                        file: file.to_string(),
                        line: index + 1,
                        rule: ConversationBoundaryRule::RootFnWhitelist,
                        symbol: name.to_string(),
                    });
                }
            }
        }
    }
}

fn is_conversation_module_root(file: &str) -> bool {
    file == "mod.rs"
}

fn strip_line_comment(line: &str) -> &str {
    match line.find("//") {
        Some(idx) => &line[..idx],
        None => line,
    }
}

fn super_wildcard_symbol(line: &str) -> Option<&'static str> {
    let code = strip_line_comment(line);
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

/// 从一行源码里取出「正在定义的 `fn` 名」。函数指针类型与 re-export 返回 `None`。
fn defined_fn_name(line: &str) -> Option<&str> {
    let mut rest = strip_line_comment(line).trim();
    if rest.is_empty() {
        return None;
    }
    if let Some(after_vis) = strip_visibility(rest) {
        rest = after_vis;
    }
    loop {
        if let Some(after) = strip_keyword(rest, "async") {
            rest = after;
            continue;
        }
        if let Some(after) = strip_keyword(rest, "const") {
            rest = after;
            continue;
        }
        if let Some(after) = strip_keyword(rest, "unsafe") {
            rest = after;
            continue;
        }
        if let Some(after) = strip_extern_abi(rest) {
            rest = after;
            continue;
        }
        break;
    }
    let after_fn = strip_keyword(rest, "fn")?;
    let name_end = after_fn
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .unwrap_or(after_fn.len());
    if name_end == 0 {
        return None;
    }
    let name = &after_fn[..name_end];
    let after_name = after_fn[name_end..].trim_start();
    if after_name.starts_with('(') || after_name.starts_with('<') {
        Some(name)
    } else {
        None
    }
}

fn strip_visibility(code: &str) -> Option<&str> {
    let rest = code.strip_prefix("pub")?;
    if let Some(after_paren) = rest.strip_prefix('(') {
        let close = after_paren.find(')')?;
        return Some(after_paren[close + 1..].trim_start());
    }
    if rest.starts_with(|c: char| c.is_whitespace()) {
        Some(rest.trim_start())
    } else {
        None
    }
}

fn strip_keyword<'a>(code: &'a str, keyword: &str) -> Option<&'a str> {
    let rest = code.strip_prefix(keyword)?;
    if rest.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    Some(rest.trim_start())
}

fn strip_extern_abi(code: &str) -> Option<&str> {
    let rest = strip_keyword(code, "extern")?;
    if rest.starts_with('"') {
        let after_quote = rest.get(1..)?;
        let close = after_quote.find('"')?;
        return Some(after_quote[close + 1..].trim_start());
    }
    Some(rest)
}
