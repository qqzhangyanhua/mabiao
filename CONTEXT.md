# 码表 (Mabiao)

一个在本机运行的图形界面工具，扫描各类 AI 编程 CLI 工具留在本地的会话数据，聚合并展示 token 消耗（及可选费用）的明细。

## Language

**消耗记录 (Usage Record)**：
统一的标准化用量条目，是所有工具数据归一化后的通用模型。至少包含：时间、来源工具、模型、provider、项目、会话 ID（及原始文件定位）、各口径 token（输入/输出/缓存/推理/总量）、可选费用。不含会话正文。
_Avoid_: 日志、log、message（这些是原始数据，不是归一后的记录）

**来源 (Source)**：
一个被统计的 AI 工具（如 codex、claude code、pi、omp、opencode、kimi 等）。每个 Source 有各自的本地存储格式与字段命名。
_Avoid_: 工具、tool、渠道

**适配器 (Adapter)**：
把某个 Source 的原始存储格式解析、归一化成「消耗记录」的模块。新增一个来源 = 往适配器表加一行 + 写一个 Adapter，统计与界面逻辑不受影响。对话记录不走这张表，见「对话记录适配器」。
_Avoid_: parser、解析器、插件；不要用它指对话正文/事件的解析模块

**Token 口径 (Token Dimension)**：
token 的分类计量：输入 (input)、输出 (output)、缓存读 (cache read)、缓存写/创建 (cache creation)、推理 (reasoning)、总量 (total)。不同工具暴露的口径不完全一致。
_Avoid_: token 类型

**代码量 (Code Volume)**：
Cursor `scored_commits` 记录的提交行数：新增/删除/净增、Composer、Tab、人工，以及按天、按分支、提交明细。与 token 无关，是独立维度。AI 占比 = Composer 新增 ÷ 新增行，Tab 单独展示、不计入该百分比。
_Avoid_: 用量、消耗（避免与 token 混淆）；不要把 hash 条数当成行数；不要并进报告的 token 总数

**Cursor 账号用量 (Cursor Account Usage)**：
从 Cursor 云端仪表盘拉回的账号级 token 事件，含全部设备与全时段，self-serve 计划下仅有 token、没有费用。独立于本机消耗记录与代码量，不并入本机 token 总量、不进 `UsageRecord` / `Source` / 5 小时计费窗。凭证只有一个来源：本机 Cursor 客户端写在 globalStorage `state.vscdb` 里的登录态（Cursor 自己续期），没有手动粘贴通路、也不落钥匙串；缓存可在设置页独立清空，不参与本机文件对账。默选手动刷新，不跟本机会话的 1/5/10 分钟定时器；要定时联网须在设置 Cursor 页打开独立开关。界面可翻看已缓存的单条事件，对不上本机会话。概览页单独展示缓存摘要（跟随当前时间/模型筛选），仍不并入本机 token KPI。例外：概览「7 天滚动用量」、来源统计、使用统计、项目统计可挂 Cursor 账号用量，费用按用户价目、缺价时用 LiteLLM 快照按模型估算；来源统计不把该行并进页顶本机效率卡片，使用统计按时间桶叠加进趋势，项目统计单独成一行（账号用量无 cwd）。
_Avoid_: 把它叫成本机用量、消耗记录，或与代码量混称；不要把它并进本机 token KPI、5 小时窗或报告的 token 总数

**Cursor 会话 (Cursor Session)**：
从本机 `~/.cursor/projects/*/agent-transcripts` jsonl 解析的跨会话行为统计（会话数、轮次、工具调用、失败率、提问数、工具分类等）。`subagents/` 子代理并入父会话，不单独计数。Cursor IDE Agent 与 `cursor-agent` CLI 写同一套目录，无法从路径区分来源；hash 的 `source` 可标 composer/cli。独立仪表盘只展示聚合，不含对话正文，不进总览 token KPI。单条会话的正文、工具事件、用量、读写路径与 hash 文件走对话记录。工作时间线按 `first_seen_at` / `last_seen_at` 把本机会话铺成当天片段，不进消耗记录、不贡献本机 token KPI。
_Avoid_: 与消耗记录、对话记录、代码量混称；不要把 `~/.cursor-agent-usage` 当成官方会话目录；不要把子代理 jsonl 当成独立会话；不要把账号用量或代码量画进工作时间线

**对话记录 (Conversation Record)**：
本机会话目录：索引元数据，详情按页读事件索引（正文在 `conversation_events`，ADR 0011）。目录搜索可命中标题与已索引正文（FTS 派生缓存，不进备份、不上传）。Cursor Agent 与其它来源共用同一目录；Cursor 单条行为聚合挂在对话详情上，不另开一份正文索引。
_Avoid_: 消耗记录、Cursor 会话仪表盘；不要把正文送进备份或上传

**对话记录适配器 (Conversation Adapter)**：
把某个 Source 的原始会话文件解析成对话记录（目录行与语义事件）的模块。与把同一来源变成消耗记录的适配器是两回事：各一张表，互不替代。
_Avoid_: parser、解析器、插件；不要省略「对话记录」只叫适配器（那条特指消耗记录）

**官方额度 (Official Quota)**：
账号级订阅限额（已用百分比、重置时间，以及按连续两次官方快照估计的撞线时间）。成员由两部分构成：内置九家账号（Claude / Codex / Cursor / Grok / Droid / Antigravity / OpenCode / Copilot / Devin）与用户自行登记的**自定义提供商**。独立于消耗记录、本机 5 小时/7 天估计窗、Cursor 账号用量与代码量，不并入本机 token KPI。每行可带套餐名（接口原值经 `display_plan_label` 归一）。新鲜度分 official / stale / unavailable；取数失败保留上次正确缓存。凭证一律读各客户端本机已有的登录态，不要求用户粘贴：Claude 来自 statusline 捕获，Codex 问本机 app-server，Cursor 读 globalStorage `state.vscdb`，Grok 读 `~/.grok/auth.json`，Antigravity 先读 macOS 钥匙串再回落客户端本机状态。本机既没凭证、也没历史缓存的账号不占一行。预计撞线只由官方前后两拍的百分比差计算，文案写成估计，不和官方进度条混成一根，也不用本机 5 小时燃烧去填官方百分比。
_Avoid_: 把它叫成本机计费窗、消耗记录，或与本机 5 小时/7 天估计混成同一根进度条；不要并进报告的 token 总数

**自定义提供商 (Custom Quota Provider)**：
用户在设置页自行登记的、按内置预设类型取数的账号级额度来源（第三方 API 中转站、聚合服务）。属于「官方额度」维度，与消耗记录、本机 5 小时/7 天估计窗、Cursor 账号用量、代码量互不相干，不进本机 token KPI。标识随机生成、带 `custom:` 前缀，与内置账号永不冲突；名称是纯展示标签，改名不改标识，额度缓存与告警去重记录跟着标识走。配置与密钥分两份文件存，密钥不进备份。每条有启用开关：关掉就不取数、不占首页与托盘、不参与额度告警，名称 / 类型 / 地址 / 密钥都留着；自定义提供商不进首页「配置显示」。取数只走内置预设类型的解析器，只打计费/余额接口。已实现档：OpenAI 兼容计费、其别名 NewAPI / OneAPI、LiteLLM Proxy。托盘「最紧一档」按窗口有无重置时间分流，不按 `custom:` 前缀一刀切。见 `docs/adr/0012-custom-quota-providers.md`、`docs/adr/0013-custom-quota-implemented-presets.md`。
_Avoid_: 中转站、渠道、来源 (Source)（后者特指有本地会话数据的 AI 工具）

**LiteLLM Proxy**：
用户自建 LiteLLM Proxy 网关上那把 virtual key 的预算窗口，属于官方额度维度的自定义提供商预设。
_Avoid_: 把它和「LiteLLM 价目快照」当成同一个东西。价目快照属于费用维度，是社区维护的公开模型单价、作为费用推导的兜底层。两者语义无关。用户会在设置页的「额度」页和「费用」页各看到一个 LiteLLM。

**LiteLLM 价目快照 (LiteLLM Price Snapshot)**：
社区维护的公开模型单价，属于费用维度，作为来源自带费用、用户价目之后的费用推导兜底层。界面与代码里也常简称「LiteLLM 快照」。
_Avoid_: 把它和「LiteLLM Proxy」当成同一个东西。LiteLLM Proxy 属于官方额度维度，是用户自建网关上那把 virtual key 的预算窗口。两者语义无关。

**工作时间线 (Work Timeline)**：
单日会话区间铺开。消耗记录按 `occurred_at` 聚成横条；Cursor 本机会话按起止时间并入同一天，不把账号用量或代码量画上去。Token 与对话轮次仍只统计当天消耗记录。
_Avoid_: 把它当成又一份 token KPI，或把 Cursor 会话伪造成消耗记录

**报告 (Report)**：
某个已结束的完整自然周期内、仅基于消耗记录的可分享汇总，形态是一张竖版长图。不是独立数据维度，不是又一份 token KPI，也不是仪表盘的另一种排版。token 与费用只来自消耗记录；代码量、官方额度、Cursor 账号用量本期不出现在海报上，更不得并进总数。洞察在 Rust 侧产生，前端只措辞与排版。本期只做周报、只写剪贴板。见 `docs/adr/0015-report-and-insights.md`。
_Avoid_: 摄取报告（那是 `IngestReport`）；滚动 7 天（那是计费窗）；把报告叫成导出或仪表盘截图

**洞察 (Insight)**：
报告中的一条结构化事实（`kind` + 数值 payload），由 Rust 规则引擎产生。payload 不含自然语言；措辞不属于洞察本身。
_Avoid_: 评语、文案、headline；不要在 webview 里现算洞察

**全局指令 (Global Instruction)**：
某个 Source 会跨项目加载的、由用户手写的自定义指令文本。独立于消耗记录、代码量、Cursor 会话与官方额度，不并入本机 token KPI。判定口径是「该 Source 真正会加载的」，不是磁盘上有哪些 markdown。Cursor 遗留 memories、Claude 自动记忆是机器写的残渣，只可作体检项，不进本词条。
_Avoid_: 规则、rules（会和本仓库的项目规则撞名）；记忆、memory（会和 Claude 自动记忆、Cursor 残留 memories 撞名）；提示词

## 采集源现状

| Source | 存储 | 本机 token | 本机费用 |
|--------|------|:---:|:---:|
| Codex | jsonl `~/.codex/sessions` | ✅ | ❌ |
| Claude Code | jsonl `~/.claude/projects` | ✅ | ✅ 自带 `costUSD` |
| pi | jsonl `~/.pi/agent/sessions` | ✅ | ✅ 自带 |
| OMP | jsonl `~/.omp/agent/sessions` | ✅ | ✅ 自带 `cost.total` |
| dsh | zstd jsonl `~/.dsh/sessions` | ✅(需解压) | ❌ |
| opencode | sqlite+json `~/.local/share/opencode` | ✅ | ✅ 自带 |
| kimi | jsonl `~/.kimi/sessions/*/wire.jsonl` | ✅ | ❌ |
| gemini | json `~/.gemini/tmp/*/chats/session-*.json` | ✅ | ❌ |
| grok | `~/.grok/sessions` | ✅（`turn_completed.usage`） | ✅ 自带 `costUsdTicks` |
| Hermes | sqlite `~/.hermes/state.db`（`session_model_usage`） | ✅（模型级累计） | ✅ 自带 `actual_cost_usd` |
| qwen | `~/.qwen/tmp/*/logs.json` | ❌（本地无 Token） | ❌ |
| Factory/droid | `~/.factory/sessions/**/<id>.jsonl` 正文 + `<id>.settings.json` 累计用量 | ✅（会话累计、无模型名） | ❌ |
| Cursor | sqlite（代码量）+ 账号级 token（联网）+ 会话 transcript（行为统计） | ⚠️ 账号级（默选手动，可独立自动刷新） | ❌ |
| cursor-agent | 会话与 IDE 共用 `~/.cursor/chats` + `agent-transcripts`；token 仅无头 stdout（需包装落盘到 `~/.cursor-agent-usage`） | ⚠️（仅包装） | ❌ |
| copilot | jsonl `~/.copilot/session-state/<id>/events.jsonl` | ✅（仅会话结束时，按模型累计） | ❌ |
| amp | 本机仅配置 | ❌（云端） | ❌ |

以上是各 Source 的默认扫描路径；每个 Source 都可以用设置页绝对路径或环境变量整体覆盖（逗号分隔可指定多个目录，同时扫描），用于非默认安装位置或多份数据目录。设置页优先于环境变量，从 Dock 打开也能生效。默认路径与对应环境变量见 `docs/adr/0005-configurable-source-paths.md`。Claude Code 默认会同时扫 `~/.claude/projects` 和 XDG 路径 `~/.config/claude/projects`。Cursor 账号用量见 `docs/adr/0006-cursor-account-usage-network-ingest.md`，Cursor 会话见 `docs/adr/0007-cursor-session-local-ingest.md`。全局指令见 `docs/adr/0009-global-instruction-dimension.md`；写入用户文件的约束见 `docs/adr/0010-writing-user-owned-files.md`。报告与洞察见 `docs/adr/0015-report-and-insights.md`。
