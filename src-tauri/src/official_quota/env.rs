//! 官方额度模块的依赖注入容器 [`QuotaEnv`] 与传输层抽象 [`QuotaTransport`]。
//!
//! 参照 `push::PushEnv`：把 `ScanEnv` 默认客户端根目录、平台配置目录、应用数据路径、
//! 确定性时钟与 HTTP / 子进程 / 钥匙串传输统一注入，使内置 9 家、额度账号（含隔离
//! `profile_dir`）、自定义提供商以及生命周期入口都能在 `tempfile` 里零污染端到端测试。

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};

use super::{
    accounts, antigravity, backoff, codex, copilot, custom, devin, droid, CAPTURE_NAME,
    CONFIG_NAME, NOTIFY_NAME,
};
use crate::domain::{OfficialQuotaProvider, Source};
use crate::scan_paths::ScanEnv;

/// 官方额度在应用数据目录下读写的所有文件路径。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotaPaths {
    pub config: PathBuf,
    pub notify_state: PathBuf,
    pub backoff: PathBuf,
    pub claude_capture: PathBuf,
    pub custom: custom::store::CustomQuotaPaths,
    pub accounts: accounts::QuotaAccountPaths,
}

impl QuotaPaths {
    pub fn in_dir(dir: &Path) -> Self {
        Self {
            config: dir.join(CONFIG_NAME),
            notify_state: dir.join(NOTIFY_NAME),
            backoff: dir.join(backoff::STATE_NAME),
            claude_capture: dir.join(CAPTURE_NAME),
            custom: custom::store::CustomQuotaPaths::in_dir(dir),
            accounts: accounts::QuotaAccountPaths::in_dir(dir),
        }
    }

    pub fn app_data() -> Self {
        Self::in_dir(&crate::paths::app_data_dir())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpBody {
    Empty,
    Text(String),
    Bytes(Vec<u8>),
    Form(Vec<(String, String)>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: HttpBody,
    pub timeout: Duration,
}

impl HttpRequest {
    pub fn get(url: impl Into<String>, timeout: Duration) -> Self {
        Self {
            method: HttpMethod::Get,
            url: url.into(),
            headers: Vec::new(),
            body: HttpBody::Empty,
            timeout,
        }
    }

    pub fn post_text(url: impl Into<String>, body: impl Into<String>, timeout: Duration) -> Self {
        Self {
            method: HttpMethod::Post,
            url: url.into(),
            headers: Vec::new(),
            body: HttpBody::Text(body.into()),
            timeout,
        }
    }

    pub fn post_bytes(url: impl Into<String>, body: Vec<u8>, timeout: Duration) -> Self {
        Self {
            method: HttpMethod::Post,
            url: url.into(),
            headers: Vec::new(),
            body: HttpBody::Bytes(body),
            timeout,
        }
    }

    pub fn post_form(
        url: impl Into<String>,
        form: Vec<(String, String)>,
        timeout: Duration,
    ) -> Self {
        Self {
            method: HttpMethod::Post,
            url: url.into(),
            headers: Vec::new(),
            body: HttpBody::Form(form),
            timeout,
        }
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    pub fn body_string(&self) -> Result<String, String> {
        String::from_utf8(self.body.clone()).map_err(|e| e.to_string())
    }

    pub fn body_string_lossy(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// 官方额度的外部传输边界：HTTP、Codex 子进程、macOS 钥匙串与 Antigravity 安装扫描。
pub trait QuotaTransport: Send + Sync {
    /// 执行一次 HTTP 请求。任何拿到状态码的响应（含 4xx/5xx）返回 `Ok(HttpResponse)`；
    /// 仅连接/超时/IO 级传输失败返回 `Err`。
    fn http(&self, request: &HttpRequest) -> Result<HttpResponse, String>;

    fn codex_app_server_rate_limits(&self) -> Result<String, String> {
        Err("未找到 Codex CLI，或无法启动 app-server".to_string())
    }

    fn droid_keychain_key(&self) -> Option<String> {
        None
    }

    fn antigravity_keychain_blob(&self) -> Option<String> {
        None
    }

    fn antigravity_oauth_clients(&self) -> Result<Vec<(String, String)>, String> {
        Err("找不到本机 Antigravity 安装目录，无法取得刷新登录所需的客户端信息".to_string())
    }
}

/// 生产传输实现：真实 `ureq`（回环直连不走代理）、`codex app-server` 与系统钥匙串。
#[derive(Debug, Default)]
pub struct LiveQuotaTransport;

impl QuotaTransport for LiveQuotaTransport {
    fn http(&self, request: &HttpRequest) -> Result<HttpResponse, String> {
        execute_ureq(request)
    }

    fn codex_app_server_rate_limits(&self) -> Result<String, String> {
        codex::query_app_server()
    }

    fn droid_keychain_key(&self) -> Option<String> {
        droid::macos_login_key()
    }

    fn antigravity_keychain_blob(&self) -> Option<String> {
        antigravity::macos_keychain_password()
    }

    fn antigravity_oauth_clients(&self) -> Result<Vec<(String, String)>, String> {
        antigravity::local_oauth_clients()
    }
}

/// 隔离 HTTP 传输：只跑真实 `ureq`（配合 `quota_base` 打本机回环桩），
/// 屏蔽宿主 `codex` CLI、系统钥匙串与 `/Applications` 安装扫描，避免单测污染。
#[derive(Debug, Default)]
pub struct IsolatedHttpTransport;

impl QuotaTransport for IsolatedHttpTransport {
    fn http(&self, request: &HttpRequest) -> Result<HttpResponse, String> {
        execute_ureq(request)
    }
}

fn execute_ureq(req: &HttpRequest) -> Result<HttpResponse, String> {
    let agent = crate::net::agent_for_url(&req.url, req.timeout);
    let mut call = match req.method {
        HttpMethod::Get => agent.get(&req.url),
        HttpMethod::Post => agent.post(&req.url),
    };
    for (name, value) in &req.headers {
        call = call.set(name, value);
    }
    let result = match &req.body {
        HttpBody::Empty => call.call(),
        HttpBody::Text(text) => call.send_string(text),
        HttpBody::Bytes(bytes) => call.send_bytes(bytes),
        HttpBody::Form(pairs) => {
            let refs: Vec<(&str, &str)> = pairs
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            call.send_form(&refs)
        }
    };
    match result {
        Ok(response) => read_ureq_response(response),
        Err(ureq::Error::Status(_, response)) => read_ureq_response(response),
        Err(ureq::Error::Transport(err)) => Err(err.to_string()),
    }
}

fn read_ureq_response(response: ureq::Response) -> Result<HttpResponse, String> {
    let status = response.status();
    let headers = response
        .headers_names()
        .into_iter()
        .filter_map(|name| {
            let value = response.header(&name)?.to_string();
            Some((name, value))
        })
        .collect();
    let mut body = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut body)
        .map_err(|e| e.to_string())?;
    Ok(HttpResponse {
        status,
        headers,
        body,
    })
}

#[derive(Clone, Copy, Debug)]
enum QuotaClock {
    System,
    Fixed(DateTime<Utc>),
}

/// 官方额度运行环境快照：封装路径解析、时钟、配置目录与外部传输。
#[derive(Clone)]
pub struct QuotaEnv {
    pub scan_env: ScanEnv,
    pub paths: QuotaPaths,
    pub config_dir: Option<PathBuf>,
    pub droid_home: PathBuf,
    pub copilot_config_home: PathBuf,
    clock: QuotaClock,
    pub quota_base: Option<String>,
    pub transport: Arc<dyn QuotaTransport>,
}

impl QuotaEnv {
    /// 生产环境快照：捕获宿主 `ScanEnv`、`dirs::config_dir()`、环境变量与真实传输。
    pub fn live(paths: QuotaPaths) -> Self {
        let home = crate::ingest::default_home();
        let scan_env = ScanEnv::capture_for_home(&home);
        let droid_home = std::env::var("FACTORY_HOME_OVERRIDE")
            .ok()
            .map(PathBuf::from)
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| scan_env.home().join(".factory"));
        let copilot_config_home = std::env::var("XDG_CONFIG_HOME")
            .ok()
            .map(|raw| raw.trim().to_string())
            .filter(|raw| !raw.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| scan_env.home().join(".config"));
        Self {
            scan_env,
            paths,
            config_dir: dirs::config_dir(),
            droid_home,
            copilot_config_home,
            clock: QuotaClock::System,
            quota_base: None,
            transport: Arc::new(LiveQuotaTransport),
        }
    }

    /// 隔离测试环境：全部凭证根限定在 `scan_env.home()` 下，固定时钟，不读宿主配置目录或钥匙串。
    pub fn isolated(scan_env: ScanEnv, paths: QuotaPaths, now: DateTime<Utc>) -> Self {
        let config_dir = scan_env.home().join(".config");
        let droid_home = scan_env.home().join(".factory");
        let copilot_config_home = config_dir.clone();
        Self {
            scan_env,
            paths,
            config_dir: Some(config_dir),
            droid_home,
            copilot_config_home,
            clock: QuotaClock::Fixed(now),
            quota_base: None,
            transport: Arc::new(IsolatedHttpTransport),
        }
    }

    pub fn with_config_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.config_dir = Some(dir.into());
        self
    }

    pub fn with_droid_home(mut self, dir: impl Into<PathBuf>) -> Self {
        self.droid_home = dir.into();
        self
    }

    pub fn with_copilot_config_home(mut self, dir: impl Into<PathBuf>) -> Self {
        self.copilot_config_home = dir.into();
        self
    }

    pub fn with_now(mut self, now: DateTime<Utc>) -> Self {
        self.clock = QuotaClock::Fixed(now);
        self
    }

    pub fn with_quota_base(mut self, base: Option<String>) -> Self {
        self.quota_base = base.filter(|value| !value.trim().is_empty());
        self
    }

    pub fn with_transport(mut self, transport: Arc<dyn QuotaTransport>) -> Self {
        self.transport = transport;
        self
    }

    pub fn now(&self) -> DateTime<Utc> {
        match self.clock {
            QuotaClock::System => Utc::now(),
            QuotaClock::Fixed(now) => now,
        }
    }

    pub(crate) fn rebase_url(&self, url: &str) -> String {
        super::rebase_quota_url(url, self.quota_base.as_deref())
    }

    pub fn claude_credentials_path(&self) -> PathBuf {
        self.scan_env
            .client_credential_path(Source::Claude, ".credentials.json")
    }

    pub fn claude_hook_settings_path(&self) -> PathBuf {
        self.scan_env.claude_hook_settings_path()
    }

    pub fn codex_auth_path(&self) -> PathBuf {
        self.scan_env
            .client_credential_path(Source::Codex, "auth.json")
    }

    pub fn grok_home(&self) -> PathBuf {
        self.scan_env
            .client_credential_root(Source::Grok, "auth.json")
    }

    pub fn grok_auth_path(&self) -> PathBuf {
        self.grok_home().join("auth.json")
    }

    pub fn opencode_data_dir(&self) -> PathBuf {
        self.scan_env
            .client_credential_root(Source::Opencode, "auth.json")
    }

    pub fn opencode_auth_path(&self) -> PathBuf {
        self.opencode_data_dir().join("auth.json")
    }

    pub fn vscode_global_storage_dir(&self, app_dir: &str) -> Option<PathBuf> {
        self.config_dir
            .as_deref()
            .map(|dir| crate::vscode_state::global_storage_dir_in(dir, app_dir))
    }

    pub fn cursor_storage_dir(&self) -> Option<PathBuf> {
        self.vscode_global_storage_dir("Cursor")
    }

    pub fn antigravity_storage_dirs(&self) -> Vec<PathBuf> {
        antigravity::APP_DIRS
            .into_iter()
            .filter_map(|app| self.vscode_global_storage_dir(app))
            .collect()
    }

    pub fn devin_storage_dirs(&self) -> Vec<PathBuf> {
        devin::APP_DIRS
            .into_iter()
            .filter_map(|app| self.vscode_global_storage_dir(app))
            .collect()
    }

    pub fn copilot_credential_paths(&self) -> Vec<PathBuf> {
        copilot::credential_paths_under(&self.copilot_config_home)
    }

    pub fn default_roots(&self, provider: OfficialQuotaProvider) -> Vec<PathBuf> {
        match provider {
            OfficialQuotaProvider::Claude => vec![self.claude_credentials_path()],
            OfficialQuotaProvider::Codex => vec![self.codex_auth_path()],
            OfficialQuotaProvider::Cursor => self.cursor_storage_dir().into_iter().collect(),
            OfficialQuotaProvider::Grok => vec![self.grok_auth_path()],
            OfficialQuotaProvider::Droid => vec![self.droid_home.clone()],
            OfficialQuotaProvider::Antigravity => self.antigravity_storage_dirs(),
            OfficialQuotaProvider::OpenCode => vec![self.opencode_auth_path()],
            OfficialQuotaProvider::Copilot => vec![self.copilot_config_home.clone()],
            OfficialQuotaProvider::Devin => self.devin_storage_dirs(),
        }
    }
}
