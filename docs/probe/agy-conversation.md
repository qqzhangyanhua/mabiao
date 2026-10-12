# 探测结果：Antigravity（agy）对话记录

实测时间：2026-10-12。本机 `~/.gemini/antigravity-cli/conversations/*.db`（23 个，WAL 模式）。只记录字段位置，不摘录会话正文。`antigravity-ide/conversations` 下只有旧版加密 `.pb`，不读。

这是对真实库做 protobuf 结构探测得到的字段号，**不是官方 schema**。官方二进制里的 `CortexStep` 以后若对不上，以本文件为准重新探测。

## 表

`steps(idx, step_type, status, metadata, step_payload, step_format, …)`。`step_payload` 是完整的 `CortexStep`：字段 1 是步骤类型（等于 `step_type` 列），字段 5 与 `metadata` 列逐字节相同，其余是按步骤类型分开的 oneof 字段。没有 `step_payload` 列的库不当对话记录读。

## 已解析

| 步骤类型 | 含义 | 字段 |
|----------|------|------|
| 14 | 用户输入 | `19.2` 用户原文 |
| 15 | 规划器回复 | `20.1` 回复正文；`20.3` 思考；`20.7`（重复）工具调用 `{1 id, 2 name, 3 参数 JSON}` |

公共：`5.1` 是 `Timestamp`；`5.9.11` 是 request id，去 `gen_metadata` 查模型名（与消耗记录同一条路）。

## 没解析

- 工具执行步骤（本机以 132 为主，另有 17、23、101）。每个工具调用后紧跟一个执行步骤，但结果字段随工具类型各异，且没有稳定的 call id 能和规划器里的工具调用配对。所以对话记录里只有工具调用，没有工具结果，详情里带 `capability_degraded: tool_result`。
- `trajectory_meta`、`parent_references`：本机 23 个库全是顶层、无父引用，不据此判断子代理。
