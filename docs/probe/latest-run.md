# 探测程序 2026-08-16 / 2026-08-17 批次

本文件是 2026-08-16（`cargo run --bin probe`）与 2026-08-17（cursor-agent 脚本）的实测摘录，**不是**仓库里最新的探测记录。之后的探测：

- Hermes：2026-09-02，见 [`hermes.md`](hermes.md)
- 各来源字段总表：[`token-fields.md`](token-fields.md)
- 官方额度：[`official-quota.md`](official-quota.md)

时间：2026-08-17。只记录字段位置，不含会话正文。

## cursor-agent

命令：`python3 scripts/probe_cursor_agent.py`（CLI `2026.08.11-e8db854`，`--mode ask`）

- 有 token：是（仅无头 stdout）
- 口径：`result.usage`
  - input ← `inputTokens`
  - output ← `outputTokens`
  - cache_read ← `cacheReadTokens`
  - cache_creation ← `cacheWriteTokens`
  - reasoning：无
  - total：各口径之和
  - native_cost：无
- 模型：`system.model`（`result` 上没有）
- 项目 / 会话：`system.cwd` / `session_id`
- 去重：只取 `type=result`，可用 `request_id`
- hook：`sessionEnd` 无数字；这次无头运行未触发 `stop`
- 本机 `store.db` / `agent-transcripts`：无 token
- 详见 `docs/probe/cursor-agent.md`

---

以下为 2026-08-16 的 `cargo run --bin probe` 结果。

## dsh

- 有 token：是
- zstd 解压：成功
- 最终口径：`assistant/message.data.usage`
  - input ← `inputTokens`
  - output ← `outputTokens`
  - cache_read ← `cacheReadTokens`
  - cache_creation：无
  - reasoning ← `reasoningTokens`
  - total：各口径之和
- 模型 / provider：`data.message.source` 或 `request/header.config`
- 项目 / 会话：`session.cwd` / `session.id`
- 去重：只用 `assistant/message`，忽略流式 `assistant/chunk`

## gemini

- 有 token：是（在 `tmp/*/chats/session-*.json`，不在 `logs.json`）
- 口径：`tokens.input` / `output` / `cached` / `thoughts` / `total`
- 口径：`tokens.input` / `output` / `cached` / `thoughts` / `total`；cache_creation：无
- 模型：`message.model`
- provider：文件中不存在
- 会话：`sessionId`
- 项目：tmp 子目录名
- 原始文件：该 `session-*.json`

## grok

- 有 token：是（`turn_completed.usage` 轮级分项；旧日志才只有上下文总量）
- 口径：`params.update.usage`（`sessionUpdate=turn_completed`）
  - input ← `inputTokens`（含 cache read）
  - output ← `outputTokens`（含 reasoning）
  - cache_read ← `cachedReadTokens`
  - cache_creation ← `cacheCreationTokens`
  - reasoning ← `reasoningTokens`
  - total ← `totalTokens`
  - native_cost ← `costUsdTicks` / 1e10
- 模型：`usage.modelUsage` 的键
- 项目：url-decode 的会话父目录
- 会话：目录 uuid
- 去重：同一 `prompt_id`+model 取最后一次 `turn_completed`

## qwen

- 有 token：否
- 本机 `~/.qwen/tmp/*/logs.json` 只有 user 文本与 `sessionId`
- 模型 / provider / 项目：文件中不存在

## factory

- 有 token：是（会话累计，非每轮）
- 口径：`<id>.settings.json` 的 `tokenUsage`
  - input ← `inputTokens`
  - output ← `outputTokens`
  - cache_read ← `cacheReadTokens`
  - cache_creation ← `cacheCreationTokens`
  - reasoning ← `thinkingTokens`
  - total：各口径之和
- 模型：settings 中不存在
- provider：`providerLock`
- 项目：会话目录 dashed 名解码（根级 settings 则空）
- 会话：文件名前缀 uuid
- 原始文件：该 `<id>.settings.json`
