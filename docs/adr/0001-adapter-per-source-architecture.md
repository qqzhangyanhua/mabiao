# 每个采集源一个适配器（Adapter-per-Source）

> **后续说明**：`Source` 枚举的权威名单是 `domain::Source::ALL`（当前 14 个 Usage Source，含 Hermes / Copilot 等）。下文举例里的 Cursor 与 amp **不是** Usage Source 变体：Cursor 走代码量 / 账号用量 / 会话等独立维度，amp 用量在云端、不纳入消耗记录。

本机使用的 AI 编程工具超过 10 个（codex、claude code、pi、dsh、opencode、kimi、gemini、grok、qwen、factory、cursor、amp…），且数量仍在增长。各工具的本地存储格式高度异构：jsonl、zstd 压缩 jsonl、sqlite、每消息一个 json 文件；token 字段的命名与结构也各不相同。

**决定**：定义一个统一的「消耗记录 (Usage Record)」标准模型，为每个 Source 编写独立的 Adapter，负责把该 Source 的原始格式解析并归一化为 Usage Record。统计聚合与 GUI 展示只面向标准模型，不感知任何具体工具格式。

**理由**：新增/变更一个工具只需增删一个 Adapter，核心逻辑与界面零改动。相较把各工具解析硬编码进统计层，这样避免了"加一个工具改一大片"的耦合。

## Consequences
- 每个 Adapter 需各自处理该工具的去重口径（如 Codex 同时存在累计 `total_token_usage` 与单轮 `last_token_usage`，必须只取其一避免重复计数）。
- Cursor 只有代码量、amp 数据在云端，它们要么走独立维度（代码量面板），要么暂不纳入，而非硬塞进 token 模型。
