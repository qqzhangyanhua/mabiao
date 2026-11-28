use std::path::{Path, PathBuf};

use crate::adapters::buddy_jsonl::{self, BuddyUsageProfile};
use crate::adapters::{parse_streaming_jsonl, LineFactory};
use crate::domain::{Source, UsageRecord};
use crate::ingest::PathOverrides;

/// CodeBuddy CLI 会话：`$CODEBUDDY_DIR`，否则 `~/.codebuddy`，扫 `projects/**/*.jsonl`。
/// 目录布局像 Claude Code，行格式是 OpenAI Responses 形态，用量在
/// `providerData.rawUsage` / `providerData.usage`（`prompt_tokens` 含 cache）。
const PROFILE: BuddyUsageProfile = BuddyUsageProfile {
    source: Source::CodeBuddy,
    path_env: "CODEBUDDY_DIR",
    default_roots: &[".codebuddy"],
    skip_run_and_incomplete: true,
    dedupe_provider_keys: &["messageId", "traceId"],
    dedupe_fallback_root_id: true,
    use_raw_and_message_usage: true,
    cache_read_direct_keys: &[
        "cache_read_input_tokens",
        "cacheReadInputTokens",
        "prompt_cache_hit_tokens",
        "cached_tokens",
        "cacheTokens",
    ],
    cache_write_keys: &[
        "cache_creation_input_tokens",
        "cacheCreationInputTokens",
        "prompt_cache_write_tokens",
        "cachedWriteTokens",
    ],
    input_miss_keys: &[
        "cachedMissTokens",
        "cacheMissTokens",
        "prompt_cache_miss_tokens",
    ],
    raw_input_keys: &["prompt_tokens", "inputTokens", "input_tokens"],
    output_keys: &["completion_tokens", "outputTokens", "output_tokens"],
    reasoning_keys: &[
        "completion_thinking_tokens",
        "completionThinkingTokens",
        "reasoningTokens",
    ],
    model_fallback_message: true,
};

pub(crate) fn scan_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    buddy_jsonl::scan_dirs(overrides, home, &PROFILE)
}

pub(crate) fn sidecar_fingerprint(path: &Path, _dirs: &[PathBuf]) -> String {
    buddy_jsonl::sidecar_fingerprint(path)
}

pub(crate) fn parse(path: &Path, _scan_dir: &Path) -> Result<Vec<UsageRecord>, String> {
    parse_streaming_jsonl(path, parse_codebuddy_jsonl)
}

pub fn parse_codebuddy_jsonl(lines: &LineFactory<'_>, source_file: &str) -> Vec<UsageRecord> {
    buddy_jsonl::parse_jsonl(lines, source_file, &PROFILE)
}
