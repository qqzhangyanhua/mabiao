//! 取数调度：谁去取、并发怎么跑、结果怎么落。
//!
//! 内置 9 家与自定义提供商在这里合成同一个线程池的输入，因此两侧共用
//! `QuotaTarget`（标识 + 展示名）这一个形状。内置那侧的枚举与穷尽匹配一行不动，
//! 只是多实现了这个 trait。

use std::path::Path;

use chrono::{DateTime, Utc};
use rusqlite::Connection;

use super::{
    antigravity, backoff, capture_path, claude, claude_usage, codex, codex_usage, copilot, cursor,
    custom, detect, devin, droid, grok, opencode,
};
use crate::domain::{OfficialQuotaProvider, OfficialQuotaWindow};
use crate::store;

pub type ProviderFetch = Result<(Vec<OfficialQuotaWindow>, String), String>;

/// 取数目标的两个共同点：额度缓存用的标识，和报错时给用户看的名字。
///
/// 内置与自定义共用取数线程池、共用 sqlite 里的额度表、共用退避状态，
/// 三者的键都是这个标识。
pub trait QuotaTarget: Send + Sync {
    fn quota_id(&self) -> &str;
    fn quota_display_name(&self) -> &str;
}

impl QuotaTarget for OfficialQuotaProvider {
    fn quota_id(&self) -> &str {
        self.as_str()
    }

    fn quota_display_name(&self) -> &str {
        self.display_name()
    }
}

impl QuotaTarget for custom::ResolvedProvider {
    fn quota_id(&self) -> &str {
        &self.config.id
    }

    fn quota_display_name(&self) -> &str {
        &self.config.name
    }
}

/// 只有标识、没有展示名时走这条——写缓存只需要标识。目前的用户是测试：
/// 它们按标识直接铺一行额度缓存，不必先造一个完整的取数目标。
#[cfg(test)]
impl QuotaTarget for String {
    fn quota_id(&self) -> &str {
        self
    }

    fn quota_display_name(&self) -> &str {
        self
    }
}

/// 一次刷新里的一个目标。两条通道在这里合流成一个线程池的输入，
/// 自定义提供商因此与内置各家**同一波**并发，而不是排在它们后面等。
#[derive(Debug)]
pub enum FetchTarget {
    Builtin(OfficialQuotaProvider),
    Custom(Box<custom::ResolvedProvider>),
}

impl QuotaTarget for FetchTarget {
    fn quota_id(&self) -> &str {
        match self {
            Self::Builtin(provider) => provider.quota_id(),
            Self::Custom(provider) => provider.quota_id(),
        }
    }

    fn quota_display_name(&self) -> &str {
        match self {
            Self::Builtin(provider) => provider.quota_display_name(),
            Self::Custom(provider) => provider.quota_display_name(),
        }
    }
}

pub fn parse_provider(value: &str) -> Result<OfficialQuotaProvider, String> {
    OfficialQuotaProvider::parse(value).ok_or_else(|| format!("未知的官方额度账号：{value}"))
}

/// 单条刷新的标识解析：先试内置枚举，认不出再回落到自定义通道。
/// 顺序不能反——内置那 9 个标识不带 `custom:` 前缀，自定义的一定带，两边不会互相吃掉。
///
/// 停用的自定义提供商解析不出目标：关掉就是「不请求它的接口」，
/// 手动刷新也不该是例外。
pub fn resolve_target(
    id: &str,
    custom: &[custom::ResolvedProvider],
) -> Result<FetchTarget, String> {
    if let Some(provider) = OfficialQuotaProvider::parse(id) {
        return Ok(FetchTarget::Builtin(provider));
    }
    let found = custom
        .iter()
        .find(|provider| provider.config.id == id)
        .ok_or_else(|| format!("未知的官方额度账号：{id}"))?;
    if !found.config.enabled {
        return Err(format!("「{}」已停用，先在设置页打开它", found.config.name));
    }
    Ok(FetchTarget::Custom(Box::new(found.clone())))
}

/// 真正会打网的自定义目标：启用、且有密钥。
///
/// 缺密钥的在 `load_dto` 里画成待办，不走进取数线程——那条路连「失败」都算不上。
pub fn custom_targets_for_fetch(custom: &[custom::ResolvedProvider]) -> Vec<FetchTarget> {
    custom
        .iter()
        .filter(|provider| provider.config.enabled && provider.secret.is_some())
        .cloned()
        .map(|provider| FetchTarget::Custom(Box::new(provider)))
        .collect()
}

/// 先打 ChatGPT 的用量接口（不依赖 CLI 装没装），读不到再拉起 `codex app-server`。
/// 两条都失败时报接口那条：多数人应该走的是它。
fn fetch_codex() -> ProviderFetch {
    match codex_usage::fetch_usage() {
        Ok(result) => Ok(result),
        Err(error) => codex::fetch_rate_limits().map_err(|app_server_error| {
            if app_server_error.contains("未找到 Codex CLI") {
                error
            } else {
                app_server_error
            }
        }),
    }
}

/// 先问官方用量接口（零配置），读不到再回落到 statusline 捕获文件——后者是老路径，
/// 装了 hook 的用户和走第三方代理的用户都还得靠它。两条都没有才报错，
/// 错误信息取自动接口那条，因为那是多数人应该走的路。
fn fetch_claude() -> ProviderFetch {
    match claude_usage::fetch_usage() {
        Ok(result) => Ok(result),
        Err(error) => claude::refresh_from_capture(&capture_path()).map_err(|capture_error| {
            if capture_error.contains("尚未捕获") {
                // 两条路都没有：多数是第三方代理用户，官方登录态是空的，
                // 提示里把 statusline 这条兜底也说出来，否则只能看到一句读不懂的报错。
                format!("{error}。若使用第三方代理，可在设置页写入 statusline hook 后重试")
            } else {
                capture_error
            }
        }),
    }
}

pub fn fetch_provider(provider: OfficialQuotaProvider) -> ProviderFetch {
    match provider {
        OfficialQuotaProvider::Claude => fetch_claude(),
        OfficialQuotaProvider::Codex => fetch_codex(),
        OfficialQuotaProvider::Cursor => cursor::fetch_usage_summary(),
        OfficialQuotaProvider::Grok => grok::fetch_rate_limits(),
        OfficialQuotaProvider::Droid => droid::fetch_rate_limits(),
        OfficialQuotaProvider::Antigravity => antigravity::fetch_rate_limits(),
        OfficialQuotaProvider::OpenCode => opencode::fetch_usage(),
        OfficialQuotaProvider::Copilot => copilot::fetch_usage(),
        OfficialQuotaProvider::Devin => devin::fetch_usage(),
    }
}

pub fn fetch_target(target: &FetchTarget) -> ProviderFetch {
    match target {
        FetchTarget::Builtin(provider) => fetch_provider(*provider),
        FetchTarget::Custom(provider) => custom::fetch(provider),
    }
}

/// 先取数再交给调用方加锁写入，避免在持锁期间打网络。
/// 各家并发取数：串行的话总耗时是求和，实测 5 家 7.2 秒，而单家超时上限是 12~20 秒，
/// 网络一差就能拖到分钟级——而这整段跑在一个阻塞线程里，托盘定时刷新也走这条路。
/// 并发之后总耗时变成取最大值。
pub fn fetch_all_targets(custom: &[custom::ResolvedProvider]) -> Vec<(FetchTarget, ProviderFetch)> {
    let now = Utc::now();
    let path = backoff::state_path();
    let mut state = backoff::load_state(&path);
    let mut targets: Vec<FetchTarget> = OfficialQuotaProvider::ALL
        .into_iter()
        .filter(|provider| detect::has_local_credentials(*provider))
        .map(FetchTarget::Builtin)
        .collect();
    targets.extend(custom_targets_for_fetch(custom));
    let targets = exclude_cooling(targets, &state, now);

    let results = fetch_in_parallel(targets, fetch_target);
    record_backoff(
        &mut state,
        results
            .iter()
            .map(|(target, result)| (target.quota_id(), result)),
        now,
        &path,
    );
    results
}

/// 并发跑各家、按传入顺序返回。取数函数作为参数传入，这样调度本身可以脱网测试。
///
/// 用作用域线程而不是引 async 运行时：每家之间没有共享状态，退避状态在开始前读、
/// 结束后写，不进线程。结果按传入顺序 join，保证输出稳定。
pub fn fetch_in_parallel<T, F>(targets: Vec<T>, fetch: F) -> Vec<(T, ProviderFetch)>
where
    T: QuotaTarget,
    F: Fn(&T) -> ProviderFetch + Sync,
{
    let results: Vec<ProviderFetch> = std::thread::scope(|scope| {
        let fetch = &fetch;
        let handles: Vec<_> = targets
            .iter()
            .map(|target| scope.spawn(move || fetch(target)))
            .collect();
        handles
            .into_iter()
            .zip(targets.iter())
            .map(|(handle, target)| {
                // 某一家 panic 不该带走整次刷新，其余结果照常写入。
                handle.join().unwrap_or_else(|_| {
                    Err(format!("{} 取数线程异常退出", target.quota_display_name()))
                })
            })
            .collect()
    });
    targets.into_iter().zip(results).collect()
}

/// [`fetch_target_throttled`] 的结果：冷却期短路和真正打了网络的失败要分开，
/// 调用方不该把「还要等 N 分钟」这句提示当成新的失败原因落库——那会冲掉上一次
/// 真实失败的诊断信息，而且每点一次刷新剩余时间都会变，存下去毫无意义。
pub enum ThrottledFetch {
    /// 仍在冷却，没有实际发请求；这句话只用于这次响应临时展示，不落库。
    Cooldown(String),
    /// 冷却已过，真正打了网络（不论成功失败）。
    Attempted(ProviderFetch),
}

/// 冷却中的目标这一轮不打网。整体刷新走这里：不把「还要等多久」写进每一行，
/// 上次结果原样留着。单条手动刷新走 `fetch_target_throttled`，要开口说话。
pub(crate) fn exclude_cooling<T: QuotaTarget>(
    targets: Vec<T>,
    state: &backoff::BackoffState,
    now: DateTime<Utc>,
) -> Vec<T> {
    targets
        .into_iter()
        .filter(|target| backoff::cooldown_remaining(state, target.quota_id(), now).is_none())
        .collect()
}

/// 单个目标的手动刷新。限流期间也拦——「多点几次」正是让限流恢复更慢的原因，
/// 但要明确告诉用户还要等多久，而不是让按钮看起来没反应。
pub fn fetch_target_throttled(target: &FetchTarget) -> ThrottledFetch {
    fetch_target_throttled_at(target, &backoff::state_path(), Utc::now())
}

/// 与 `fetch_target_throttled` 相同，状态文件路径和「现在」由调用方注入。
/// 单测用 tempfile，避免去读真实用户目录。
pub(crate) fn fetch_target_throttled_at(
    target: &FetchTarget,
    path: &Path,
    now: DateTime<Utc>,
) -> ThrottledFetch {
    let mut state = backoff::load_state(path);
    if let Some(message) =
        backoff::cooldown_message(&state, target.quota_id(), target.quota_display_name(), now)
    {
        return ThrottledFetch::Cooldown(message);
    }
    let result = fetch_target(target);
    record_backoff(&mut state, [(target.quota_id(), &result)], now, path);
    ThrottledFetch::Attempted(result)
}

/// 悬浮面板上的「强制刷新」用：跳过冷却检查，即便还在冷却期也真打一次网络。
/// 结果照样喂给 [`record_backoff`]——连续失败依然会拉长下次自动重试的等待。
pub fn fetch_target_forced(target: &FetchTarget) -> ProviderFetch {
    let now = Utc::now();
    let path = backoff::state_path();
    let mut state = backoff::load_state(&path);
    let result = fetch_target(target);
    record_backoff(&mut state, [(target.quota_id(), &result)], now, &path);
    result
}

/// 取数结果 → 退避状态。只认标识与结果，因此内置与自定义共用同一段逻辑。
///
/// `path` 由调用方注入：生产走应用数据目录，单测走 tempfile。
pub(crate) fn record_backoff<'a>(
    state: &mut backoff::BackoffState,
    results: impl IntoIterator<Item = (&'a str, &'a ProviderFetch)>,
    now: DateTime<Utc>,
    path: &Path,
) {
    let mut touched = false;
    for (id, result) in results {
        match result {
            Ok(_) => {
                touched = true;
                backoff::record_success(state, id);
            }
            // 没打网就失败的（没配密钥、预设没实现）不进退避：退避是为了别把对方
            // 打挂，而这些压根没碰到对方。记下去只会用「稍后重试」盖住真正的原因。
            // 只对自定义那侧认——这套判据读的是自定义通道自己的错误文案，
            // 内置 9 家的错误不该被同一句中文误伤。
            Err(error) if custom::is_custom_id(id) && custom::is_precheck_error(error) => {}
            Err(error) => {
                touched = true;
                backoff::record_failure(state, id, error, now);
            }
        }
    }
    // 一次都没动过状态就别去写那个文件。
    if !touched {
        return;
    }
    // 状态写不下去不该让刷新失败，最多是下次少歇一会儿。
    let _ = backoff::save_state(path, state);
}

/// 打开总览或手动刷新时尝试更新各路；取数在调用方锁外完成，写入彼此隔离。
pub fn apply_fetch_results<T: QuotaTarget>(
    conn: &Connection,
    results: impl IntoIterator<Item = (T, ProviderFetch)>,
) -> Result<(), String> {
    for (target, result) in results {
        match result {
            Ok((windows, captured_at)) => {
                store::upsert_official_quota(conn, target.quota_id(), &windows, &captured_at, None)?
            }
            Err(error) => store::set_official_quota_error(conn, target.quota_id(), &error)?,
        }
    }
    Ok(())
}
