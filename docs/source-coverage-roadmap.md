# 来源覆盖扩展路线图

> 背景：CodexBar / CodeBurn / Token Monitor / TokenTracker 等竞品已扫描的来源里，本项目还没有覆盖或覆盖很弱的有 OpenClaw、Zed、Antigravity 本地 transcript、Cline/Roo/Kilo、Cherry Studio、Qoder CN、Windsurf；国内向的有 CodeBuddy/WorkBuddy/ZCode/Trae CN。另外已接入的 qwen/gemini/factory/copilot 四个来源字段有残缺。本文档记录一次本机 probe 后的优先级排序，供后续排期实现。
>
> **进度更新（2026-09-03）**：原 Tier 1 第一名 **Hermes 已实现并合并**（PR #145，`adapters/hermes.rs` + `USAGE_ADAPTERS` 表 `Source::Hermes`，coverage「模型级 Token（含原生费用）」），已从下方待新增列表移除，见「已实现」小节。当前 Tier 1 待新增顺序为 Cline/Roo/Kilo → ZCode → WorkBuddy。

**排序方法论**：以「实现成本 / 数据形态可行性」（ROI）为主轴排序，而不是先按业务重要性或竞品对齐程度排。数据形态越接近「结构化字段」越优先；需要逆向 Electron LevelDB/IndexedDB 的一律放到不投入档。

**范围声明**：本文档只做优先级排序和可行性记录，不是实施计划；不包含 RED/GREEN 步骤（那类文档见 `docs/superpowers/plans/`）。所有路径/字段来自 2026-09-02 对本机 `~` 目录的一次性 probe，样本量很小（多数来源只有个位数会话），真正实现前仍需按 `AGENTS.md` 的 Adapter 新增流程补脱敏 fixture 并跑 `cargo run --bin probe` 确认字段稳定性。

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
| OpenClaw | 本机无安装痕迹，无法 probe |
| Qoder CN | 本机无安装痕迹，无法 probe |

排期前提：拿到任一渠道（自己装、社区 fixture、官方文档）的真实本机会话样本后，重新走一次本文档的 probe 流程再决定归入 Tier 1 还是 Tier 3。

## 已实现（原 Tier 1）

### Hermes ✅（PR #145）

- **本机路径**：`~/.hermes/state.db`（SQLite），环境变量 `HERMES_HOME` 可覆盖
- **实现**：`src-tauri/src/adapters/hermes.rs`，在 `USAGE_ADAPTERS` 表注册为 `Source::Hermes`，coverage「模型级 Token（含原生费用）」；按 `session_model_usage`（`session_id + model + billing_provider` 维度）累计 token，取 `actual_cost_usd` 为原生费用；缺列时回落默认值，不让整个来源失败
- **为什么当时排第一**：所有候选里唯一自带原生费用字段（`actual_cost_usd`）的来源，且是结构化 SQLite，直接 SQL 映射到 `UsageRecord`
- **落地时的取舍**：以模型级累计（`session_model_usage`）为准，未按会话级（`sessions`）再计一遍，避免会话级 vs 模型级重复计数；`cost_source` 枚举仍以本机样本为限，后续拿到更多真实会话可再校验

## Tier 3 — 本轮不投入

| 来源 | 原因 |
|---|---|
| Cherry Studio | 数据在 Electron `IndexedDB`/`WebStorage`/`File System` 里，没有干净的会话文件；只有 `config.json`（应用配置，非用量） |
| Windsurf | 同上，`Session Storage` 是 LevelDB；`User/workspaceStorage` 下没找到独立于 VS Code 标准结构的 AI 对话文件 |
| Trae CN | 同上，本机只看到 `extensions`/`skills`/`argv.json`，没有会话数据落地文件 |
| CodeBuddy | `CodeBuddyExtension/Data/Public/auth` 只有鉴权数据，没有会话数据 |
| Antigravity 本地 transcript | 同上，`Session Storage`/`blob_storage` 是 LevelDB（注意：Antigravity 的**官方额度**已经作为 `OfficialQuotaProvider` 接入，这里指的是本机会话明细，是另一件事） |
| Zed | 本机 **未安装**（`/Applications/Zed.app` 不存在），`~/.config/zed/conversations` 是 2024-07 的残留数据，工具本身已经不在用，优先级低于「格式过时」 |

以上五个 Electron 系来源的共同问题是数据在 LevelDB/IndexedDB 里，需要引入第三方 leveldb 解析库 + 逆向 IndexedDB 内部序列化结构，工程成本远高于 Tier 1；如果之后有人验证到这几个工具会把会话导出为文件（比如某个「导出」功能），可以重新评估。

## 已接入但字段残缺的四个来源 — 本轮结论

| 来源 | 残缺 | 结论 |
|---|---|---|
| qwen | 无 token | **结构性死胡同**：本机 `~/.qwen/tmp/*/logs.json` 只有用户文本和 `sessionId`，`~/.qwen` 下其余文件都是配置/凭证，没有第二个可能藏 token 的文件。除非上游改格式，本机侧没有可挖的余地 |
| gemini | 无费用 | **不算真问题**：`native_cost` 为 `None` 是预期行为，费用推导架构本身已经有价目表/LiteLLM 快照兜底（`cost.rs` 优先级链路），不需要额外适配器改动 |
| factory | 无模型名 | **本轮没找到新线索**：`~/.factory` 下 `artifacts/tool-outputs/*.log` 文件名里能看到 provider 特征（如 `toolu_bdrk_...`），但没有找到逐次请求的模型名落地文件；值得以后单独花时间查 Factory CLI 是否有更细的日志开关或新版本格式 |
| copilot | 仅会话结束累计 | **本机无法验证**：这台机器没有安装/使用过 Copilot CLI（`~/.copilot` 下没有 `session-state` 目录），现有认知来自架构文档推导，不是本机实测；维持现状 |

## 已剔除

- **MiMo**：本机检索确认这是小米的模型名（如 `mimo-v2.5-pro`），被 opencode 等已接入来源当作可选模型调用，不是一个有独立本机会话落地的 AI CLI 客户端，不作为来源加入路线图。

## 后续步骤（真正排期实现某一项时）

1. 按 `AGENTS.md` 的「新增 / 修改 Adapter」清单：注册 `Source`、写 adapter、脱敏 fixture 放 `src-tauri/tests/fixtures/`、`tests/adapters.rs` 单测、必要时递增 `store.rs::ADAPTER_VERSION`
2. 不确定字段先跑 `cargo run --bin probe`，结果写入 `docs/probe/`，尤其是 Cline 家族的 model 字段来源，本文档的判断样本量都很小
3. Cline/Roo/Kilo 三个 Source 共享解析函数时，注意在 `USAGE_ADAPTERS` 表里各自的 `coverage` 文案要分开写清楚，避免用户以为是同一个来源
