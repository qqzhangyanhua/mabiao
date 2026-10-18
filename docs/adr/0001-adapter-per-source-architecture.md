# 每个采集源一个适配器（Adapter-per-Source）

> **后续说明**：`Source` 枚举的权威名单是 `domain::Source::ALL`（当前 29 个 Usage Source，含 Hermes / Copilot / Antigravity / Qoder / Qoder CN / Cline / WorkBuddy / ZCode / CodeBuddy / Trae / Goose / Roo Code / Kilo Code / Amp / Zed / Cherry Studio 等）。下文举例里的 Cursor **不是** Usage Source 变体：Cursor 走代码量 / 账号用量 / 会话等独立维度。

本机使用的 AI 编程 CLI 当前有 **29 个 Usage Source**（权威名单：`domain::Source::ALL`）：codex、claude、pi、omp、dsh、opencode、kimi、gemini、grok、qwen、factory、cursor_agent、copilot、hermes、agy、qoder、qoder_cn、cline、workbuddy、zcode、alma、codebuddy、trae、goose、roo、kilo、amp、zed、cherry。各工具的本地存储格式高度异构：jsonl、zstd 压缩 jsonl、sqlite、每消息一个 json 文件；token 字段的命名与结构也各不相同。

**Cursor**（代码量 / 账号用量 / 会话 / 对话记录）**不是** `Source` 变体，走独立维度，见 ADR 0006 / 0007 / 0011 与 CONTEXT.md。

**决定**：定义一个统一的「消耗记录 (Usage Record)」标准模型，为每个 Source 编写独立的 Adapter，负责把该 Source 的原始格式解析并归一化为 Usage Record。统计聚合与 GUI 展示只面向标准模型，不感知任何具体工具格式。

**理由**：新增一个来源时，补消耗记录适配器、`Source` 上的穷举登记，并重新生成展示身份文件。它属于已有品牌时，前端只把它加进额度模块的手排名单。只有引入新品牌，才在前端补 SVG 和配色。统计层仍然不感知原始格式，避免把各工具解析硬编码进统计与界面。

## 新增一个来源

1. 写消耗记录适配器，并登记到 `USAGE_ADAPTERS`。
2. 给 `Source` 的穷举方法补分支：`application_name`（显示名）、`brand`、`resume_template`（没有公开恢复命令就返回 `None`）。缺一个分支就编译失败。
3. 重新生成展示身份文件（ADR 0028）。
4. 把新来源加进额度模块「可单独开关的来源」（`QUOTA_SOURCE_IDS`）。这是手排顺序，常用项靠前；集合必须等于全部来源加上 `cursor`，vitest 会检查。
5. 只有这个来源引入了新品牌时，才在前端补该品牌的 SVG（`src/components/sourceGlyph.tsx`）和配色 class（`src/lib/sourceIcon.ts`）。少一条时 tsc 报错。已有品牌不用改这两张视觉表。

Cursor 专属的展示规则（对话记录里的 “Cursor / Cursor Agent”、用量筛选 `cursor` 对应 `cursor_agent`、“共 N 条事件”）描述的是 Cursor 维度在某个界面里怎么称呼，不在这张清单里。官方额度开关的文案读展示身份显示名，见 ADR 0028。

## Consequences
- 每个 Adapter 需各自处理该工具的去重口径（如 Codex 同时存在累计 `total_token_usage` 与单轮 `last_token_usage`，必须只取其一避免重复计数）。
- Cursor 只有代码量，走独立维度（代码量面板），而非硬塞进 token 模型。
- 展示身份的生成、重名 slug、品牌与视觉的分工见 ADR 0028。层清单的 Adapter 节与上面的步骤一致。
