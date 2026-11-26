//! 一个提供方在快照里占哪些行。首页和取数共用这一份，避免两边各写一套。

use super::{LocalLogin, ResolvedQuotaAccount};
use crate::domain::OfficialQuotaProvider;

pub(crate) struct Placement<'a> {
    pub account: &'a ResolvedQuotaAccount,
    pub active_local: bool,
}

pub(crate) enum ProviderPlan<'a> {
    /// 这个提供方还没有额度账号，沿用原来的单行。
    Unchanged,
    Managed {
        show_builtin: bool,
        placements: Vec<Placement<'a>>,
    },
}

pub(crate) fn plan_provider<'a>(
    provider: OfficialQuotaProvider,
    accounts: &'a [ResolvedQuotaAccount],
    logins: &[LocalLogin],
) -> ProviderPlan<'a> {
    let slug = provider.as_str();
    let mine: Vec<&ResolvedQuotaAccount> = accounts
        .iter()
        .filter(|account| account.config.provider == slug)
        .collect();
    if mine.is_empty() {
        return ProviderPlan::Unchanged;
    }
    let login = logins.iter().find(|login| login.provider == provider);
    let Some(login) = login else {
        return ProviderPlan::Managed {
            show_builtin: false,
            placements: enabled(mine, |_| false),
        };
    };
    if let Some(matched) = mine
        .iter()
        .copied()
        .find(|account| account.config.fingerprint == login.fingerprint)
    {
        if matched.config.enabled {
            let matched_id = matched.config.id.clone();
            return ProviderPlan::Managed {
                show_builtin: false,
                placements: enabled(mine, |account| account.config.id == matched_id),
            };
        }
        let matched_id = matched.config.id.clone();
        return ProviderPlan::Managed {
            show_builtin: false,
            placements: mine
                .into_iter()
                .filter(|account| account.config.enabled && account.config.id != matched_id)
                .map(|account| Placement {
                    active_local: false,
                    account,
                })
                .collect(),
        };
    }
    ProviderPlan::Managed {
        show_builtin: true,
        placements: enabled(mine, |_| false),
    }
}

fn enabled<'a>(
    mine: Vec<&'a ResolvedQuotaAccount>,
    active: impl Fn(&ResolvedQuotaAccount) -> bool,
) -> Vec<Placement<'a>> {
    mine.into_iter()
        .filter(|account| account.config.enabled)
        .map(|account| Placement {
            active_local: active(account),
            account,
        })
        .collect()
}
