use crate::official_quota;
use tauri::Manager;

use crate::domain::{
    OfficialQuotaConfig, OfficialQuotaDto, OfficialQuotaFreshness, OfficialQuotaHookDto,
    OfficialQuotaRow,
};
use crate::official_quota::QuotaTarget;
use crate::AppState;

fn official_quota_snapshot(app: &tauri::AppHandle) -> Result<OfficialQuotaDto, String> {
    let state = app.state::<AppState>();
    let conn = state.lock_write()?;
    let config = official_quota::load_config(&state.official_quota_path);
    let custom = official_quota::custom::store::load_providers(&state.custom_quota_paths);
    let dto = official_quota::load_dto(&conn, &config, &custom, chrono::Utc::now());
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
        {
            let state = app.state::<AppState>();
            let conn = state.lock_write()?;
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
        let results = official_quota::fetch_all_targets(&load_custom_providers(&app));
        persist_official_quota_fetches(&app, results)
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
        let target = official_quota::resolve_target(&provider, &load_custom_providers(&app))?;
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
        let target = official_quota::resolve_target(&provider, &load_custom_providers(&app))?;
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
        let snapshot =
            official_quota::custom::fetch_quota(request.preset, &request.base_url, Some(&secret))?;
        Ok(official_quota::custom::panel::CustomQuotaTestDto {
            windows: snapshot.windows,
            captured_at: snapshot.captured_at,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}
