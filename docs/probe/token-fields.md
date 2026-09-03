# 探测结果：各来源 token 字段与 Usage Record 映射

实测时间：2026-08-16。Hermes 行于 2026-09-02 补入（详见 [`hermes.md`](hermes.md)）。只记录元数据字段位置，不摘录会话正文。

## 消耗记录 (Usage Record) 定稿

| 字段 | 类型 | 说明 |
|------|------|------|
| `occurred_at` | RFC3339 | 该轮发生时间 |
| `source` | 文本 | 来源：codex / claude / pi / omp / opencode / kimi / dsh / gemini / grok / qwen / factory / cursor_agent / copilot / hermes |
| `model` | 文本 | 模型 ID |
| `provider` | 文本 | 官方或中转；未知则为空 |
| `project` | 文本 | 工作目录（解码后的路径） |
| `session_id` | 文本 | 会话 ID |
| `source_file` | 文本 | 原始文件定位 |
| `input_tokens` | i64 | 输入 |
| `output_tokens` | i64 | 输出 |
| `cache_read_tokens` | i64 | 缓存读 |
| `cache_creation_tokens` | i64 | 缓存写/创建 |
| `reasoning_tokens` | i64 | 推理 |
| `total_tokens` | i64 | 总量；来源未给时按各口径之和 |
| `native_cost` | f64? | 来源自带费用（pi / opencode / hermes 等） |

不含会话正文。

复跑探测：

```bash
cargo run --bin probe --manifest-path src-tauri/Cargo.toml
```

## sqlite schema

```sql
CREATE TABLE IF NOT EXISTS usage_records (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    occurred_at TEXT NOT NULL,
    source TEXT NOT NULL,
    model TEXT NOT NULL,
    provider TEXT NOT NULL,
    project TEXT NOT NULL,
    session_id TEXT NOT NULL,
    source_file TEXT NOT NULL,
    input_tokens INTEGER NOT NULL,
    output_tokens INTEGER NOT NULL,
    cache_read_tokens INTEGER NOT NULL,
    cache_creation_tokens INTEGER NOT NULL,
    reasoning_tokens INTEGER NOT NULL,
    total_tokens INTEGER NOT NULL,
    native_cost REAL,
    archived_at TEXT
);

CREATE TABLE IF NOT EXISTS ingested_files (
    path TEXT PRIMARY KEY,
    mtime_ms INTEGER NOT NULL,
    size INTEGER NOT NULL,
    source TEXT NOT NULL DEFAULT '',
    fingerprint TEXT NOT NULL DEFAULT '',
    adapter_version INTEGER NOT NULL DEFAULT 0
);
```

## 字段映射

| Source | 本机路径 | 是否含 token | 口径映射 | 模型 / provider / 项目 / 会话 | 去重口径 |
|--------|----------|:---:|----------|------------------------------|----------|
| Codex | `~/.codex/sessions/**/*.jsonl` | 是 | 优先 `last_token_usage`：input_tokens→input，cached_input_tokens 或 cache_read_input_tokens→cache_read（钳制 ≤ input），output_tokens→output，reasoning_output_tokens→reasoning，total_tokens→total。无 last 时对 `total_token_usage` 做会话高水位差分 | 模型：`turn_context.payload.model` 或 `info.model`；provider：`session_meta.payload.model_provider`；项目：`session_meta.cwd`；会话：`session_meta.id` | 优先单轮 last；相同快照去重；全零跳过 |
| Claude Code | `~/.claude/projects/<dashed-cwd>/<session>.jsonl` | 是 | assistant `message.usage`：input_tokens / output_tokens / cache_read_input_tokens / cache_creation_input_tokens；费用：事件根级 `costUSD`（>0 才作 native_cost） | 模型：`message.model`；项目：目录名解码；会话：`sessionId` 或文件名 | 同一 `message.id` 一条（优先 stop_reason，否则 output 更大）；全零跳过 |
| pi | `~/.pi/agent/sessions/<dashed-cwd>/*.jsonl` | 是 | assistant `message.usage`：input / output / cacheRead / cacheWrite / reasoning / totalTokens；费用：`usage.cost.total` | 模型：`message.model`；provider：`message.provider` 或最近的 `model_change.provider`；项目：session.cwd 或目录名；会话：session.id | 每条 assistant 一条；采用自带 cost |
| OMP | `~/.omp/agent/sessions/<dashed-cwd>/*.jsonl`（含子代理子目录 jsonl） | 是 | 同 Pi：assistant `message.usage`；费用：`usage.cost.total`。子代理 token 计入父会话 `session_id` | 模型：`message.model`；provider：`message.provider` 或 `model_change.model` 的 `/` 前缀；项目：session.cwd；会话：父会话 `session.id` | 每条 assistant 一条；全零跳过；对话目录不单独列子代理 |
| opencode | `~/.local/share/opencode/opencode.db` 的 `message.data` | 是 | assistant `tokens.input/output/reasoning`，`tokens.cache.read/write`；费用：`cost`（>0 才作 native_cost） | 模型：`modelID`；provider：`providerID`；项目：`path.root` 或 `path.cwd`；会话：`session_id` | 仅已完成 assistant（有 `time.completed`）；全零跳过 |
| kimi | `~/.kimi/sessions/<hash>/<uuid>/wire.jsonl` | 是 | `StatusUpdate.payload.token_usage`：input_other→input，output→output，input_cache_read→cache_read，input_cache_creation→cache_creation | 模型：未知则空（wire 未稳定暴露）；项目：`kimi.json` work_dirs 或上层 hash 目录；会话：目录 uuid | 同一 `message_id` 的 StatusUpdate 只保留最后一次（累进更新） |
| dsh | `~/.dsh/sessions/<dashed-cwd>/**/session.jsonl.zstd` | 是 | 解压后 `assistant/message.data.usage`：inputTokens / outputTokens / cacheReadTokens / reasoningTokens。忽略流式 `assistant/chunk` | 模型/provider：`data.message.source` 或最近 `request/header.config`；项目：session.cwd；会话：session.id | 只用最终 `assistant/message` |
| gemini | `~/.gemini/tmp/*/chats/session-*.json` | 是 | 消息 `type=gemini` 的 `tokens`：input / output / cached→cache_read / thoughts→reasoning / total。`logs.json` 无 token | 模型：`message.model`；项目：tmp 子目录名或 `projectHash`；会话：`sessionId` | 只收 `type=gemini`；全零跳过 |
| grok | `~/.grok/sessions/<url-encoded-cwd>/<session-id>/updates.jsonl` | 是 | 优先 `sessionUpdate=turn_completed` 的 `usage`：inputTokens→input，outputTokens→output，cachedReadTokens→cache_read，cacheCreationTokens→cache_creation，reasoningTokens→reasoning，totalTokens→total；`costUsdTicks`（1 tick=1e-10 USD）→native_cost。input 含 cache read，reasoning ⊂ output。无 turn_completed 时才回退 `_meta.totalTokens`（仅上下文占用） | 模型：`usage.modelUsage` 的键，或 summary `current_model_id`；项目：url-decode 目录名；会话：目录 uuid | 每 prompt_id+model 一条（最后一次 turn_completed），不按 chunk / 上下文占用累加 |
| qwen | `~/.qwen/tmp/*/logs.json` | 否 | 本机仅有 user 文本，无 token 字段。适配器输出空列表 | 会话：`sessionId`；模型 / provider / 项目：文件中不存在 | 无可计用量 |
| factory | `~/.factory/sessions/**/<id>.settings.json` 的 `tokenUsage` | 是（会话累计） | jsonl 正文无 per-turn usage。`tokenUsage`：inputTokens / outputTokens / cacheCreationTokens / cacheReadTokens / thinkingTokens | provider：`providerLock`；项目：dashed 目录名解码；会话：文件名前缀 uuid | 每会话一条累计记录（本机无轮级口径） |
| Cursor | `~/.cursor/ai-tracking/ai-code-tracking.db` 的 `scored_commits` | 否（代码量） | `linesAdded`/`linesDeleted` / `composer*` / `tab*` / `human*` / `v2AiPercentage`。**不进入 Usage Record**。AI 占比只用 composer÷added | — | 独立代码量面板 |
| cursor-agent | 无头 stdout `stream-json`；本机 `store.db` / transcript 无 token | 是（仅 stdout） | `result.usage`：inputTokens / outputTokens / cacheReadTokens / cacheWriteTokens。reasoning / 费用：无 | 模型：`system.model`；项目：`system.cwd`；会话：`session_id` | 只取 `type=result`；`request_id` 去重。hook / 本机文件不可用 |
| copilot | `~/.copilot/session-state/<session-id>/events.jsonl` | 是（仅会话结束时，按模型累计） | `session.shutdown.data.modelMetrics.<model>.usage`：inputTokens / outputTokens / cacheReadTokens→cache_read、cacheWriteTokens→cache_creation。reasoning：无 | 模型：`modelMetrics` 的键；项目：`session.start.data.context.cwd`；会话：`session.start.data.sessionId`（缺失时退回父目录名） | 只取文件里时间最晚的一次 `session.shutdown`（会话续接会写多次，均为累计值，不能叠加）。详见 `docs/probe/copilot.md` |
| Hermes | `~/.hermes/state.db` 的 `session_model_usage` JOIN `sessions` | 是（模型级累计） | `input_tokens` / `output_tokens` / `cache_read_tokens` / `cache_write_tokens`→cache_creation / `reasoning_tokens`。费用：`cost_source` 非空且非 `none` 且 `actual_cost_usd > 0` 时取 `actual_cost_usd`。`estimated_cost_usd` 不用 | 模型：`session_model_usage.model`；provider：`billing_provider`；项目：`sessions.cwd`，空则 `git_repo_root`；会话：`session_id`；时间：`sessions.started_at`（unix 秒） | 主键六元组每行一条，同 session 多模型拆行。详见 `docs/probe/hermes.md` |
| amp | 本机仅配置 | 否 | 不纳入 | — | — |

## dsh 解压后结构

`session.jsonl.zstd` 为 zstd 压缩 jsonl。首行 `type=session`（id / cwd / createdAt）。后续事件含 `request/header`（provider/model）、流式 `assistant/chunk`（带 usage）、最终 `assistant/message`（`data.usage` + `data.message.source`）。统计只取最终 message。
