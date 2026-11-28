//! 本机是否有某家的登录态——只看本地文件，不联网、不写任何东西。
//!
//! 用途有两个：没凭证的 provider 不去打网（省一次必然失败的请求），
//! 界面上也不给它留一行永远好不了的红字。
//!
//! 判定要「宁可显示、不可误藏」：探针只在能确定读不到凭证时返回 false，
//! 拿不准就返回 true，让真正的刷新去报准确的错。

use std::path::Path;

use crate::domain::OfficialQuotaProvider;
use crate::official_quota::{antigravity, devin, droid, opencode, QuotaEnv, QuotaPaths};

pub fn has_local_credentials(provider: OfficialQuotaProvider) -> bool {
    has_local_credentials_in(&QuotaEnv::live(QuotaPaths::app_data()), provider)
}

pub fn has_local_credentials_in(env: &QuotaEnv, provider: OfficialQuotaProvider) -> bool {
    match provider {
        // 文件在就算有登录痕迹。token 过期、缺 scope 是刷新时报的错，
        // 不能在这里当成「没装 Claude」把整行从列表里拿掉。
        OfficialQuotaProvider::Claude => {
            claude_artifacts_present(&env.claude_credentials_path(), &env.paths.claude_capture)
        }
        // auth.json 在就算 Codex 在用。纯 API key 没有订阅百分比，刷新会写清
        // 楚原因；这里如果要求 tokens.access_token，列表里就永远看不到这一行。
        OfficialQuotaProvider::Codex => codex_artifacts_present(&env.codex_auth_path()),
        OfficialQuotaProvider::Cursor => env
            .cursor_storage_dir()
            .and_then(|dir| {
                crate::cursor_credentials::read_credential_at(&dir)
                    .ok()
                    .flatten()
            })
            .is_some_and(|credential| !credential.is_expired_at(env.now().timestamp_millis())),
        OfficialQuotaProvider::Grok => env.grok_auth_path().exists(),
        OfficialQuotaProvider::Droid => droid::load_access_token_in(env).is_ok(),
        OfficialQuotaProvider::Antigravity => antigravity::load_local_tokens_in(env).is_ok(),
        OfficialQuotaProvider::OpenCode => {
            // 文件读坏了不算「没有」——让刷新去报错，别把故障当成没登录。
            opencode::load_api_key(&env.opencode_auth_path()).map_or(true, |key| key.is_some())
        }
        OfficialQuotaProvider::Copilot => env
            .copilot_credential_paths()
            .into_iter()
            .any(|path| path.exists()),
        OfficialQuotaProvider::Devin => devin::load_api_key_in(env).is_ok(),
    }
}

pub(crate) fn claude_artifacts_present(credentials: &Path, capture: &Path) -> bool {
    credentials.exists() || capture.exists()
}

pub(crate) fn codex_artifacts_present(auth: &Path) -> bool {
    auth.exists()
}
