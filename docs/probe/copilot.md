# 探测结果：GitHub Copilot CLI

与其它 `docs/probe/*.md` 不同，早期没有 GitHub Copilot CLI 本机样本时，字段位置来自官方文档与开源实现的交叉验证。仓库现已用脱敏 fixture `copilot-events.jsonl` 与 `tests/adapters.rs` 单测锁定口径；落地前仍建议用真实 `events.jsonl` 再核对一遍：

- <https://docs.github.com/en/copilot/how-tos/copilot-cli/cli-best-practices>（会话落盘路径）
- <https://docs.github.com/en/copilot/concepts/agents/copilot-cli/chronicle>（`session-state` 与本地 session store 的关系）
- GitHub `copilot-cli` 仓库 issue #1394（`session.shutdown.data.modelMetrics` 字段结构与上线时间）
- `ccusage` 仓库 issue #1174（`events.jsonl` 逐行事件的字段映射建议）

## 本机落盘路径

```text
~/.copilot/session-state/<session-id>/
├── events.jsonl      # 完整会话事件流，我们只用这个文件
├── workspace.yaml
├── plan.md
├── checkpoints/
└── files/
```

`events.jsonl` 每行一个 JSON 事件，顶层有 `type` 判别字段。老版本 CLI 曾用
`~/.copilot/history-session-state/<id>.json` 单文件格式，且不含 token 统计，不再支持。

## 字段映射

token 只出现在 `session.shutdown` 事件的 `data.modelMetrics` 里，按模型给出**本会话累计值**
（不是本轮增量）：

```json
{
  "type": "session.shutdown",
  "timestamp": "2026-08-10T15:12:30.500Z",
  "data": {
    "modelMetrics": {
      "gpt-5.4": {
        "requests": { "count": 5, "cost": 1 },
        "usage": {
          "inputTokens": 244120,
          "outputTokens": 2383,
          "cacheReadTokens": 202112,
          "cacheWriteTokens": 0
        }
      }
    },
    "currentModel": "gpt-5.4"
  }
}
```

| Usage Record | 字段 |
|--------------|------|
| input | `usage.inputTokens` |
| output | `usage.outputTokens` |
| cache_read | `usage.cacheReadTokens` |
| cache_creation | `usage.cacheWriteTokens` |
| reasoning | 无 |
| total | 无，按各口径之和 |
| native_cost | 无（`requests.cost` 是「高级请求」计费单位，不是 USD，不映射为 native_cost） |
| 模型 | `modelMetrics` 的对象键 |
| 项目 | `session.start.data.context.cwd` |
| 会话 | `session.start.data.sessionId`；缺失时退回父目录名（目录名本身就是 session id） |

## 去重口径

一次 CLI 会话可能被多次暂停/续接，每次退出都会追加一条新的 `session.shutdown`，且
`modelMetrics` 每次都是**从会话开始到当前**的累计值（会包含更早退出时已经统计过的用量）。
因此适配器只取文件里**时间最晚**的一次 `session.shutdown`，而不是把每条 `session.shutdown`
都当独立记录相加，否则会重复计入。这与 Codex 适配器「取最后一次快照、不逐条累加」的策略一致。

## 未采纳的口径

- `~/.copilot/otel/*.jsonl`（需要用户显式设置 `COPILOT_OTEL_FILE_EXPORTER_PATH` 才会产出，不是
  默认行为，因此不作为主口径）。
- `session.compaction_complete` 里的 nano-AIU 计价表：目前只有部分版本携带，且换算规则（AI 积分制）
  仍在变化中，先不接入 `native_cost`，等价格口径稳定后再评估。
