# 探测结果：cursor-agent

实测时间：2026-08-17。命令：

```bash
python3 scripts/probe_cursor_agent.py
```

只记录字段位置与数字口径，不含会话正文。CLI 版本：`2026.08.11-e8db854`。无头调用使用 `--mode ask`。

## 结论

| 通道 | 有 token | 说明 |
|------|:---:|------|
| 无头 stdout `stream-json` 的 `type=result` | 是 | 每轮一条，含 input/output/cache |
| `sessionEnd` hook | 否 | 有会话/模型/项目，无数 |
| `stop` hook | 未触发 | 这次无头运行没有收到 `stop` |
| `~/.cursor/chats/.../store.db` | 否 | 会话会落盘，blob 无 usage |
| `~/.cursor/projects/.../agent-transcripts/*.jsonl` | 否 | 只有 user/assistant/`turn_ended` |

历史会话补不回 token。要进 Usage Record，只能前瞻捕获 `stream-json` 的 `result` 并自己落盘。

## stream-json

事件顺序：`system` → `user` → `thinking`* → `assistant` → `result`。

token 只出现在最后一条 `result`：

| Usage Record | 字段 |
|--------------|------|
| input | `usage.inputTokens` |
| output | `usage.outputTokens` |
| cache_read | `usage.cacheReadTokens` |
| cache_creation | `usage.cacheWriteTokens` |
| reasoning | 无 |
| total | 无，按各口径之和 |
| native_cost | 无 |
| 模型 | `system.model`（`result` 上没有） |
| 项目 | `system.cwd` |
| 会话 | `session_id` |
| 去重 | `request_id`（同一请求只留 output 更大的一条）；只计 `type=result` |

本次数字：input 18851 / output 35 / cacheRead 0 / cacheWrite 0。`assistant`、`thinking` 均无 usage。

## hook

项目级临时 hook 只写字段名。无头运行触发了 `sessionEnd`，未触发 `stop`。

`sessionEnd` 有：`conversation_id` / `session_id` / `model` / `workspace_roots` / `duration_ms` / `final_status` / `transcript_path`。`numeric_token_fields` 为空。`reason` 是结束原因字符串，不是 reasoning token。

## 本机落盘

同一次无头会话写入了：

- `~/.cursor/chats/<projectHash>/<session_id>/store.db`
- `~/.cursor/projects/.../agent-transcripts/<session_id>/<session_id>.jsonl`

transcript 行类型：`user` / `assistant` / `turn_ended`。无 token 字段。与此前全库扫描一致。
