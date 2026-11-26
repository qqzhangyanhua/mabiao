use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use crate::official_quota;
use tauri::{Emitter, Manager};

use crate::domain::{
    OfficialQuotaConfig, OfficialQuotaDto, OfficialQuotaFreshness, OfficialQuotaHookDto,
    OfficialQuotaRow,
};
use crate::official_quota::QuotaTarget;
use crate::AppState;

/// 整体刷新期间每落库一家就推一次全量快照，界面逐行更新，不等最慢的那家。
const EVENT_UPDATED: &str = "official-quota-updated";

/// 只读缓存，走读连接：摄取会长时间占着写锁，首页额度不该跟着排队。
fn official_quota_snapshot(app: &tauri::AppHandle) -> Result<OfficialQuotaDto, String> {
    let state = app.state::<AppState>();
    let config = official_quota::load_config(&state.official_quota_path);
    let custom = official_quota::custom::store::load_providers(&state.custom_quota_paths);
    let accounts = official_quota::accounts::load_resolved(&state.quota_account_paths);
    let logins = official_quota::accounts::probe_default_logins(&accounts);
    let dto = {
        let conn = state.lock_read()?;
        official_quota::load_dto_with_accounts(
            &conn,
            &config,
            &custom,
            &accounts,
            &logins,
            chrono::Utc::now(),
        )
    };
    official_quota::notify::check_and_notify_with_config(
        app,
        &dto,
        &config,
        &state.official_quota_notify_path,
    )?;
    Ok(dto)
}

#[tauri::command]
pub async fn get_official_quota(app: tauri::AppHandle) -> Result<OfficialQuotaDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        // 写锁被摄取占着就跳过：捕获文件同步晚一轮无妨，紧接着的整体刷新还会再做。
        if let Some(conn) = app.state::<AppState>().try_lock_write() {
            let _ = official_quota::sync_claude_capture(&conn);
        }
        official_quota_snapshot(&app)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn refresh_official_quota(app: tauri::AppHandle) -> Result<OfficialQuotaDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        // 回调会在各家取数线程里并发进入；串起来保证推出去的快照只会越来越新。
        let publishing = Mutex::new(());
        let persisted = Mutex::new(HashSet::<String>::new());
        let custom = load_custom_providers(&app);
        let (accounts, logins) = load_quota_accounts(&app);
        let results = official_quota::fetch_all_targets_with_accounts(
            &custom,
            &accounts,
            &logins,
            |target, result| {
                let _serial = publishing.lock().unwrap_or_else(PoisonError::into_inner);
                let id = target.quota_id().to_string();
                if let Ok(dto) =
                    persist_official_quota_fetches(&app, [(id.clone(), result.clone())])
                {
                    let _ = app.emit(EVENT_UPDATED, &dto);
                    persisted
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .insert(id);
                }
            },
        );
        let persisted = persisted
            .into_inner()
            .unwrap_or_else(PoisonError::into_inner);
        // 取数线程 panic 或回调里落库失败的那几家没写进去，这里补一次。
        let leftover: Vec<_> = results
            .into_iter()
            .filter(|(target, _)| !persisted.contains(target.quota_id()))
            .collect();
        {
            let state = app.state::<AppState>();
            let conn = state.lock_write()?;
            let _ = official_quota::sync_claude_capture(&conn);
            official_quota::apply_fetch_results(&conn, leftover)?;
        }
        official_quota_snapshot(&app)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn refresh_official_quota_provider(
    app: tauri::AppHandle,
    provider: String,
) -> Result<OfficialQuotaDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        // 先试内置枚举，认不出再回落到自定义通道——`custom:` 那些标识
        // 走的是后一条路，不该再撞上「未知的官方额度账号」。
        let custom = load_custom_providers(&app);
        let (accounts, logins) = load_quota_accounts(&app);
        let target =
            official_quota::resolve_target_with_accounts(&provider, &custom, &accounts, &logins)?;
        match official_quota::fetch_target_throttled(&target) {
            // 冷却期短路：不写库，避免把上一次真实失败原因换成这句「还要等 N 分
            // 钟」——只在这次响应的快照里临时替换该行 error，够按钮即时反馈用。
            official_quota::ThrottledFetch::Cooldown(message) => overlay_cooldown_message(
                &app,
                target.quota_id(),
                target.quota_display_name(),
                message,
            ),
            official_quota::ThrottledFetch::Attempted(result) => {
                persist_official_quota_fetches(&app, [(target, result)])
            }
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 悬浮额度面板每行的强制刷新：跳过冷却检查，用户点了就是要现在就试一次
/// （托盘弹窗那边空间小，不方便像主窗口那样先弹一句「还要等 N 分钟」再让人
/// 决定要不要硬刷）。结果照样记入退避状态，连续失败仍会拉长下次自动重试。
#[tauri::command]
pub async fn refresh_official_quota_provider_force(
    app: tauri::AppHandle,
    provider: String,
) -> Result<OfficialQuotaDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let custom = load_custom_providers(&app);
        let (accounts, logins) = load_quota_accounts(&app);
        let target =
            official_quota::resolve_target_with_accounts(&provider, &custom, &accounts, &logins)?;
        let result = official_quota::fetch_target_forced(&target);
        persist_official_quota_fetches(&app, [(target, result)])
    })
    .await
    .map_err(|e| e.to_string())?
}

fn load_custom_providers(app: &tauri::AppHandle) -> Vec<official_quota::custom::ResolvedProvider> {
    let state = app.state::<AppState>();
    official_quota::custom::store::load_providers(&state.custom_quota_paths)
}

fn load_quota_accounts(
    app: &tauri::AppHandle,
) -> (
    Vec<official_quota::accounts::ResolvedQuotaAccount>,
    Vec<official_quota::accounts::LocalLogin>,
) {
    let state = app.state::<AppState>();
    let accounts = official_quota::accounts::load_resolved(&state.quota_account_paths);
    let logins = official_quota::accounts::probe_default_logins(&accounts);
    (accounts, logins)
}

/// 保存或重新导入之后，启用的那一条立刻取一次。失败写进行里，不让保存本身失败。
fn refresh_saved_account(app: &tauri::AppHandle, id: &str) {
    let custom = load_custom_providers(app);
    let (accounts, logins) = load_quota_accounts(app);
    let Ok(target) = official_quota::resolve_target_with_accounts(id, &custom, &accounts, &logins)
    else {
        return;
    };
    let result = match official_quota::fetch_target_throttled(&target) {
        official_quota::ThrottledFetch::Cooldown(_) => return,
        official_quota::ThrottledFetch::Attempted(result) => result,
    };
    let state = app.state::<AppState>();
    let Ok(conn) = state.lock_write() else {
        return;
    };
    let _ = official_quota::apply_fetch_results(&conn, [(target, result)]);
}

fn persist_official_quota_fetches<T: official_quota::QuotaTarget>(
    app: &tauri::AppHandle,
    results: impl IntoIterator<Item = (T, official_quota::ProviderFetch)>,
) -> Result<OfficialQuotaDto, String> {
    {
        let state = app.state::<AppState>();
        let conn = state.lock_write()?;
        official_quota::apply_fetch_results(&conn, results)?;
    }
    // snapshot 自己再取锁；这里必须先放下，std::sync::Mutex 不可重入。
    official_quota_snapshot(app)
}

/// 冷却提示只挂在这次返回的快照上，不落库。
fn overlay_cooldown_message(
    app: &tauri::AppHandle,
    id: &str,
    display_name: &str,
    message: String,
) -> Result<OfficialQuotaDto, String> {
    let mut dto = official_quota_snapshot(app)?;
    match dto.rows.iter_mut().find(|row| row.provider == id) {
        Some(row) => row.error = Some(message),
        None => dto.rows.push(OfficialQuotaRow {
            provider: id.to_string(),
            application: display_name.to_string(),
            windows: Vec::new(),
            freshness: OfficialQuotaFreshness::Unavailable,
            captured_at: None,
            error: Some(message),
            todo: None,
            plan: None,
            base_provider: None,
            account_label: None,
            is_active_local: false,
        }),
    }
    Ok(dto)
}

#[tauri::command]
pub fn get_official_quota_hook() -> OfficialQuotaHookDto {
    official_quota::hook::preview(
        &official_quota::hook::default_settings_path(),
        &official_quota::hook::hook_command(),
    )
}

#[tauri::command]
pub fn apply_official_quota_hook() -> Result<OfficialQuotaHookDto, String> {
    official_quota::hook::apply(
        &official_quota::hook::default_settings_path(),
        &official_quota::hook::hook_command(),
    )
}

#[tauri::command]
pub fn get_official_quota_config(state: tauri::State<AppState>) -> OfficialQuotaConfig {
    official_quota::load_config(&state.official_quota_path)
}

#[tauri::command]
pub fn save_official_quota_config(
    state: tauri::State<AppState>,
    config: OfficialQuotaConfig,
) -> Result<(), String> {
    official_quota::save_config(&state.official_quota_path, &config)
}

#[tauri::command]
pub fn list_custom_quota_providers(
    state: tauri::State<AppState>,
) -> official_quota::custom::panel::CustomQuotaPanelDto {
    official_quota::custom::panel::list(&state.custom_quota_paths)
}

#[tauri::command]
pub fn save_custom_quota_provider(
    state: tauri::State<AppState>,
    request: official_quota::custom::panel::SaveCustomQuotaProvider,
) -> Result<official_quota::custom::panel::SavedCustomQuotaDto, String> {
    let saved = official_quota::custom::panel::save(&state.custom_quota_paths, request)?;
    // 用户刚改过这一条（多半正是在轮换密钥或换域名来修上一轮的失败），
    // 旧的退避不该再拦着它：否则保存后那次刷新只会回「刚取数失败，N 分钟后
    // 自动重试」，把刚做完的修复盖掉，用户会以为改了没用。
    official_quota::backoff::clear(&official_quota::backoff::state_path(), &saved.saved_id);
    Ok(saved)
}

#[tauri::command]
pub fn delete_custom_quota_provider(
    state: tauri::State<AppState>,
    id: String,
) -> Result<official_quota::custom::panel::CustomQuotaPanelDto, String> {
    official_quota::custom::panel::delete(&state.custom_quota_paths, &id)
}

/// base URL 输入框下方那行回显。纯计算、不打网，边打边问也不会有负担。
#[tauri::command]
pub fn preview_custom_quota_request(
    preset: official_quota::custom::CustomQuotaPreset,
    base_url: String,
) -> official_quota::custom::panel::CustomQuotaRequestPreviewDto {
    official_quota::custom::panel::preview_requests(
        preset,
        &base_url,
        chrono::Utc::now().date_naive(),
    )
}

/// 用表单里尚未保存的配置直接打一次，把解析出的额度交回去。
///
/// 走 `custom::fetch` 而不是 `fetch_target_throttled`：这是用户点出来的一次性验证，
/// 既不该被上一轮失败留下的冷却拦住（点测试往往正是为了修好它），也不该把失败
/// 记进退避——那条配置还没保存，退避里没有它的位置。同理不写额度缓存：
/// 首页那一行归已保存的配置管。
#[tauri::command]
pub async fn test_custom_quota_provider(
    app: tauri::AppHandle,
    request: official_quota::custom::panel::TestCustomQuotaProvider,
) -> Result<official_quota::custom::panel::CustomQuotaTestDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let paths = app.state::<AppState>().custom_quota_paths.clone();
        let secret = official_quota::custom::panel::resolve_secret(&paths, &request)?;
        let access_key_id = if request.preset.needs_access_key_id() {
            Some(official_quota::custom::panel::resolve_access_key_id(
                &paths, &request,
            )?)
        } else {
            None
        };
        let snapshot = official_quota::custom::fetch_quota(
            request.preset,
            &request.base_url,
            Some(&secret),
            access_key_id.as_deref(),
        )?;
        Ok(official_quota::custom::panel::CustomQuotaTestDto {
            windows: snapshot.windows,
            captured_at: snapshot.captured_at,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn list_quota_accounts(
    state: tauri::State<AppState>,
) -> official_quota::accounts::QuotaAccountPanelDto {
    official_quota::accounts::list(&state.quota_account_paths)
}

#[tauri::command]
pub async fn import_local_quota_account(
    app: tauri::AppHandle,
    provider: String,
) -> Result<official_quota::accounts::SavedQuotaAccountDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let provider = official_quota::parse_provider(&provider)?;
        let state = app.state::<AppState>();
        let paths = state.quota_account_paths.clone();
        let saved = official_quota::accounts::import_default(
            &paths,
            provider,
            &official_quota::backoff::state_path(),
        )?;
        if saved.enabled {
            refresh_saved_account(&app, &saved.saved_id);
        }
        Ok(saved)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn save_profile_dir_account(
    app: tauri::AppHandle,
    request: official_quota::accounts::SaveProfileDirAccount,
) -> Result<official_quota::accounts::SavedQuotaAccountDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let paths = state.quota_account_paths.clone();
        let saved = official_quota::accounts::save_profile_dir(
            &paths,
            request,
            &official_quota::backoff::state_path(),
        )?;
        if saved.enabled {
            refresh_saved_account(&app, &saved.saved_id);
        }
        Ok(saved)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn save_quota_account(
    app: tauri::AppHandle,
    request: official_quota::accounts::SaveQuotaAccount,
) -> Result<official_quota::accounts::SavedQuotaAccountDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let paths = state.quota_account_paths.clone();
        let saved = official_quota::accounts::save(
            &paths,
            request,
            &official_quota::backoff::state_path(),
        )?;
        if saved.enabled {
            refresh_saved_account(&app, &saved.saved_id);
        }
        Ok(saved)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn delete_quota_account(
    state: tauri::State<AppState>,
    id: String,
) -> Result<official_quota::accounts::QuotaAccountPanelDto, String> {
    official_quota::accounts::delete_account(&state.quota_account_paths, &id)
}

fn copilot_device_sessions(
) -> &'static Mutex<HashMap<String, official_quota::accounts::CopilotDeviceLogin>> {
    static SESSIONS: std::sync::LazyLock<
        Mutex<HashMap<String, official_quota::accounts::CopilotDeviceLogin>>,
    > = std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));
    &SESSIONS
}

fn device_session_id<T>(taken: &HashMap<String, T>) -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let bits = || {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u64(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_nanos() as u64),
        );
        hasher.finish()
    };
    for _ in 0..8 {
        let id = format!("{:016x}{:016x}", bits(), bits());
        if !taken.contains_key(&id) {
            return id;
        }
    }
    format!("{:016x}", bits())
}

fn copilot_device_agent() -> ureq::Agent {
    crate::net::agent_with_timeout(Duration::from_secs(20))
}

/// 申请一次性设备码。验证码和验证页给界面，`device_code` 留在内存里。
#[tauri::command]
pub async fn start_copilot_device_login(
) -> Result<official_quota::accounts::CopilotDeviceStartDto, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let login = official_quota::accounts::start_copilot_device_login(
            &copilot_device_agent(),
            &official_quota::accounts::CopilotDeviceEndpoints::github(),
        )?;
        let mut sessions = copilot_device_sessions()
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let session_id = device_session_id(&sessions);
        let dto = official_quota::accounts::CopilotDeviceStartDto {
            session_id: session_id.clone(),
            user_code: login.user_code.clone(),
            verification_url: login.verification_url.clone(),
            interval_seconds: login.interval_seconds(),
        };
        sessions.insert(session_id, login);
        Ok(dto)
    })
    .await
    .map_err(|error| error.to_string())?
}

/// 按服务端间隔轮询一次。成功后只刷新这一条 Copilot 额度账号。
#[tauri::command]
pub async fn poll_copilot_device_login(
    app: tauri::AppHandle,
    session_id: String,
) -> Result<official_quota::accounts::CopilotDevicePoll, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let login = {
            let sessions = copilot_device_sessions()
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            sessions
                .get(&session_id)
                .cloned()
                .ok_or_else(|| "这次设备码授权已经结束，请重新获取验证码".to_string())?
        };
        let state = app.state::<AppState>();
        let paths = state.quota_account_paths.clone();
        let polled = official_quota::accounts::poll_copilot_device_login(
            &login,
            &copilot_device_agent(),
            &paths,
            &official_quota::backoff::state_path(),
        );
        match polled {
            Ok(official_quota::accounts::CopilotDevicePoll::Pending { interval_seconds }) => {
                Ok(official_quota::accounts::CopilotDevicePoll::Pending { interval_seconds })
            }
            Ok(official_quota::accounts::CopilotDevicePoll::Authorized { saved }) => {
                copilot_device_sessions()
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .remove(&session_id);
                if saved.enabled {
                    refresh_saved_account(&app, &saved.saved_id);
                }
                Ok(official_quota::accounts::CopilotDevicePoll::Authorized { saved })
            }
            Err(error) => {
                copilot_device_sessions()
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .remove(&session_id);
                Err(error)
            }
        }
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub fn cancel_copilot_device_login(session_id: String) -> Result<(), String> {
    let mut sessions = copilot_device_sessions()
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if let Some(login) = sessions.remove(&session_id) {
        login.cancel();
    }
    Ok(())
}

#[tauri::command]
pub fn open_copilot_device_verification(session_id: String) -> Result<(), String> {
    let url = {
        let sessions = copilot_device_sessions()
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        sessions
            .get(&session_id)
            .map(|login| login.verification_url.clone())
            .ok_or_else(|| "这次设备码授权已经结束，请重新获取验证码".to_string())?
    };
    official_quota::accounts::open_copilot_verification(&url)
}

fn codex_pkce_sessions() -> &'static Mutex<HashMap<String, official_quota::accounts::CodexPkceLogin>>
{
    static SESSIONS: std::sync::LazyLock<
        Mutex<HashMap<String, official_quota::accounts::CodexPkceLogin>>,
    > = std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));
    &SESSIONS
}

fn codex_pkce_agent() -> ureq::Agent {
    crate::net::agent_with_timeout(Duration::from_secs(20))
}

fn codex_pkce_dto(
    session_id: String,
    login: &official_quota::accounts::CodexPkceLogin,
) -> official_quota::accounts::CodexPkceStartDto {
    official_quota::accounts::CodexPkceStartDto {
        session_id,
        authorize_url: login.authorize_url().to_string(),
        redirect_uri: login.redirect_uri().to_string(),
        listening: login.listening(),
        paste_hint: login.paste_hint().map(str::to_string),
    }
}

/// 生成 PKCE 并尽量在回环端口听回调。端口被占用时仍返回授权地址，界面改走粘贴。
#[tauri::command]
pub async fn start_codex_pkce_login() -> Result<official_quota::accounts::CodexPkceStartDto, String>
{
    tauri::async_runtime::spawn_blocking(|| {
        let login = official_quota::accounts::start_codex_pkce_login(
            &official_quota::accounts::CodexPkceEndpoints::openai(),
            official_quota::accounts::CODEX_PKCE_REDIRECT_PORT,
        )?;
        let mut sessions = codex_pkce_sessions()
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let session_id = device_session_id(&sessions);
        let dto = codex_pkce_dto(session_id.clone(), &login);
        sessions.insert(session_id, login);
        Ok(dto)
    })
    .await
    .map_err(|error| error.to_string())?
}

/// 回调到了就交换令牌并只刷新这一条 Codex 额度账号。还在等就返回 pending。
#[tauri::command]
pub async fn poll_codex_pkce_login(
    app: tauri::AppHandle,
    session_id: String,
) -> Result<official_quota::accounts::CodexPkcePoll, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let login = {
            let sessions = codex_pkce_sessions()
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            sessions
                .get(&session_id)
                .cloned()
                .ok_or_else(|| "这次浏览器授权已经结束，请重新发起".to_string())?
        };
        let state = app.state::<AppState>();
        let paths = state.quota_account_paths.clone();
        let polled = official_quota::accounts::poll_codex_pkce_login(
            &login,
            &codex_pkce_agent(),
            &paths,
            &official_quota::backoff::state_path(),
        );
        match polled {
            Ok(official_quota::accounts::CodexPkcePoll::Pending) => {
                Ok(official_quota::accounts::CodexPkcePoll::Pending)
            }
            Ok(official_quota::accounts::CodexPkcePoll::Authorized { saved }) => {
                codex_pkce_sessions()
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .remove(&session_id);
                if saved.enabled {
                    refresh_saved_account(&app, &saved.saved_id);
                }
                Ok(official_quota::accounts::CodexPkcePoll::Authorized { saved })
            }
            Err(error) => {
                if !codex_pkce_retryable(&error) {
                    codex_pkce_sessions()
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .remove(&session_id);
                }
                Err(error)
            }
        }
    })
    .await
    .map_err(|error| error.to_string())?
}

/// 浏览器没能打到回环端口时，用粘贴的回调链接做同一次状态校验和令牌交换。
#[tauri::command]
pub async fn submit_codex_pkce_callback(
    app: tauri::AppHandle,
    session_id: String,
    callback_url: String,
) -> Result<official_quota::accounts::SavedQuotaAccountDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let login = {
            let sessions = codex_pkce_sessions()
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            sessions
                .get(&session_id)
                .cloned()
                .ok_or_else(|| "这次浏览器授权已经结束，请重新发起".to_string())?
        };
        let state = app.state::<AppState>();
        let paths = state.quota_account_paths.clone();
        let submitted = official_quota::accounts::submit_codex_pkce_callback(
            &login,
            &callback_url,
            &codex_pkce_agent(),
            &paths,
            &official_quota::backoff::state_path(),
        );
        match submitted {
            Ok(saved) => {
                codex_pkce_sessions()
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .remove(&session_id);
                if saved.enabled {
                    refresh_saved_account(&app, &saved.saved_id);
                }
                Ok(saved)
            }
            Err(error) => {
                if !codex_pkce_retryable(&error) {
                    codex_pkce_sessions()
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .remove(&session_id);
                }
                Err(error)
            }
        }
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub fn cancel_codex_pkce_login(session_id: String) -> Result<(), String> {
    let mut sessions = codex_pkce_sessions()
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if let Some(login) = sessions.remove(&session_id) {
        login.cancel();
    }
    Ok(())
}

#[tauri::command]
pub fn open_codex_pkce_login(session_id: String) -> Result<(), String> {
    let url = {
        let sessions = codex_pkce_sessions()
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        sessions
            .get(&session_id)
            .map(|login| login.authorize_url().to_string())
            .ok_or_else(|| "这次浏览器授权已经结束，请重新发起".to_string())?
    };
    official_quota::accounts::open_codex_authorize(&url)
}

/// 链接本身不完整时留着会话，让用户再贴一次。状态对不上或交换失败就结束。
fn codex_pkce_retryable(error: &str) -> bool {
    matches!(
        error,
        "请粘贴浏览器地址栏里的回调链接"
            | "回调链接无法识别，没有保存账号"
            | "回调链接里没有授权码，没有保存账号"
            | "回调链接里没有状态参数，没有保存账号"
            | "正在完成授权，请稍候"
    )
}
