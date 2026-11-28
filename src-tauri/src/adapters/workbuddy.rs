use std::path::{Path, PathBuf};

use crate::adapters::buddy_jsonl::{self, BuddyUsageProfile};
use crate::adapters::{parse_streaming_jsonl, LineFactory};
use crate::domain::{Source, UsageRecord};
use crate::ingest::PathOverrides;

/// WorkBuddy 会话：`$WORKBUDDY_CONFIG_DIR`，否则 `~/.workbuddy` 和 `~/.workbuddy-ai`，各扫 `projects/`。
/// 桌面端 `WorkBuddy AI.app` 把会话写在 `~/.workbuddy-ai`，而 `~/.workbuddy` 里只有日志。
const PROFILE: BuddyUsageProfile = BuddyUsageProfile {
    source: Source::WorkBuddy,
    path_env: "WORKBUDDY_CONFIG_DIR",
    default_roots: &[".workbuddy", ".workbuddy-ai"],
    skip_run_and_incomplete: false,
    dedupe_provider_keys: &["messageId"],
    dedupe_fallback_root_id: false,
    use_raw_and_message_usage: false,
    cache_read_direct_keys: &[],
    cache_write_keys: &["cacheCreationTokens", "cache_creation_input_tokens"],
    input_miss_keys: &[],
    raw_input_keys: &["inputTokens", "input_tokens"],
    output_keys: &["outputTokens", "output_tokens"],
    reasoning_keys: &[],
    model_fallback_message: false,
};

pub(crate) fn scan_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    buddy_jsonl::scan_dirs(overrides, home, &PROFILE)
}

pub(crate) fn sidecar_fingerprint(path: &Path, _dirs: &[PathBuf]) -> String {
    buddy_jsonl::sidecar_fingerprint(path)
}

pub(crate) fn parse(path: &Path, _scan_dir: &Path) -> Result<Vec<UsageRecord>, String> {
    parse_streaming_jsonl(path, parse_workbuddy_jsonl)
}

pub fn parse_workbuddy_jsonl(lines: &LineFactory<'_>, source_file: &str) -> Vec<UsageRecord> {
    buddy_jsonl::parse_jsonl(lines, source_file, &PROFILE)
}
