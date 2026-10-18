# 来源覆盖扩展路线图

> 背景：CodexBar / CodeBurn / Token Monitor / TokenTracker 等竞品已扫描的来源里，本项目还没有覆盖或覆盖很弱的有 OpenClaw、Zed、Antigravity 本地 transcript、Cline/Roo/Kilo、Cherry Studio、Qoder CN、Windsurf；国内向的有 CodeBuddy/WorkBuddy/ZCode/Trae CN。另外已接入的 qwen/gemini/factory/copilot 四个来源字段有残缺。本文档记录一次本机 probe 后的优先级排序，供后续排期实现。
>
> **进度更新（2026-09-03）**：原 Tier 1 第一名 **Hermes 已实现并合并**（PR #145，`adapters/hermes.rs` + `USAGE_ADAPTERS` 表 `Source::Hermes`，coverage「模型级 Token（含原生费用）」），已从下方待新增列表移除，见「已实现」小节。当前 Tier 1 待新增顺序为 VS Code 系 Cline/Roo/Kilo → ZCode → WorkBuddy。
>
> **进度更新（2026-10-10）**：**Cline CLI**（`~/.cline/data/sessions/<id>/<id>.messages.json`）已作为独立 Usage Source 接入，见「已实现」小节。下文 Tier 1 的「Cline / Roo / Kilo」仍指 VS Code 扩展的 `ui_messages.json` 路径，与 Cline CLI 不是同一套落地文件。
>
> **进度更新（2026-10-18）**：**Goose**（`~/.local/share/goose/sessions/sessions.db`）已作为独立 Usage Source 接入，见「已实现」小节。
>
> **进度更新（2026-10-18）**：**Roo Code** VS Code 扩展（`globalStorage/rooveterinaryinc.roo-cline/tasks/*/ui_messages.json`）已作为独立 Usage Source 接入，见「已实现」小节。下文 Tier 1 的「Cline / Roo / Kilo」里 Roo 已落地；Cline 扩展仍待接入。
> **进度更新（2026-10-18）**：**Kilo Code** VS Code 扩展（`globalStorage/kilocode.kilo-code/tasks/*/ui_messages.json`）已作为独立 Usage Source 接入，见「已实现」小节。下文 Tier 1 的「Cline / Roo / Kilo」里 Kilo 已落地；Cline 扩展仍待接入。
> **进度更新（2026-10-18）**：**Amp**（`~/.local/share/amp/threads/*.json`）已作为独立 Usage Source 接入，见「已实现」小节。字段对齐 ccusage Amp parser 与 `amp threads export` 的线程 JSON。
> **进度更新（2026-10-18）**：**Zed**（`threads.db` 的 zstd/json 线程 blob）已作为独立 Usage Source 接入，见「已实现」小节。原 Tier 3「本机未安装」结论被上游源码与 codeburn 口径推翻。
> **进度更新（2026-10-18）**：**Cherry Studio**（Electron userData `cherrystudio.sqlite` 的 `ai_usage_record`）已作为独立 Usage Source 接入；原 Tier 3「IndexedDB」结论过时，v2 已落 sqlite。
> **进度更新（2026-10-18）**：**OpenClaw**（`~/.openclaw/agents/*/sessions/*.jsonl`）已作为独立 Usage Source 接入。
> **进度更新（2026-10-18）**：**CodeBuddy IDE**（`CodeBuddyExtension/Data/**/history/**/messages/*.json` 的累计 `statsSnapshot`）已作为独立 Usage Source 接入，品牌复用 CodeBuddy。
> **进度更新（2026-10-18）**：**Crush**（`projects.json` 指向的各项目 `crush.db`）已作为独立 Usage Source 接入。
> **进度更新（2026-10-18）**：**Aider**（只读用户 `--analytics-log` JSONL）已作为独立 Usage Source 接入，见「已实现」小节。

**排序方法论**：以「实现成本 / 数据形态可行性」（ROI）为主轴排序，而不是先按业务重要性或竞品对齐程度排。数据形态越接近「结构化字段」越优先；需要逆向 Electron LevelDB/IndexedDB 的一律放到不投入档。

**范围声明**：本文档只做优先级排序和可行性记录，不是实施计划；不包含 RED/GREEN 步骤（那类文档见 `docs/superpowers/plans/`）。所有路径/字段来自 2026-09-02 对本机 `~` 目录的一次性 probe，样本量很小（多数来源只有个位数会话），真正实现前仍需按 `docs/agent-layers.md` 的 Adapter 节补脱敏 fixture 并跑 `cargo run --bin probe` 确认字段稳定性。

## Tier 1 — 高可行性，建议按此顺序新增来源

### 1. Cline / Roo / Kilo（同一 fork 架构）

- **本机路径**：VS Code 系编辑器的 `globalStorage/<extension-id>/tasks/<task-uuid>/`，本机已验证 `rooveterinaryinc.roo-cline`（Roo Code）；Cline 原版扩展 ID 通常是 `saoudrizwan.claude-dev`，Kilo Code 是 `kilocode.kilo-code`——本机未装，需要另外找 fixture 确认路径一致
- **关键文件**：`ui_messages.json` 是消息数组，其中 `say == "api_req_started"` 的记录 `text` 字段是一段 JSON 字符串，内含 `tokensIn`/`tokensOut`/`cacheWrites`/`cacheReads`/`cost`；`api_conversation_history.json` 只有 `role/content/ts`，不含 token
- **架构决策**：三者数据格式完全一致，注册为 **3 个独立 Source**（Cline/Roo/Kilo），共享同一个解析函数，只在 `USAGE_ADAPTERS` 表里用不同的扩展路径区分（符合 ADR 0001 的路径级区分原则，且界面上可分别看到各自用量）
- **待确认**：model 字段目前没有在 `ui_messages.json` 里直接看到（`api_req_started` 只有 token/cost，没有 model id），需要确认模型名是从 `task_metadata.json`（本机没有）还是从扩展的 `taskHistory`（存在 VS Code 的 `state.vscdb` 里，需要额外读取）取得；如果拿不到 model，会重复 factory 的「无模型名」问题

### 2. ZCode

- **本机路径**：`~/.zcode/cli/rollout/model-io-sess_*.jsonl`
- **格式**：标准 JSONL，每行一次请求/响应，成功记录的 `response.usage.{inputTokens,outputTokens,totalTokens,cacheReadTokens,cacheWriteTokens}` 和 `model.modelId`/`model.providerId` 都是顶层字段，失败记录用 `error` 字段标记（无 usage，需要跳过或按 0 token 处理）
- **为什么排这**：格式最标准（不用解析嵌套字符串），但没有原生费用字段，比已实现的 Hermes 和 Cline 家族价值略低
- **待确认**：`sessionId`/`traceId`/`turnId` 三者的去重粒度，避免同一轮请求的 retry（`attempt` 字段）被重复计数

### 3. WorkBuddy

- **本机路径**：`~/.workbuddy-ai/traces/<pid>/trace_*.json`
- **格式**：`{trace, spans}` 结构（类 OpenTelemetry）。`trace.totalTokens` 顶层字段目前观察到恒为 0（不可用），真实 token 藏在 `type == "generation"` 的 span 的 `toolOutput` 字段里——这是一段 OpenAI `chat.completion` 格式的 JSON 字符串，包含 `model` 和 `usage.{prompt_tokens,completion_tokens,total_tokens,prompt_tokens_details.cached_tokens,completion_tokens_details.reasoning_tokens}`
- **待确认**：`toolOutput` 有时是数组包一层（`[{...}]`），需要处理非 chat.completion 格式的其他 span（比如走的是非 OpenAI 兼容通道时的字段名差异）；project/cwd 字段目前没在 trace/span 层看到，可能要从 `rum-electron-store` 或其他侧车文件补

## Tier 2 — 阻塞待样本，暂不排具体优先级

| 来源 | 状态 |
|---|---|
| OpenClaw | 已作为独立 Usage Source 接入（`adapters/openclaw.rs`，jsonl） |
| Qoder CN | 已作为独立 Usage Source 接入（`adapters/qoder_cn.rs`，Claude 形态 jsonl） |

排期前提：拿到任一渠道（自己装、社区 fixture、官方文档）的真实本机会话样本后，重新走一次本文档的 probe 流程再决定归入 Tier 1 还是 Tier 3。

## 已实现（原 Tier 1）

### Hermes ✅（PR #145）

- **本机路径**：`~/.hermes/state.db`（SQLite），环境变量 `HERMES_HOME` 可覆盖
- **实现**：`src-tauri/src/adapters/hermes.rs`，在 `USAGE_ADAPTERS` 表注册为 `Source::Hermes`，coverage「模型级 Token（含原生费用）」；按 `session_model_usage`（`session_id + model + billing_provider` 维度）累计 token，取 `actual_cost_usd` 为原生费用；缺列时回落默认值，不让整个来源失败
- **为什么当时排第一**：所有候选里唯一自带原生费用字段（`actual_cost_usd`）的来源，且是结构化 SQLite，直接 SQL 映射到 `UsageRecord`
- **落地时的取舍**：以模型级累计（`session_model_usage`）为准，未按会话级（`sessions`）再计一遍，避免会话级 vs 模型级重复计数；`cost_source` 枚举仍以本机样本为限，后续拿到更多真实会话可再校验

### Amp ✅

- **本机路径**：`~/.local/share/amp/threads/**/*.json`（另扫 macOS Application Support 与 Windows AppData）；`AMP_DATA_DIR` 覆盖根并拼 `threads/`
- **实现**：`src-tauri/src/adapters/amp.rs` + `conversation/amp.rs`，注册为 `Source::Amp`，coverage「轮级 Token（input 不含 cache）」；优先 `usageLedger.events`，缺则回落 `messages[].usage`；ledger cache 经 `toMessageId` 对助手消息；credits 不是美元，不写 `native_cost`
- **说明**：当前 Amp CLI 以云端线程为准，本机 `threads/` 来自仍落盘的本地优先版本或 `amp threads export`。没有文件时只是未检测到，不报失败

### Cline CLI ✅

- **本机路径**：`~/.cline/data/sessions/<id>/<id>.messages.json`，侧车 `<id>.json` 提供 `cwd` / `metadata.fork.forkedAt`；环境变量 `CLINE_SESSION_DATA_DIR` 直接覆盖会话目录
- **实现**：`src-tauri/src/adapters/cline.rs`，注册为 `Source::Cline`，coverage「轮级 Token（input 不含 cache）」；`input = max(0, inputTokens - cacheRead - cacheWrite)`，`metrics.cost > 0` 写入 `native_cost`；`ts < forkedAt` 的消息跳过
- **与 VS Code Cline 的区别**：这是 Cline CLI 的本机会话目录，不是扩展 `globalStorage/.../tasks/*/ui_messages.json`

### Goose ✅

- **本机路径**：`~/.local/share/goose/sessions/sessions.db`（另扫 macOS Application Support 与 Block 旧路径）；`GOOSE_PATH_ROOT` 覆盖根并拼 `data/sessions`
- **实现**：`src-tauri/src/adapters/goose.rs` + `conversation/goose.rs`，注册为 `Source::Goose`，coverage「会话级 Token」；取 `accumulated_*`（缺则回落会话级 input/output/total）；`session_type = hidden` 不入账；对话读 `messages.content_json` 的 text / toolRequest / toolResponse

### Roo Code ✅

- **本机路径**：VS Code / Insiders / VSCodium / Cursor / Windsurf / Trae / vscode-server 的 `User/globalStorage/rooveterinaryinc.roo-cline/tasks/<id>/`；`ROO_TASKS_DIR` 直接指向 tasks 目录
- **实现**：`src-tauri/src/adapters/roo.rs` + 共用 `adapters/extension_tasks.rs` + `conversation/roo.rs`，注册为 `Source::Roo`，coverage「轮级 Token（input 不含 cache）」；`say == "api_req_started"` 嵌套 JSON 的 `tokensIn` 是含 cache 的 `totalInputTokens`（Roo PR #8954），入账 `input = tokensIn − cacheReads − cacheWrites`；模型按 `api_conversation_history.json` 用户消息里的 `<model>` 标签顺序配对；`cost > 0` 写入 `native_cost`；对话读同目录 `api_conversation_history.json`，剥 `<environment_details>`
### Kilo Code ✅

- **本机路径**：VS Code / Insiders / VSCodium / Cursor / Windsurf / Trae / vscode-server 的 `User/globalStorage/kilocode.kilo-code/tasks/<id>/`；`KILO_TASKS_DIR` 直接指向 tasks 目录
- **实现**：`src-tauri/src/adapters/kilo.rs` + 共用 `adapters/extension_tasks.rs` + `conversation/kilo.rs`，注册为 `Source::Kilo`，coverage「轮级 Token（input 不含 cache）」；`tokensIn` 含 cache，入账时拆出；`usageMissing=true` 跳过该轮；无 `apiProtocol` 时回落 `inferenceProvider`；对话读 `api_conversation_history.json`
### Zed ✅

- **本机路径**：Linux `~/.local/share/zed/threads/threads.db`，macOS `~/Library/Application Support/Zed/threads/threads.db`，Windows `%LOCALAPPDATA%/Zed/threads/threads.db`；`ZED_DATA_DIR` 覆盖数据根并拼 `threads/`
- **实现**：`src-tauri/src/adapters/zed.rs` + `conversation/zed.rs`，注册为 `Source::Zed`，coverage「轮级 Token（input 不含 cache）」；`data_type` 为 `zstd`/`json` 才解 blob，未知类型跳过；`request_token_usage` 按用户消息键出轮，累计大于 map 时补 `cumulative-remainder`（codeburn 在真实线程上观察到 map 可能只有累计的约 1/3）；input 已不含 cache（Anthropic 形）；对话读外部标签 `User`/`Agent`（text / thinking / tool_use / tool_results），`parent_id` 非空不算顶层
- **证据**：上游 `crates/agent/src/db.rs` 的 `DbThread` / `TokenUsage` / `DataType`，以及 codeburn `src/providers/zed.ts`

### Cherry Studio ✅

- **本机路径**：Electron userData `cherrystudio.sqlite`（Linux `~/.config/{CherryStudio,Cherry Studio}`，macOS Application Support，Windows `%APPDATA%`）；`CHERRY_STUDIO_DATA_DIR` 覆盖 userData 根
- **实现**：`src-tauri/src/adapters/cherry.rs` + `conversation/cherry.rs`，注册为 `Source::Cherry`，coverage「轮级 Token（input 不含 cache）」；读 v2 `ai_usage_record`（PR CherryHQ/cherry-studio#15992）；优先 `no_cache_tokens`，否则从 `input_tokens` 拆 cache；`cost_currency = USD` 才入 `native_cost`；无模型名跳过。对话读 `topic` + `message`（AI SDK `parts`；`role = root` 与 `deleted_at` 非空不进目录）

### OpenClaw ✅

- **本机路径**：`$OPENCLAW_STATE_DIR/agents/<agent>/sessions/`（默认 `~/.openclaw`，另扫 `~/.clawdbot`）
- **实现**：`src-tauri/src/adapters/openclaw.rs` + `conversation/openclaw.rs`，注册为 `Source::OpenClaw`，coverage「轮级 Token（input 不含 cache）」；assistant `message.usage` 的 input/output/cacheRead/cacheWrite/reasoningTokens；`cost.total > 0` 写入 `native_cost`；含 `.jsonl.reset.*` / `.jsonl.deleted.*`；跳过 compaction checkpoint、`.bak`、trajectory、`delivery-mirror`
### CodeBuddy IDE ✅

- **本机路径**：`CodeBuddyExtension/Data/**/history/<session>/<conversation>/messages/*.json`（Linux `~/.config`、macOS Application Support、Windows `%APPDATA%` / `%LOCALAPPDATA%`）；`CODEBUDDY_IDE_DIR` 覆盖 Data 根
- **实现**：`src-tauri/src/adapters/codebuddy_ide.rs` + `conversation/codebuddy_ide.rs`，注册为 `Source::CodeBuddyIde`，coverage「会话级 Token（input 不含 cache）」；每会话取最新 `statsSnapshot`（`cacheMissTokens` / `cachedInputTokens` / `lastOutputTokens` / `thinkingTokens`）；`credit` 是积分，不写入 `native_cost`
### Crush ✅

- **本机路径**：全局 `~/.local/share/crush/projects.json`（Windows `%LOCALAPPDATA%/crush`；`CRUSH_GLOBAL_DATA` 可覆盖）列出各项目 `data_dir`，用量在 `{data_dir}/crush.db`
- **实现**：`src-tauri/src/adapters/crush.rs` + `conversation/crush.rs`，注册为 `Source::Crush`，coverage「会话级 Token（含原生费用）」；只入账 `parent_session_id` 为空的根会话；`prompt_tokens` / `completion_tokens`；`cost > 0` 写入 `native_cost`；模型取 `messages.model`；对话读 typed `parts`（text / reasoning / tool_call / tool_result）
- **证据**：charmbracelet/crush `internal/projects/projects.go`、`internal/db/migrations/20250424200609_initial.sql`、`internal/db/sql/sessions.sql` 的 `ListSessions` / `UpdateSessionTitleAndUsage`
### Aider ✅

- **本机路径**：只读用户用 `--analytics-log` 主动打开的 JSONL。默认认 `~/.aider/analytics.jsonl`；`AIDER_ANALYTICS_LOG` 可覆盖。**不读、不写** `.aider.conf.yml`
- **实现**：`src-tauri/src/adapters/aider.rs`，注册为 `Source::Aider`，coverage「轮级 Token（含原生费用）」；只收 `event=message_send`；跳过 `main_model` 为 `None` 或 `*/REDACTED`；`cost` 写入 `native_cost`。analytics JSONL 没有会话正文，对话目录为空
- **证据**：Aider-AI/aider `aider/analytics.py` 的 `event()` 落盘格式，以及官方 `website/assets/sample-analytics.jsonl`
### Warp ✅

- **本机路径**：`warp.sqlite` 的 `agent_conversations`。Linux `~/.local/state/warp-terminal`；macOS Group Container Stable/Preview；Windows `%LOCALAPPDATA%/warp/Warp/data`。`WARP_HOME` 覆盖目录或库文件
- **实现**：`src-tauri/src/adapters/warp.rs`，注册为 `Source::Warp`，coverage「会话级 Token（无 input/output 拆分）」；`token_usage[].warp_tokens`（旧 `total_tokens`）+ `byok_tokens` + `custom_endpoint_tokens` 记入 input/total；`credits_spent` 不是 USD，不写入 `native_cost`。本机不存助手回复，对话目录为空
### Windsurf ✅

- **本机路径**：Devin Desktop / Windsurf 的 `User/acp-events/*.ndjson`；`WINDSURF_ACP_HOME` 覆盖该目录
- **实现**：`src-tauri/src/adapters/windsurf.rs`，注册为 `Source::Windsurf`，coverage「会话级 Token（ACP usage_update）」；input 累计含 cache read，入账时拆出；output 逐步相加。无模型名跳过。不读加密 Cascade `.pb`；额度%走已有 Devin 官方额度，不在本适配器

## Tier 3 — 本轮不投入

| 来源 | 原因 |
|---|---|
| Windsurf Cascade `.pb` / LevelDB | ACP `usage_update` ndjson 已作为独立 Usage Source 接入；加密 Cascade 与 Session Storage 仍不读 |
| Trae CN / Trae IDE | IDE 会话在 SQLCipher 加密库里，国际版用量走云端 API；**Trae CLI**（`~/.trae/cli/sessions` Codex rollout）已作为独立 Usage Source 接入 |
| Antigravity 本地 transcript | 同上，`Session Storage`/`blob_storage` 是 LevelDB（注意：Antigravity 的**官方额度**已经作为 `OfficialQuotaProvider` 接入，这里指的是本机会话明细，是另一件事） |
| Zed | 已作为独立 Usage Source 接入（`adapters/zed.rs`，读 `threads.db`） |

以上 Electron 系来源的共同问题是数据在 LevelDB/IndexedDB 里，需要引入第三方 leveldb 解析库 + 逆向 IndexedDB 内部序列化结构，工程成本远高于 Tier 1；如果之后有人验证到这几个工具会把会话导出为文件（比如某个「导出」功能），可以重新评估。Cherry Studio v2 已改写 sqlite，不再属于这一档。

## 已接入但字段残缺的四个来源 — 本轮结论

| 来源 | 残缺 | 结论 |
|---|---|---|
| qwen | chats jsonl 已接 | `projects/**/chats/*.jsonl` 的 `usageMetadata` 入账；`tmp/*/logs.json` 仍无 token，只作对话源 |
| gemini | 无费用 | **不算真问题**：`native_cost` 为 `None` 是预期行为，费用推导架构本身已经有价目表/LiteLLM 快照兜底（`cost.rs` 优先级链路），不需要额外适配器改动 |
| factory | 会话 settings 的 model；`custom:` 查 `~/.factory/settings.json`，否则剥 slot 后缀；`callingSessionId` 并入父会话 |
| copilot | 仅会话结束累计 | **本机无法验证**：这台机器没有安装/使用过 Copilot CLI（`~/.copilot` 下没有 `session-state` 目录），现有认知来自架构文档推导，不是本机实测；维持现状 |

## 已剔除

- **MiMo**：本机检索确认这是小米的模型名（如 `mimo-v2.5-pro`），被 opencode 等已接入来源当作可选模型调用，不是一个有独立本机会话落地的 AI CLI 客户端，不作为来源加入路线图。

## 后续步骤（真正排期实现某一项时）

1. 按 `docs/agent-layers.md` 的 Adapter 节做完每一项
2. 不确定字段先跑 `cargo run --bin probe`，结果写入 `docs/probe/`，尤其是 Cline 家族的 model 字段来源，本文档的判断样本量都很小
3. Cline/Roo/Kilo 三个 Source 共享解析函数时，注意在 `USAGE_ADAPTERS` 表里各自的 `coverage` 文案要分开写清楚，避免用户以为是同一个来源
