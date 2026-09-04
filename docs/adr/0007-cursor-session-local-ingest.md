# Cursor 会话走本机文件摄取、独立维度

> 原文件名 `0005-cursor-session-local-ingest.md`，2026-08-18 重编号为 0007，避免与路径配置 ADR 0005 撞号。

> **2026-09-03 修订（对话记录索引）**：Cursor Agent transcript 的正文与工具事件已纳入对话记录维度（ADR 0011 / 0014），`source=cursor_agent` 走 `conversation/cursor.rs` 写入 `conversation_events`，目录 FTS 可搜。本篇仍只管 **Cursor 会话** 行为聚合表 `cursor_sessions`（轮次、工具、失败率等 KPI），不进 `conversation_events`。

Cursor IDE Agent 与 `cursor-agent` CLI 共用本机目录：`~/.cursor/chats/<md5(cwd)>/<session>/store.db`（cursor-agent **UsageRecord** 适配器用）和 `~/.cursor/projects/*/agent-transcripts/*/*.jsonl`（**Cursor 会话** KPI 与 **对话记录** 用）。`store.db` / `~/.cursor/chats` **不进** `cursor_sessions` 行为表。transcript 不含 token；行为统计（轮次、工具调用、失败率）与代码量、账号用量语义不同，不应并入消耗记录或总览 token KPI。`~/.cursor-agent-usage` 只是本仓库包装脚本的 token 落盘，不是官方会话库。

**决定**：新增独立维度「Cursor 会话 (Cursor Session)」。在 `ingest_all` 时扫描 `~/.cursor/projects/*/agent-transcripts/*/*.jsonl`，按 `agent-transcripts/<session-id>/` 分组：只给父 jsonl 建会话，同目录 `subagents/*.jsonl` 的轮次/工具/提问并入父会话。并从 `ai-code-tracking.db` 的 `ai_code_hashes` enrich 模型/文件/时间/source/扩展名；解析为会话级聚合写入独立缓存表 `cursor_sessions`；只存聚合字段，不存对话正文。orphan hash、只有子代理没有父 jsonl 的目录，都不单独造会话。

缓存表加列后用 `cursor_session_meta.schema_version` 强制重解析，不参与 `ADAPTER_VERSION`。

边界：

- **独立维度**：不进入 `UsageRecord`、`Source` 枚举或本机 token 聚合；界面 Sidebar 独立入口。
- **本机文件**：与 Cursor 账号用量（联网、独立 refresh）严格分离；不得扩散联网路径。
- **可信缓存**：文件 `(mtime_ms, size)` 指纹未变跳过重解析；解析失败保留旧缓存；有失败时跳过对账删除；删除 transcript 后对账清理。
- **摄取失败不 abort**：Cursor 会话问题记入 `IngestReport.issues`（source=`cursor-session`），不阻断其它 Source。
- **不参与 ADAPTER_VERSION**：独立 `cursor_session_files` / `cursor_session_meta` 表。

## Consequences

- 无 hash enrich 时模型为空、时间退化为文件 mtime；纯问答会话仍计入。
- **Cursor 会话表不存 transcript 正文**；行为 KPI 在 `cursor_sessions`，单条 transcript 的正文与事件索引在对话记录（`source=cursor_agent`，ADR 0011）。transcript 缺失时对话详情仍可展示 `cursor_behavior` 与包装 token 用量。
- 子代理不再占用会话数。升级后旧的子路径会话行会在一次成功对账后删掉。
- 单条浏览入口是对话记录：仪表盘点行跳到 `source=cursor_agent` 的对话详情，`cursor_behavior` 挂在同一条记录上。跨会话 KPI/图表仍留在 Cursor 会话页。
