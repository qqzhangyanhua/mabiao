# 上下文清单：加载档位与体积口径

对话详情的上下文清单要从「磁盘上可能有什么」升级成「这一轮到底带进去了什么」。没有加载档位，Cursor 的 alwaysApply / globs / description / 手动四档会混成一张平铺名单，体积是错的。没有体积口径，界面上的 token 数字会被当成精确值。

**决定**：沿用 `ConversationContextItem` / `ConversationContextManifest`，不另造平行结构体。清单分三层证据：`injected`、`observed`、`on_disk_possible`。条目带 `load_mode`（`always` / `on_match` / `on_demand` / `manual` / `observed`）。体积以**字符数**为权威值写入 DTO；token 只在展示层按工作纪要的 `chars_to_tokens`（4 字符 ≈ 1 token）换算，主显「约 N tok」，副行给实测字符数。

注入快照读取与对话记录适配器隔离：不写 `conversation_events`、不改 `CONVERSATION_ADAPTER_VERSION`、不把注入正文送进任何缓存。Grok 的 tracer 读会话目录 `prompt_context.json` 的 `agents_md_files[]`。

## 加载档位

`load_mode` 回答「这东西什么时候会进上下文」，不是抽象置信度。

| 档位 | 含义 |
|------|------|
| `always` | 常驻。Grok 注入的 AGENTS.md 走这一档。 |
| `on_match` | 路径 / glob 命中才挂。 |
| `on_demand` | 模型按描述按需拉取。 |
| `manual` | 用户手动 @。 |
| `observed` | 本会话事件里真实出现过。 |

磁盘层条目在尚未分档前可以不填。`injected` 层可表述为已注入；`on_disk_possible` 层仍然不得。

## 体积

权威值是 Unicode 标量个数（Rust `chars().count()`），不是文件字节、不是估算 token。展示层换算允许误差，文案必须带「约」。Grok 注入正文用快照里的 `content` 计量，不回读当前磁盘——会话之后改过的文件不能冒充当时体积。Grok MCP 连上的 server 按注入工具名列表（逗号拼接）的字符数计量，不用配置文件字节；未连上的不占体积、不吃噪音红标。

## 故意不做

这三条不写进本 ADR，日后会被当成遗漏补上：

1. **不启动 MCP server 拉 tool schema。** 真正占上下文的 schema 只在运行时握手才有。为了一个数字去连服务器，违反本应用只读本机文件的基调。
2. **指令不判噪音。** 红标只给 skill 与 MCP。指令 / 规则没有「用没用上」的可观测信号，标红会误导删除。
3. **不跑 `grok inspect`。** 它能列出已加载的 skills 与 MCP，但会拉起全部 MCP server 子进程。把这个塞进「打开对话详情」不可接受。

## 后果

- 看到有人把注入正文写入 sqlite / FTS / 备份，就是在违反本篇与 ADR 0011 / 0014。
- 看到有人把 `on_disk_possible` 写成「已注入」，就是在违反三层证据模型。
- 看到打开对话详情去 spawn MCP 或 `grok inspect`，就是在违反「故意不做」。
- Cursor 注入快照读取另模块落地，复用本篇的层、档位与体积口径。
