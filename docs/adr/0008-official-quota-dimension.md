# 官方额度作为独立维度

> **2026-09-03 修订（成员与取数）**：内置账号已扩到九家（Claude / Codex / Cursor / Grok / Droid / Antigravity / OpenCode / Copilot / Devin）；自定义提供商见 ADR 0012 / 0013。下文仍保留最初四条通道的决策叙述，不要把它读成当前完整名单。现行取数见 `official_quota/fetch.rs` 与 `docs/probe/official-quota.md`：Claude 首选 `/api/oauth/usage`，失败再回落 statusline 捕获；Codex 首选 ChatGPT `wham/usage`，失败再回落 `codex app-server`；Cursor 官方额度只读本机 `state.vscdb`（与 ADR 0006 相同，不走钥匙串）；其余内置家各自在 `fetch_provider` 分派。
>
> **2026-09-03 修订（托盘）**：下文「托盘只读缓存、不为菜单栏打 HTTP」已过时。现行 `tray.rs` 在菜单「刷新」、全量摄取后、以及每 5 分钟 `refresh_if_stale` 时会调 `sync_official_quota` → `fetch_all_targets`（受退避冷却约束，与主窗口刷新同级），再读缓存更新标题。破例仍是「受控额度通道」，不是通用摄取。

Claude / Codex / Cursor / Grok 的订阅限额是账号级事实，和本机消耗记录不是同一件事：官方 5 小时 / 周窗口还包含 claude.ai、Cowork、grok.com 等没有本地 jsonl 的用量。本机计费窗只是本地时间戳估计，不能画在同一根进度条上。

## 现行实现（2026-09，以代码为准）

**决定（现行）**：独立维度「官方额度 (Official Quota)」。成员 = 内置九家账号（Claude / Codex / Cursor / Grok / Droid / Antigravity / OpenCode / Copilot / Devin，见 `domain::OfficialQuotaProvider::ALL`）+ 用户登记的**自定义提供商**（ADR 0012 / 0013）。合流出口 **`official_quota::load_dto`**；取数 **`official_quota/fetch.rs::fetch_all_targets`**（并行、各 provider 独立失败）。

| 内置账号 | 凭证 / 取数要点 |
|----------|----------------|
| Claude | 本机 OAuth → `/api/oauth/usage`；失败回落 statusline 捕获文件 |
| Codex | 本机 `auth.json` → ChatGPT `wham/usage`；失败回落 `codex app-server` |
| Cursor | 只读本机 `state.vscdb`（与 ADR 0006 相同，**不走钥匙串**）→ `GET /api/usage-summary` |
| Grok | `~/.grok/auth.json` → CLI billing 接口 |
| Droid / Antigravity / OpenCode / Copilot / Devin | 各自 `official_quota/<provider>.rs` + `fetch_provider` 分派；字段见 `docs/probe/official-quota.md` |

- **自定义提供商**：只打计费/余额接口；密钥不进备份；托盘「最紧一档」按窗口有无 `resets_at` 分流（0013）。
- **缓存**：「最后一次正确结果」；新鲜度 `official` / `stale`（>10 分钟）/ `unavailable`。
- **托盘**：`tray.rs::sync_official_quota` 在菜单刷新、全量摄取后、以及每 5 分钟 stale 检查时调用 `fetch_all_targets`（受退避冷却约束），再读缓存更新标题。
- **CLI 调试**：`cargo run --bin quota`（`mabiao-quota`），见 `docs/probe/official-quota.md`。
- **undetected**：本机无凭证且无历史缓存的账号出现在 DTO 的 `undetected[]`，不占首页行。

不进 `UsageRecord` / `Source` / 本机 token KPI；禁止把官方百分比叠进本机 5 小时 / 7 天估计窗。ADR 0012 是第二条受控联网通道（地址由用户提供、解析器仍内置）。

## 历史决定（2026-08 四条通道）

> 以下为最初四条通道的决策叙述，**已被上节取代**；保留供理解决策演进，不要当作当前完整名单或取数路径。

**决定（历史）**：新增独立维度「官方额度 (Official Quota)」。四条受控取数通道各自失败、互不影响：

- Claude：设置页 opt-in 写入 statusline hook，把 stdin 的 `rate_limits` 落到本机捕获文件；应用只读该文件。
- Codex：在用户打开总览或手动刷新时，一次性启动本机 `codex app-server`，调用 `account/rateLimits/read`。
- Cursor：复用已有钥匙串 token，请求 `GET /api/usage-summary`（与账号用量事件接口分开）。
- Grok：读取本机 `~/.grok/auth.json`（`GROK_HOME` 可覆盖）里未过期的会话 token 与 `user_id`，按官方 CLI 头请求 CLI chat proxy 的 `GET /v1/billing?format=credits`（周额度）和 `GET /v1/billing`（月额度，失败不影响周额度）。缺 `user_id` 时先 `GET /v1/user`。

缓存遵循「最后一次正确结果」：取数失败不覆盖旧窗口，只更新错误文案。新鲜度三态：`official` / `stale`（捕获超过 10 分钟）/ `unavailable`。（历史：托盘曾只读缓存；现行见上节。）

这是继 ADR 0006 之后的**第二个显式破例维度**。（历史：最初只允许四条受控通道。）

> **破例**：ADR 0012「自定义额度提供商」在这条禁令上开了一个受控口子——用户可以在设置页登记第三方中转站，取数地址因此**由用户提供**。它仍不是通用摄取：解析器只能是内置预设类型、只打计费 / 余额接口、不进消耗记录。看到 `official_quota::custom` 下那条联网通道时，请先读 ADR 0012 再决定它是不是违规代码。

**理由**：用户真正怕的是下午断窗、周四撞线。官方百分比必须和本机估计并排，且来源/新鲜度可见，才能提高感知质量而不污染本机统计口径。

## Consequences

- Claude 在 OAuth 与 statusline 捕获均不可用时，该行 `unavailable`（历史叙述曾写「必须配置 hook」）。
- 已有 `statusLine` 的 Claude 配置不得覆盖，只提供可复制 command。
- Cursor 限额接口与账号用量一样是非公开的，结构变更时降级为可读中文错误。Cursor 一档订阅里并行有总量 / Auto / API / 按需，必须拆成多个官方额度窗口，不能只画 `totalPercentUsed`。
- Codex 依赖本机 CLI；进程不在或超时不影响 Claude / Cursor / Grok。
- Grok 依赖本机 `grok login` 写入的会话凭证；文件缺失、过期或仅有 API key 时该行 `unavailable`，不影响另外三路。Grok 限额接口与消耗记录摄取分开，结构变更时保留上次正确缓存。
- 80% / 100% 告警按 `provider + window_kind + resets_at` 去重，`stale` 不弹，与月度预算分开开关。
- 预计撞线只由连续两次官方快照的百分比（或金额）差计算，文案写成估计；禁止把本机 5 小时燃烧叠进官方进度条。尚未凑齐两拍、或间隔不足一分钟时不显示。
