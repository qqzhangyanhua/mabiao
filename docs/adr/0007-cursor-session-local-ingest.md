# Cursor 会话走本机文件摄取、独立维度

> 原文件名 `0005-cursor-session-local-ingest.md`，2026-08-18 重编号为 0007，避免与路径配置 ADR 0005 撞号。

Cursor IDE Agent 与 `cursor-agent` CLI 共用本机目录：`~/.cursor/chats/<md5(cwd)>/<session>/store.db` 和 `~/.cursor/projects/*/agent-transcripts/*/*.jsonl`。transcript 不含 token；行为统计（轮次、工具调用、失败率）与代码量、账号用量语义不同，不应并入消耗记录或总览 token KPI。`~/.cursor-agent-usage` 只是本仓库包装脚本的 token 落盘，不是官方会话库。

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
- 会话正文留在磁盘 jsonl，App 不索引；详情页、搜索不在本 ADR 范围。
- 子代理不再占用会话数。升级后旧的子路径会话行会在一次成功对账后删掉。
- 会话详情按需读盘：工具次数用缓存 JSON，读写 path 重解析 transcript（只取 `input.path` / `paths`），hash 文件按 conversationId 只读查询。详情不入库，不触发 schema_version。
- 单条会话的浏览入口是对话记录：仪表盘点行跳到 `source=cursor_agent` 的对话详情，行为聚合作为 `cursor_behavior` 挂在同一条记录上，不再用独立详情页。跨会话 KPI/图表仍留在 Cursor 会话页。
