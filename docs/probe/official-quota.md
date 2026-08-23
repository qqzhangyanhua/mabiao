# 官方额度字段探测

只记账号级限额字段位置，不写会话正文。

## Claude

首选 `GET https://api.anthropic.com/api/oauth/usage`（零配置），失败才回落下面的 statusline 捕获。

请求头缺一不可：`Authorization: Bearer <accessToken>`、`anthropic-beta: oauth-2025-04-20`、`User-Agent: claude-code/<版本>`、`Accept`/`Content-Type: application/json`。

凭证读 `~/.claude/.credentials.json` 的 `claudeAiOauth`：

- `accessToken` + `expiresAt`（毫秒）。**`expiresAt` 为 0 或缺失不当成过期**——第三方代理会写 0，交给接口判。
- 必须有 `user:profile` scope；`claude setup-token` 生成的纯推理 token 没有，接口会拒，先本地筛掉。
- 只读不刷新：刷新会把新 token 写回第三方文件，违反 ADR 0010。过期就提示打开一次 Claude Code。
- macOS 上 Claude Code 以钥匙串为准、文件为镜像；当前只读文件。

响应：

- `five_hour` / `seven_day` / `seven_day_sonnet` → `utilization`（0–100）+ `resets_at`
- `limits[]` 里 `kind == "weekly_scoped"` 的条目 → `percent` + `scope.model.display_name`，按模型拆的周窗口。老的 `seven_day_<model>` 顶层键现在返回 null，模型名不写死。
- `resets_at` 既可能是 ISO 字符串也可能是 epoch 秒
- `extra_usage.{is_enabled,used_credits,monthly_limit}`（分），当前不采

429 限流较紧，提示里要劝阻手动狂刷。

## Claude statusline stdin

Claude Code 2.1.80+ 在 statusline 命令的 stdin JSON 里提供：

- `rate_limits.five_hour.used_percentage`（0–100；偶发泄漏 `resets_at` 的 epoch，需丢弃 >100）
- `rate_limits.five_hour.resets_at`（Unix 秒或 ISO）
- `rate_limits.seven_day.used_percentage`
- `rate_limits.seven_day.resets_at`

捕获文件：应用数据目录 `claude_statusline.json`。

## Codex

首选 `GET https://chatgpt.com/backend-api/wham/usage`（不依赖 CLI 装没装），失败才回落下面的 app-server。

凭证读 `~/.codex/auth.json`（`CODEX_HOME` 可覆盖）的 `tokens.{access_token, account_id}`：

- 请求头：`Authorization: Bearer`、`Accept: application/json`、`User-Agent`，有 `account_id` 时加 `ChatGPT-Account-Id`。
- **只有 `OPENAI_API_KEY`、没有 `tokens` 的账号是按量计费，没有额度百分比**，直接判定不可用，别报解析错误。
- 只读不刷新（ADR 0010）。

响应 `rate_limit.{primary_window, secondary_window}`，每个窗口：

- `used_percent`（0–100）；缺了就取响应头 `x-codex-primary-used-percent` / `x-codex-secondary-used-percent`
- `limit_window_seconds` 决定窗口种类——**不能按 primary/secondary 的位置认**，Codex 会把临时只剩一条的周限额挪进 primary 槽。18000 → 5 小时，604800 → 7 天，其它按小时数命名。
- `reset_at`（epoch 秒）或 `reset_after_seconds`（相对量，要按当前时间换算）

`plan_type`、`rate_limit_reset_credits` 当前不采。

## Codex app-server

一次性启动 `codex app-server`，`initialize` → `initialized` → `account/rateLimits/read`。

- `result.rateLimits.primary.usedPercent`
- `result.rateLimits.primary.windowDurationMins`
- `result.rateLimits.primary.resetsAt`
- 若有 `rateLimitsByLimitId`，按 bucket 展开 primary/secondary

进程不在或超时：该行 `unavailable`，不影响另外三路。

## Cursor

凭证只有一个来源：本机 Cursor 客户端。没有手动粘贴通路，也不落钥匙串。`state.vscdb`（Win `%APPDATA%\Cursor\User\globalStorage`、mac `~/Library/Application Support/Cursor/...`、Linux `~/.config/Cursor/...`，三平台都在 `dirs::config_dir()` 下）的 `ItemTable`：

- `cursorAuth/accessToken`：WorkOS JWT，`iss=https://authentication.cursor.sh`，`sub` 形如 `google-oauth|user_01J…`。value 列可能是 TEXT 也可能是 BLOB。
- `cursorAuth/cachedEmail` / `cursorAuth/stripeMembershipType`：只用于设置页展示。
- cookie 值 = `<sub 里 "|" 之后那段>` + `%3A%3A` + `<jwt>`，即 `WorkosCursorSessionToken`。
- 过期判断用 JWT 的 `exp`（留 60s 容差）。

必须原地只读打开：库有几百 MB，且 `immutable=1` / 复制会跳过 WAL 读到陈旧值。

拿到 token 后 `GET https://cursor.com/api/usage-summary`。

Cursor 订阅限额是多档并行，不能只取总量：

- `individualUsage.plan.totalPercentUsed`（或 `used` / `limit`）→ 窗口 `billing_cycle` / 总量
- `individualUsage.plan.autoPercentUsed` → 窗口 `auto` / Auto
- `individualUsage.plan.apiPercentUsed` → 窗口 `api` / API
- `individualUsage.onDemand.used` / `limit`（无 limit 时回退 `teamUsage.onDemand`）→ 窗口 `on_demand` / 按需
- `billingCycleEnd`

与账号用量事件接口 `get-filtered-usage-events` 分开。结构变更时保留上次正确缓存。

## Antigravity

登录态两条，**macOS 上 AGY / 2.7+ 优先读钥匙串**：

- service=`gemini`、account=`antigravity`（zalando go-keyring）
- 值是 `go-keyring-base64:` + JSON：`token.{access_token,refresh_token,expiry}`、`auth_method`
- 走 `/usr/bin/security find-generic-password`（和 Droid 同一条已授权路径，通常不弹框），5 秒超时

旧 VSCode 壳才读 `state.vscdb`（和 Cursor 同样落在 `dirs::config_dir()` 下，Win 是 `%APPDATA%\Antigravity\User\globalStorage`，旧包还有 `Antigravity IDE/`）的 `ItemTable`：

- `antigravityAuthStatus`：JSON，`apiKey` 是 Google OAuth access token（`ya29.`），只活约 1 小时，**基本总是过期，不能直接用**。
- `antigravityUnifiedStateSync.oauthToken`：嵌套 protobuf（外层 base64 → protobuf → 内层 base64 → protobuf），内层含 access token、`Bearer`、**refresh token（`1//` 开头）**。字段号不稳定，按形状找；内层 base64 的 padding 未必齐，要按无 padding 解。
- 刷新要用 Antigravity 自己的 OAuth 客户端。**不内嵌到本仓库**——那是 Google 发给 Antigravity 的凭证，GitHub 的 secret scanning 也会拦。运行时从本机安装里扫：老版本 / `Antigravity IDE.app` 在 `out/main.js`；macOS 2.7+ 的 `Antigravity.app` 打成 asar，客户端在 `Contents/Resources/bin/language_server`。先顺 PATH 上的 `antigravity` / `antigravity-ide` 反查安装根目录，再退回各平台默认位置。id 和 secret 各有多个且配对关系看不出来，全组合都试，错配会快速返回 `invalid_client`。
- 数据目录两边都要找：新版在 `Antigravity/`，旧 macOS 包在 `Antigravity IDE/`。
- 先用 `antigravityAuthStatus.apiKey` 直接打，401 了才走上面的刷新，省一次往返。

`POST https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary`，body `{}`（有 project 就传 `{"project": pid}`）：

- **RPC 名是 `retrieveUserQuotaSummary`，不是 `retrieveUserQuota`**；后者对消费级账号一律 403。
- **`User-Agent` 必须带 `Antigravity/` 标记**，否则 403「no valid license」。实测 `vscode/1.X.X (Antigravity/4.3.0)`、`…(Antigravity/0.0.0)`、`Antigravity/4.3.0` 都通，`vscode/1.X.X` 和其它 UA 都 403 —— 只认标记，不认版本号。那个 403 是 UA 门禁，不是真的没 license。
- 响应 `groups[].buckets[]`：`bucketId`（→ 窗口 kind，`-` 换 `_`）、`window`（`weekly` / `5h`）、`remainingFraction`（**剩余**，`(1-x)*100` 才是已用）、`resetTime`。
- 桶的 `displayName` 是「Weekly Limit Remaining」这种剩余口径，直接展示会和已用读反，所以按 `window` 自己起名，group 的 `displayName` 做前缀。
- 端点按 prod → daily → sandbox 兜底；401/403 不换环境直接结束。

`v1internal:fetchAvailableModels` 也能拿到每个模型的 `quotaInfo.{remainingFraction, resetTime}`，是同一个 5h 桶的数字，当前不采。

## Droid (Factory)

凭证读本机 `~/.factory`（`FACTORY_HOME_OVERRIDE` 可覆盖）：

- `auth.v2.file`：`base64(iv):base64(tag):base64(密文)`，AES-256-GCM，**iv 是 16 字节**（不是常见的 12），tag 单独一段而不是拼在密文尾。
- `auth.v2.key`：明文放在旁边的 base64 32 字节密钥。
- 解出 `{access_token, refresh_token, active_organization_id}`；`access_token` 是 WorkOS JWT（`iss=https://api.workos.com`）。
- 旧版 `auth.json` 是明文同结构，作为兜底。macOS 上 droid 可能改用系统钥匙串，那种情况读不到，该行 `unavailable`。

`GET https://api.factory.ai/api/billing/limits`，`Authorization: Bearer <access_token>`：

- `limits.standard.{fiveHour,weekly,monthly}` → 窗口 `five_hour` / `weekly` / `monthly`，标签「标准 …」
- `limits.core.{fiveHour,weekly,monthly}` → 窗口 `core_*`，标签「Core …」（Droid Core 池）
- 每档 `usedPercent`（0–100）、`windowEnd`（ISO，→ `resets_at`）、`secondsRemaining`
- 另有 `extraUsageBalanceCents` / `overagePreference` / `usesTokenRateLimitsBilling`，当前不采

`windowEnd` 已过去的档位跳过——对齐 droid 自己的显示逻辑（过期窗说明该桶不在计费窗内，不等于 0%）。全部过期时报结构异常，保留上次正确缓存。

EU 区是 `https://api.eu.factory.ai`，当前不自动识别。

## OpenCode (Zen / Go)

`GET https://opencode.ai/zen/go/v1/usage`，`Authorization: Bearer <key>`。

凭证读数据目录下的 `auth.json` 里 `opencode-go.key`。数据目录按 OpenCode 自己的顺序：`OPENCODE_DATA_DIR` > `$XDG_DATA_HOME/opencode` > `~/.local/share/opencode`。只认 `opencode-go` 这一条，其它 provider 的条目忽略。

**文件缺失 = 没登录（不是错误）；文件在但坏了要报错**，别把坏掉的存储当成登出。

响应 `usage.{rolling, weekly, monthly}`，每档 `percent`（**已用**口径，不用取反）+ `resetsAt`（ISO）。出错时是 `{type, error:{type, message}}`，把 `error.type` 带进提示；HTML / Cloudflare 页面则没有这个结构。

## Copilot

`GET https://api.github.com/copilot_internal/user`。

- **`Authorization` 用 `token` 而不是 `Bearer`**，这是这个内部端点认的方案。
- 还要带 Copilot 客户端那几个头：`Editor-Version`、`Editor-Plugin-Version`、`User-Agent`、`X-Github-Api-Version: 2025-04-01`。
- 凭证按「不弹窗的文件优先」：`~/.config/github-copilot/apps.json`（老版本 `hosts.json`）→ 值里的 `oauth_token`（键名形如 `github.com:<clientId>`，不稳定，扫所有条目）；其次 `~/.config/gh/hosts.yml` 的 `oauth_token`（只取 `github.com:` 段，避免读到企业实例的）。macOS 钥匙串那条不做，会弹授权框。

响应 `quota_snapshots.{premium_interactions, chat, completions}`，给的是**剩余**口径：

- `percent_remaining`，或 `remaining` / `entitlement`，取反才是已用
- **`unlimited: true`、`entitlement: -1`、`remaining: -1` 是无限额度**，`entitlement: 0` 是组织按量计费席位的零额度占位——四种都要丢掉，不能显示成 0% 或 100%
- 重置时间 `quota_reset_date` / `limited_user_reset_date` 是**纯日期**（`2026-09-01`），通用的 `parse_resets_at` 认不了，要按 UTC 零点补一层

组织账单 `orgs/{org}/settings/billing/usage/summary` 当前不采。

## Devin / Windsurf

Cognition 收购 Windsurf 后统一发 Devin 凭证，两者是同一条链路。

凭证在 VSCode 风格 `state.vscdb` 的 `windsurfAuthStatus`（和 Antigravity 同构的 `{apiKey, userStatusProtoBinaryBase64}`），`apiKey` 形如 `devin-…`，明文。**客户端目录两边都要找**：装 Windsurf 的在 `Windsurf/`，装 Devin 的在 `Devin/`。

```
POST https://server.codeium.com/exa.seat_management_pb.SeatManagementService/GetUserStatus
Content-Type: application/json
Connect-Protocol-Version: 1
{"metadata":{"apiKey":"devin-…","ideName":"devin","ideVersion":"1.108.2",
             "extensionName":"devin","extensionVersion":"1.108.2","locale":"en"}}
```

**apiKey 走 body 的 metadata，不是 Authorization 头**（Connect 协议）。

响应 `userStatus.planStatus`：

- `dailyQuotaRemainingPercent` / `weeklyQuotaRemainingPercent` —— **剩余**口径，取反才是已用
- `dailyQuotaResetAtUnix` / `weeklyQuotaResetAtUnix` —— epoch 秒
- **数字可能被包成字符串**（`"100"`），两种都要认
- `planInfo.hideDailyQuota` 为 true 时藏掉日额度，但**周额度也没有时不能一起藏**——那日额度就是唯一有意义的一条
- `planInfo.planName` / `teamsTier`、`overageBalanceMicros` 当前不采

服务端地址可被客户端配置覆盖（`windsurf_auth.apiServerUrl`，存在加密的 `secret://` 条目里），当前只用默认值。

## Grok CLI-proxy billing

读取本机 `~/.grok/auth.json`（`GROK_HOME` 可覆盖）里未过期的会话 token。优先 `https://auth.x.ai…` 作用域，其次 `https://accounts.x.ai/sign-in`。token 字段为 `key`（兼容 `access_token`）。跳过 `web_login` 与纯 API key（`xai::api_key` / `auth_mode=api_key`）。

请求头对齐官方 CLI：`Authorization: Bearer <token>`、`X-XAI-Token-Auth: xai-grok-cli`、`x-userid`（`auth.json` 的 `user_id`，缺则先 `GET /v1/user`）、`x-grok-client-version`（`~/.grok/.metadata_version`，否则 1.0.5）、`x-grok-client-mode: interactive`。

REST `?format=credits` 对部分账号会 500（`Failed to serialize billing response`）。此时回落到 `POST https://grok.com/grok_api_v2.GrokBuildBilling/GetGrokCreditsConfig`（空 gRPC-web 帧 + 同一套 Bearer），只取周额度百分比和重置时间。

- `GET https://cli-chat-proxy.grok.com/v1/billing?format=credits`
  - `config.creditUsagePercent`（0–100）→ 窗口 `weekly` / 周额度
  - 缺百分比但 `currentPeriod.type=USAGE_PERIOD_TYPE_WEEKLY` 且有 `end` → 周额度 0%
  - 无周百分比时回退 `productUsage[GrokBuild].usagePercent`
  - 同时有周百分比时另开窗口 `product_grokbuild` / Grok Build
  - `config.onDemandUsed.val` / `onDemandCap.val`（cap > 0）→ 窗口 `on_demand` / 按需
  - 重置时间：`config.currentPeriod.end`，其次 `billingPeriodEnd`
- `GET https://cli-chat-proxy.grok.com/v1/billing`（失败不影响周额度）
  - `used` / `monthlyLimit`（或 `usage.totalUsed`，支持 `{val}` 包装）→ 窗口 `monthly` / 月额度
  - 缺 `used` 不当成 0%

文件缺失、过期或结构变更：该行 `unavailable`，保留上次正确缓存。不把 token 或 billing 原文写入日志。

## 出网与代理

所有 provider 请求都走 `net::agent_with_timeout`，不要再直接 `ureq::get/post`——ureq 不会自己读 `HTTPS_PROXY` 之类的环境变量，裸调用等于让必须走代理的用户全线连不上。

代理解析顺序：应用数据目录的 `network.json`（`{"proxy": "http://host:port"}`）> 环境变量 `HTTPS_PROXY` / `https_proxy` / `ALL_PROXY` / `all_proxy` / `HTTP_PROXY` / `http_proxy`。

- 两者都要支持：桌面应用从图形界面启动，拿不到用户在 shell profile 里 export 的变量。
- `network.json` 里 `proxy` 写空串 = **明确直连**，用来盖掉不想要的环境变量。
- 支持 `socks5://`（ureq 的 `socks-proxy` feature）。
- 代理串解析失败时退回直连，而不是让所有 provider 一起挂掉。
- `NO_PROXY` 当前不支持。

## 取数退避

连续失败的 provider 先歇一会儿再试，状态在应用数据目录的 `official_quota_backoff.json`。

- **被限流（错误里含「限流」或 429）**：起步 10 分钟，每次连续失败翻倍，封顶 60 分钟。
- **其它失败**：起步 1 分钟，翻倍，封顶 15 分钟。
- 成功即清零，下次失败从最短等待重新起步。封顶后不再增长，不会把 provider 永久拉黑。
- **手动刷新同样受约束**——「多点几次」正是让限流恢复更慢的原因；但会返回「还要等 N 分钟」的提示，不能让按钮看起来没反应。
- 状态落盘而不是只放内存：否则重启一次就绕过去了。
- 冷却期间不覆盖已存的行，界面继续显示上次成功的结果。

限流判定目前靠错误字符串里的标记（provider 的错误还是纯 `String`）。新增 provider 时，限流提示里要带「限流」或 HTTP 码。

## 一次性 CLI

`mabiao-quota` 输出上面这些额度的稳定 JSON，给 agent / 脚本用。

```
mabiao-quota            # 读缓存，不联网
mabiao-quota --refresh  # 先取一次再输出（受退避与按需启用约束）
```

- **默认不联网**：额度接口大多限流很紧，脚本轮询应该打我们已缓存的结果。要新鲜数据才显式 `--refresh`。
- 输出就是 `OfficialQuotaDto`：`rows[]`（provider / windows / freshness / captured_at / error）+ `undetected[]`。
- 读应用同一个 sqlite，只读连接（WAL 下不阻塞正在写的应用）。
- 应用没跑过、库还不存在时输出**形状正确的空结果**而不是报错——脚本不该因为「还没用过」就挂。
- 退出码：0 正常，1 出错，2 参数不认识。
