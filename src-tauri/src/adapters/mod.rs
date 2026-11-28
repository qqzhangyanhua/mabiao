pub mod agy;
pub mod aider;
pub mod alma;
pub mod amp;
pub mod cherry;
pub mod claude;
pub mod cline;
pub mod codebuddy;
pub mod codebuddy_ide;
pub mod codex;
pub mod copilot;
pub mod crush;
pub mod cursor;
pub mod cursor_account;
pub mod cursor_agent;
pub mod cursor_session;
pub mod dsh;
pub(crate) mod extension_tasks;
pub mod factory;
pub mod gemini;
pub mod goose;
pub mod grok;
pub mod hermes;
pub mod kilo;
pub mod kimi;
pub mod omp;
pub mod openclaw;
pub mod opencode;
pub mod pi;
pub mod project;
pub mod qoder;
pub mod qoder_cn;
pub mod qwen;
pub mod roo;
pub(crate) mod sqlite;
pub mod trae;
pub mod warp;
pub mod windsurf;
pub mod workbuddy;
pub mod zcode;
pub mod zed;

use std::path::{Path, PathBuf};

use crate::domain::{Source, UsageRecord};

type UsageDiscoverFn = fn(&[PathBuf]) -> Result<Vec<PathBuf>, String>;
type UsageSidecarFn = fn(&Path, &[PathBuf]) -> String;
type UsagePrepareDirFn = fn(&Path) -> Result<(), (PathBuf, String)>;
type UsagePrepareFileFn = fn(&Path) -> Result<(), (PathBuf, String)>;

/// 把一个已发现的文件解析成消耗记录。
///
/// 契约是「路径 + 所属扫描目录 → 记录」；「怎么读」留在适配器内部。
/// jsonl 家族（Codex 等单文件可达 114MB）按行打开磁盘，不要先 `fs::read`
/// 再解析——这个签名故意不收 `&[u8]` / `&str`，以免把它们逼成整份读入。
pub(crate) type UsageParseFn = fn(&Path, &Path) -> Result<Vec<UsageRecord>, String>;

pub(crate) struct UsageAdapter {
    pub source: Source,
    pub discover: UsageDiscoverFn,
    pub sidecar_fingerprint: UsageSidecarFn,
    pub parse: UsageParseFn,
    /// 扫描目录级派生上下文。失败时记来源级失败并跳过该目录，而不是让整个来源返回 Err。
    /// `Err` 的路径写入诊断，由适配器自己决定（例如辅助文件而不是扫描根）。
    pub prepare_dir: Option<UsagePrepareDirFn>,
    /// 单个已发现文件的派生上下文。失败时记来源级失败并跳过该文件，不进入缓存命中/解析。
    /// 用于会话级辅助文件（例如 Grok 的 `summary.json`），不要用 `prepare_dir` 代替。
    pub prepare_file: Option<UsagePrepareFileFn>,
    pub append_log: bool,
    /// 解析 `Err` 记诊断、保留上次正确缓存，但不抬 `files_failed`，归档对账继续。
    /// agy 单库中途失败走这条缝；Kimi / Grok 侧车失败仍走硬失败，避免打断既有对账推迟语义。
    pub soft_parse_failure: bool,
    pub coverage: &'static str,
    /// 「已检测到」：摄取报告与来源诊断共用。缺省为任一扫描目录存在。
    pub detected: Option<fn(&[PathBuf]) -> bool>,
}

impl UsageAdapter {
    pub(crate) fn roots_detected(&self, dirs: &[PathBuf]) -> bool {
        self.detected.map_or_else(
            || dirs.iter().any(|root| root.exists()),
            |detected| detected(dirs),
        )
    }
}

const USAGE_ADAPTERS: &[UsageAdapter] = &[
    UsageAdapter {
        source: Source::Codex,
        discover: codex::discover,
        sidecar_fingerprint: empty_sidecar,
        parse: codex::parse,
        prepare_dir: None,
        prepare_file: None,
        append_log: true,
        soft_parse_failure: false,
        coverage: "轮级 Token",
        detected: None,
    },
    UsageAdapter {
        source: Source::Claude,
        discover: discover_jsonl,
        sidecar_fingerprint: empty_sidecar,
        parse: claude::parse,
        prepare_dir: None,
        prepare_file: None,
        append_log: true,
        soft_parse_failure: false,
        coverage: "轮级 Token",
        detected: None,
    },
    UsageAdapter {
        source: Source::Pi,
        discover: discover_jsonl,
        sidecar_fingerprint: empty_sidecar,
        parse: pi::parse,
        prepare_dir: None,
        prepare_file: None,
        append_log: true,
        soft_parse_failure: false,
        coverage: "轮级 Token",
        detected: None,
    },
    UsageAdapter {
        source: Source::Omp,
        discover: discover_jsonl,
        sidecar_fingerprint: omp::sidecar_fingerprint,
        parse: omp::parse,
        prepare_dir: None,
        prepare_file: None,
        append_log: true,
        soft_parse_failure: false,
        coverage: "轮级 Token",
        detected: None,
    },
    UsageAdapter {
        source: Source::Kimi,
        discover: kimi::discover,
        sidecar_fingerprint: kimi::sidecar_fingerprint,
        parse: kimi::parse,
        prepare_dir: Some(kimi::prepare_dir),
        prepare_file: None,
        append_log: true,
        soft_parse_failure: false,
        coverage: "轮级 Token",
        detected: Some(kimi::detected),
    },
    UsageAdapter {
        source: Source::CursorAgent,
        discover: discover_jsonl,
        sidecar_fingerprint: empty_sidecar,
        parse: cursor_agent::parse,
        prepare_dir: None,
        prepare_file: None,
        append_log: true,
        soft_parse_failure: false,
        coverage: "会话与 IDE 共用本机目录；token 仅包装落盘",
        detected: None,
    },
    UsageAdapter {
        source: Source::Copilot,
        discover: discover_jsonl,
        sidecar_fingerprint: empty_sidecar,
        parse: copilot::parse,
        prepare_dir: None,
        prepare_file: None,
        append_log: true,
        soft_parse_failure: false,
        coverage: "仅会话结束时上报（累计）",
        detected: None,
    },
    UsageAdapter {
        source: Source::Dsh,
        discover: dsh::discover,
        sidecar_fingerprint: dsh::sidecar_fingerprint,
        parse: dsh::parse,
        prepare_dir: None,
        prepare_file: None,
        // zstd 会话整份重写，不是追加型日志；记录数下降不能当截断。
        append_log: false,
        soft_parse_failure: false,
        coverage: "轮级 Token",
        detected: None,
    },
    UsageAdapter {
        source: Source::Gemini,
        discover: gemini::discover,
        sidecar_fingerprint: empty_sidecar,
        parse: gemini::parse,
        prepare_dir: None,
        prepare_file: None,
        append_log: false,
        soft_parse_failure: false,
        coverage: "轮级 Token",
        detected: None,
    },
    UsageAdapter {
        source: Source::Qwen,
        discover: qwen::discover,
        sidecar_fingerprint: empty_sidecar,
        parse: qwen::parse,
        prepare_dir: None,
        prepare_file: None,
        append_log: false,
        soft_parse_failure: false,
        coverage: "轮级 Token",
        detected: None,
    },
    UsageAdapter {
        source: Source::Factory,
        discover: factory::discover,
        sidecar_fingerprint: factory::sidecar_fingerprint,
        parse: factory::parse,
        prepare_dir: None,
        prepare_file: None,
        append_log: false,
        soft_parse_failure: false,
        coverage: "会话累计 Token（jsonl 有 usage 则轮级）",
        detected: None,
    },
    UsageAdapter {
        source: Source::Grok,
        discover: grok::discover,
        sidecar_fingerprint: grok::sidecar_fingerprint,
        parse: grok::parse,
        prepare_dir: None,
        prepare_file: Some(grok::prepare_file),
        append_log: false,
        soft_parse_failure: false,
        coverage: "轮级 Token",
        detected: None,
    },
    UsageAdapter {
        source: Source::Opencode,
        discover: opencode::discover,
        sidecar_fingerprint: opencode::sidecar_fingerprint,
        parse: opencode::parse,
        prepare_dir: None,
        prepare_file: None,
        append_log: false,
        soft_parse_failure: false,
        coverage: "轮级 Token",
        detected: None,
    },
    UsageAdapter {
        source: Source::Hermes,
        discover: hermes::discover,
        sidecar_fingerprint: hermes::sidecar_fingerprint,
        parse: hermes::parse,
        prepare_dir: None,
        prepare_file: None,
        append_log: false,
        soft_parse_failure: false,
        coverage: "模型级 Token（含原生费用）",
        detected: None,
    },
    UsageAdapter {
        source: Source::Agy,
        discover: agy::discover,
        sidecar_fingerprint: agy::sidecar_fingerprint,
        parse: agy::parse,
        prepare_dir: None,
        prepare_file: None,
        append_log: false,
        soft_parse_failure: true,
        coverage: "轮级六元组（无原生费用）",
        detected: Some(agy::detected),
    },
    UsageAdapter {
        source: Source::Qoder,
        discover: discover_jsonl,
        sidecar_fingerprint: empty_sidecar,
        parse: qoder::parse,
        prepare_dir: None,
        prepare_file: None,
        append_log: true,
        soft_parse_failure: false,
        coverage: "轮级 Token",
        detected: None,
    },
    UsageAdapter {
        source: Source::QoderCn,
        discover: discover_jsonl,
        sidecar_fingerprint: empty_sidecar,
        parse: qoder_cn::parse,
        prepare_dir: None,
        prepare_file: None,
        append_log: true,
        soft_parse_failure: false,
        coverage: "轮级 Token",
        detected: None,
    },
    UsageAdapter {
        source: Source::Cline,
        discover: cline::discover,
        sidecar_fingerprint: cline::sidecar_fingerprint,
        parse: cline::parse,
        prepare_dir: None,
        prepare_file: None,
        append_log: false,
        soft_parse_failure: false,
        coverage: "轮级 Token（input 不含 cache）",
        detected: None,
    },
    UsageAdapter {
        source: Source::WorkBuddy,
        discover: discover_jsonl,
        sidecar_fingerprint: workbuddy::sidecar_fingerprint,
        parse: workbuddy::parse,
        prepare_dir: None,
        prepare_file: None,
        append_log: true,
        soft_parse_failure: false,
        coverage: "轮级 Token（input 不含 cache）",
        detected: None,
    },
    UsageAdapter {
        source: Source::Zcode,
        discover: zcode::discover,
        sidecar_fingerprint: zcode::sidecar_fingerprint,
        parse: zcode::parse,
        prepare_dir: None,
        prepare_file: None,
        append_log: false,
        soft_parse_failure: false,
        coverage: "轮级 Token（input 不含 cache）",
        detected: None,
    },
    UsageAdapter {
        source: Source::Alma,
        discover: alma::discover,
        sidecar_fingerprint: alma::sidecar_fingerprint,
        parse: alma::parse,
        prepare_dir: None,
        prepare_file: None,
        append_log: false,
        soft_parse_failure: false,
        coverage: "轮级 Token（input 不含 cache）",
        detected: None,
    },
    UsageAdapter {
        source: Source::CodeBuddy,
        discover: discover_jsonl,
        sidecar_fingerprint: codebuddy::sidecar_fingerprint,
        parse: codebuddy::parse,
        prepare_dir: None,
        prepare_file: None,
        append_log: true,
        soft_parse_failure: false,
        coverage: "轮级 Token（input 不含 cache）",
        detected: None,
    },
    UsageAdapter {
        source: Source::CodeBuddyIde,
        discover: codebuddy_ide::discover,
        sidecar_fingerprint: codebuddy_ide::sidecar_fingerprint,
        parse: codebuddy_ide::parse,
        prepare_dir: None,
        prepare_file: None,
        append_log: false,
        soft_parse_failure: false,
        coverage: "会话级 Token（input 不含 cache）",
        detected: None,
    },
    UsageAdapter {
        source: Source::Trae,
        discover: trae::discover,
        sidecar_fingerprint: empty_sidecar,
        parse: trae::parse,
        prepare_dir: None,
        prepare_file: None,
        append_log: true,
        soft_parse_failure: false,
        coverage: "轮级 Token",
        detected: None,
    },
    UsageAdapter {
        source: Source::Goose,
        discover: goose::discover,
        sidecar_fingerprint: goose::sidecar_fingerprint,
        parse: goose::parse,
        prepare_dir: None,
        prepare_file: None,
        append_log: false,
        soft_parse_failure: false,
        coverage: "会话级 Token",
        detected: None,
    },
    UsageAdapter {
        source: Source::Roo,
        discover: roo::discover,
        sidecar_fingerprint: roo::sidecar_fingerprint,
        parse: roo::parse,
        prepare_dir: None,
        prepare_file: None,
        append_log: false,
        soft_parse_failure: false,
        coverage: "轮级 Token（input 不含 cache）",
        detected: None,
    },
    UsageAdapter {
        source: Source::Kilo,
        discover: kilo::discover,
        sidecar_fingerprint: kilo::sidecar_fingerprint,
        parse: kilo::parse,
        prepare_dir: None,
        prepare_file: None,
        append_log: false,
        soft_parse_failure: false,
        coverage: "轮级 Token（input 不含 cache）",
        detected: None,
    },
    UsageAdapter {
        source: Source::Amp,
        discover: amp::discover,
        sidecar_fingerprint: empty_sidecar,
        parse: amp::parse,
        prepare_dir: None,
        prepare_file: None,
        append_log: false,
        soft_parse_failure: false,
        coverage: "轮级 Token（input 不含 cache）",
        detected: None,
    },
    UsageAdapter {
        source: Source::Zed,
        discover: zed::discover,
        sidecar_fingerprint: zed::sidecar_fingerprint,
        parse: zed::parse,
        prepare_dir: None,
        prepare_file: None,
        append_log: false,
        soft_parse_failure: false,
        coverage: "轮级 Token（input 不含 cache；有消息时间用消息时间，否则会话 updated_at；子线程不入账）",
        detected: None,
    },
    UsageAdapter {
        source: Source::Cherry,
        discover: cherry::discover,
        sidecar_fingerprint: cherry::sidecar_fingerprint,
        parse: cherry::parse,
        prepare_dir: None,
        prepare_file: None,
        append_log: false,
        soft_parse_failure: false,
        coverage: "轮级 Token（input 不含 cache）",
        detected: None,
    },
    UsageAdapter {
        source: Source::OpenClaw,
        discover: openclaw::discover,
        sidecar_fingerprint: openclaw::sidecar_fingerprint,
        parse: openclaw::parse,
        prepare_dir: None,
        prepare_file: None,
        append_log: true,
        soft_parse_failure: false,
        coverage: "轮级 Token（input 不含 cache）",
        detected: None,
    },
    UsageAdapter {
        source: Source::Crush,
        discover: crush::discover,
        sidecar_fingerprint: crush::sidecar_fingerprint,
        parse: crush::parse,
        prepare_dir: None,
        prepare_file: None,
        append_log: false,
        soft_parse_failure: false,
        coverage: "会话级 Token（含原生费用）",
        detected: None,
    },
    UsageAdapter {
        source: Source::Aider,
        discover: aider::discover,
        sidecar_fingerprint: empty_sidecar,
        parse: aider::parse,
        prepare_dir: None,
        prepare_file: None,
        append_log: true,
        soft_parse_failure: false,
        coverage: "轮级 Token（含原生费用）",
        detected: Some(aider::detected),
    },
    UsageAdapter {
        source: Source::Warp,
        discover: warp::discover,
        sidecar_fingerprint: warp::sidecar_fingerprint,
        parse: warp::parse,
        prepare_dir: None,
        prepare_file: None,
        append_log: false,
        soft_parse_failure: false,
        coverage: "会话级 Token（无 input/output 拆分）",
        detected: None,
    },
    UsageAdapter {
        source: Source::Windsurf,
        discover: windsurf::discover,
        sidecar_fingerprint: empty_sidecar,
        parse: windsurf::parse,
        prepare_dir: None,
        prepare_file: None,
        append_log: true,
        soft_parse_failure: false,
        coverage: "会话级 Token（ACP usage_update）",
        detected: None,
    },
];

pub(crate) fn usage_adapter(source: Source) -> &'static UsageAdapter {
    USAGE_ADAPTERS
        .iter()
        .find(|adapter| adapter.source == source)
        .expect("UsageAdapter 表必须覆盖 Source::ALL 的每个变体")
}

#[cfg(test)]
pub(crate) fn usage_adapters() -> &'static [UsageAdapter] {
    USAGE_ADAPTERS
}

/// 递归收集扫描目录下的 jsonl。心跳枚举与摄取共用。
pub(crate) fn discover_jsonl(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    for root in roots {
        paths.extend(crate::ingest::walk_files(root, "jsonl")?);
    }
    Ok(paths)
}

/// 按文件名后缀收集。dsh 的 `session.jsonl.zstd` 与 Factory 的 `.settings.json` 共用。
pub(crate) fn discover_suffix(roots: &[PathBuf], suffix: &str) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    for root in roots {
        paths.extend(crate::ingest::walk_suffix(root, suffix)?);
    }
    Ok(paths)
}

/// 整份读入 JSON：先校验语法，再交给适配器解析。Gemini / Qwen / Factory 共用。
pub(crate) fn parse_whole_json(
    path: &Path,
    parse: fn(&str, &str) -> Vec<UsageRecord>,
) -> Result<Vec<UsageRecord>, String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    let content = std::str::from_utf8(&bytes).map_err(|error| error.to_string())?;
    serde_json::from_str::<serde::de::IgnoredAny>(content).map_err(|error| error.to_string())?;
    Ok(parse(content, path.to_string_lossy().as_ref()))
}

pub(crate) fn empty_sidecar(_path: &Path, _dirs: &[PathBuf]) -> String {
    String::new()
}

/// 按行流式读取 jsonl：先校验语法，再交给适配器解析。整份文件从不进内存。
///
/// 会话 jsonl 单文件可以到上百 MB（真实观测到 114MB 的 Codex rollout 日志），
/// 启动时全量摄取和对话事件索引两条路径又可能同时处理同一份大文件。
/// 这里只保留几十 KB 的行缓冲区，不 `fs::read`。
pub(crate) fn parse_streaming_jsonl(
    path: &Path,
    parse: fn(&LineFactory<'_>, &str) -> Vec<UsageRecord>,
) -> Result<Vec<UsageRecord>, String> {
    crate::ingest::validate_jsonl_file(path)?;
    let loc = path.to_string_lossy();
    let factory: &LineFactory<'_> = &|| crate::ingest::open_jsonl_lines(path);
    Ok(parse(factory, loc.as_ref()))
}

/// 惰性逐行产出 `Value`，同一时刻只有一行的解析结果活着。
///
/// 会话 jsonl 单文件可以到几十 MB，`Value` 的堆表示又是原文的数倍；
/// 一次性 collect 成 `Vec` 会让整轮摄取的常驻内存和最大文件成正比。
/// 需要多趟扫描的适配器请重复调用本函数，重复解析比把整份文件留在内存里便宜。
pub fn parse_jsonl_values(content: &str) -> impl Iterator<Item = serde_json::Value> + '_ {
    content.lines().filter_map(|line| {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }
        serde_json::from_str(line).ok()
    })
}

/// 按行来源（磁盘流式读取或内存字符串）取一次性的行迭代器，用于产出方
/// 无需先把整份文件读进内存就能拿到 `LineFactory`。
///
/// 单个源文件可以到上百 MB（真实观测到 Codex 的 rollout 日志有 114MB），
/// 摄取阶段没必要为了解析而把这么大一份内容常驻内存——尤其是启动时全量摄取
/// 和对话事件索引两条路径可能同时处理同一份文件，峰值内存会翻倍。
/// 多趟扫描的适配器直接多次调用 `lines()` 重新流式读取，磁盘/OS page cache
/// 通常已经缓存了刚读过的文件，重复读取的代价远小于把整份文件留在内存里。
pub type LineFactory<'a> = dyn Fn() -> Box<dyn Iterator<Item = String> + 'a> + 'a;

/// 与 `parse_jsonl_values` 完全相同的惰性单行语义，只是输入换成任意行来源。
pub fn parse_jsonl_value_lines<'a>(
    lines: Box<dyn Iterator<Item = String> + 'a>,
) -> impl Iterator<Item = serde_json::Value> + 'a {
    lines.filter_map(|line| {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }
        serde_json::from_str(line).ok()
    })
}

pub fn i64_field(value: &serde_json::Value, keys: &[&str]) -> i64 {
    for key in keys {
        if let Some(n) = value.get(key).and_then(|v| {
            v.as_i64()
                .or_else(|| v.as_u64().map(|n| n as i64))
                .or_else(|| v.as_f64().map(|n| n.round() as i64))
        }) {
            return n;
        }
    }
    0
}

pub fn text_field(value: &serde_json::Value, keys: &[&str]) -> String {
    for key in keys {
        if let Some(s) = value.get(*key).and_then(|v| v.as_str()) {
            if !s.is_empty() {
                return s.to_string();
            }
        }
    }
    String::new()
}

pub fn finish(record: UsageRecord) -> UsageRecord {
    record.with_total()
}

pub fn has_billable_tokens(record: &UsageRecord) -> bool {
    record.input_tokens > 0
        || record.output_tokens > 0
        || record.cache_read_tokens > 0
        || record.cache_creation_tokens > 0
        || record.reasoning_tokens > 0
}
