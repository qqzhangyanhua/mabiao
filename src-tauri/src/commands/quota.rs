use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use tauri::{Emitter, Manager};

use crate::domain::{
    OfficialQuotaConfig, OfficialQuotaConfigPatch, OfficialQuotaDto, OfficialQuotaHookDto,
};
use crate::official_quota;
use crate::AppState;

fn live_quota_env(state: &AppState) -> official_quota::QuotaEnv {
    official_quota::QuotaEnv::live(state.quota_paths.clone())
}

fn emit_quota_updated(app: &tauri::AppHandle, dto: &OfficialQuotaDto) {
    let _ = app.emit(official_quota::EVENT_UPDATED, dto);
}

fn send_quota_alert(
    app: &tauri::AppHandle,
    _alert: &official_quota::notify::QuotaAlert,
    title: &str,
    body: &str,
) {
    official_quota::notify::send_notification(app, title, body);
}

/// 纯只读缓存快照：走读连接，在内存里叠加更新的本地 Claude statusline 捕获，
/// 不拿写锁、不改告警状态、不弹系统通知。
#[tauri::command]
pub async fn get_official_quota(app: tauri::AppHandle) -> Result<OfficialQuotaDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let env = live_quota_env(&state);
        let conn = state.lock_read()?;
        Ok(official_quota::snapshot(&conn, &env))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn refresh_official_quota(app: tauri::AppHandle) -> Result<OfficialQuotaDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let env = live_quota_env(&state);
        let on_alert = |alert: &official_quota::notify::QuotaAlert, title: &str, body: &str| {
            send_quota_alert(&app, alert, title, body);
        };
        let on_updated = |dto: &OfficialQuotaDto| emit_quota_updated(&app, dto);
        official_quota::refresh_and_notify(
            &*state,
            &env,
            official_quota::RefreshOptions {
                notify: Some(&on_alert),
                on_updated: Some(&on_updated),
                stream_progress: true,
            },
        )
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
        let state = app.state::<AppState>();
        let env = live_quota_env(&state);
        let on_alert = |alert: &official_quota::notify::QuotaAlert, title: &str, body: &str| {
            send_quota_alert(&app, alert, title, body);
        };
        let on_updated = |dto: &OfficialQuotaDto| emit_quota_updated(&app, dto);
        official_quota::refresh_target_throttled(
            &*state,
            &env,
            &provider,
            official_quota::RefreshOptions {
                notify: Some(&on_alert),
                on_updated: Some(&on_updated),
                stream_progress: false,
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 悬浮额度面板每行的强制刷新：跳过冷却检查，结果照样记入退避并广播快照。
#[tauri::command]
pub async fn refresh_official_quota_provider_force(
    app: tauri::AppHandle,
    provider: String,
) -> Result<OfficialQuotaDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let env = live_quota_env(&state);
        let on_alert = |alert: &official_quota::notify::QuotaAlert, title: &str, body: &str| {
            send_quota_alert(&app, alert, title, body);
        };
        let on_updated = |dto: &OfficialQuotaDto| emit_quota_updated(&app, dto);
        official_quota::refresh_target_forced(
            &*state,
            &env,
            &provider,
            official_quota::RefreshOptions {
                notify: Some(&on_alert),
                on_updated: Some(&on_updated),
                stream_progress: false,
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

fn refresh_saved_account(app: &tauri::AppHandle, id: &str) {
    let state = app.state::<AppState>();
    let env = live_quota_env(&state);
    let on_updated = |dto: &OfficialQuotaDto| emit_quota_updated(app, dto);
    official_quota::refresh_saved_account(&*state, &env, id, Some(&on_updated));
}

#[tauri::command]
pub fn get_official_quota_hook(state: tauri::State<AppState>) -> OfficialQuotaHookDto {
    let env = live_quota_env(&state);
    official_quota::hook::preview_in(&env, &official_quota::hook::hook_command())
}

#[tauri::command]
pub fn apply_official_quota_hook(
    state: tauri::State<AppState>,
) -> Result<OfficialQuotaHookDto, String> {
    let env = live_quota_env(&state);
    official_quota::hook::apply_in(&env, &official_quota::hook::hook_command())
}

#[tauri::command]
pub fn get_official_quota_config(state: tauri::State<AppState>) -> OfficialQuotaConfig {
    official_quota::load_config(&state.quota_paths.config)
}

#[tauri::command]
pub fn save_official_quota_config(
    state: tauri::State<AppState>,
    config: OfficialQuotaConfig,
) -> Result<(), String> {
    official_quota::save_config(&state.quota_paths.config, &config)
}

/// 原子化局部更新 `official_quota.json`（如 `hidden_providers`、`alerts_enabled`、告警档位），
/// 保存后广播最新快照并刷新托盘标题与悬浮面板。
#[tauri::command]
pub async fn patch_official_quota_config(
    app: tauri::AppHandle,
    patch: OfficialQuotaConfigPatch,
) -> Result<OfficialQuotaConfig, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let updated = official_quota::patch_config(&state.quota_paths.config, &patch)?;
        let env = live_quota_env(&state);
        if let Ok(conn) = state.lock_read() {
            let dto = official_quota::snapshot(&conn, &env);
            drop(conn);
            emit_quota_updated(&app, &dto);
        }
        let _ = crate::tray::refresh(&app);
        Ok(updated)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn list_custom_quota_providers(
    state: tauri::State<AppState>,
) -> official_quota::custom::panel::CustomQuotaPanelDto {
    official_quota::custom::panel::list(&state.quota_paths.custom)
}

#[tauri::command]
pub fn save_custom_quota_provider(
    state: tauri::State<AppState>,
    request: official_quota::custom::panel::SaveCustomQuotaProvider,
) -> Result<official_quota::custom::panel::SavedCustomQuotaDto, String> {
    let saved = official_quota::custom::panel::save(&state.quota_paths.custom, request)?;
    // 用户刚改过这一条（多半正是在轮换密钥或换域名来修上一轮的失败），
    // 旧的退避不该再拦着它。
    official_quota::backoff::clear(&state.quota_paths.backoff, &saved.saved_id);
    Ok(saved)
}

#[tauri::command]
pub fn delete_custom_quota_provider(
    state: tauri::State<AppState>,
    id: String,
) -> Result<official_quota::custom::panel::CustomQuotaPanelDto, String> {
    official_quota::custom::panel::delete(&state.quota_paths.custom, &id)
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
#[tauri::command]
pub async fn test_custom_quota_provider(
    app: tauri::AppHandle,
    request: official_quota::custom::panel::TestCustomQuotaProvider,
) -> Result<official_quota::custom::panel::CustomQuotaTestDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let paths = state.quota_paths.custom.clone();
        let env = live_quota_env(&state);
        let secret = official_quota::custom::panel::resolve_secret(&paths, &request)?;
        let access_key_id = if request.preset.needs_access_key_id() {
            Some(official_quota::custom::panel::resolve_access_key_id(
                &paths, &request,
            )?)
        } else {
            None
        };
        let snapshot = official_quota::custom::fetch_quota_in(
            &env,
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
    official_quota::accounts::list(&state.quota_paths.accounts)
}

#[tauri::command]
pub async fn import_local_quota_account(
    app: tauri::AppHandle,
    provider: String,
) -> Result<official_quota::accounts::SavedQuotaAccountDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let provider = official_quota::parse_provider(&provider)?;
        let state = app.state::<AppState>();
        let env = live_quota_env(&state);
        let saved = official_quota::accounts::import_default_in(&env, provider)?;
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
        let saved = official_quota::accounts::save_profile_dir(
            &state.quota_paths.accounts,
            request,
            &state.quota_paths.backoff,
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
        let saved = official_quota::accounts::save(
            &state.quota_paths.accounts,
            request,
            &state.quota_paths.backoff,
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
    official_quota::accounts::delete_account(&state.quota_paths.accounts, &id)
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
        let polled = official_quota::accounts::poll_copilot_device_login(
            &login,
            &copilot_device_agent(),
            &state.quota_paths.accounts,
            &state.quota_paths.backoff,
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
        let polled = official_quota::accounts::poll_codex_pkce_login(
            &login,
            &codex_pkce_agent(),
            &state.quota_paths.accounts,
            &state.quota_paths.backoff,
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
        let submitted = official_quota::accounts::submit_codex_pkce_callback(
            &login,
            &callback_url,
            &codex_pkce_agent(),
            &state.quota_paths.accounts,
            &state.quota_paths.backoff,
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
            | "回调链接无法识别，没有保存额度账号"
            | "回调链接里没有授权码，没有保存额度账号"
            | "回调链接里没有状态参数，没有保存额度账号"
            | "正在完成授权，请稍候"
    )
}
