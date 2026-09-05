# Rust 模块边界与对外路径

对话记录编排层、数据模型、存储层、查询层和 Tauri command 层都曾是上千行的单文件。维护者每次都要重新争论「这文件该不该拆」，调用处也看不出碰到的是 schema 还是价目。

**决定**：生产代码 **800 行软红线**（超线触发审视，不是 CI 失败）。测试文件豁免行数，按被测模块对齐。对外路径按「词汇表 vs 服务集合」区分。

## 对外路径

- **数据模型**（`domain`）是词汇表。拆成子模块后**全量 re-export**，`crate::domain::UsageRecord` 等路径不变。
- **对话记录**子模块保持私有，模块根 re-export 少数入口（事件读取、各来源刷新）。
- **存储层 / 聚合查询层**是服务集合，子模块路径可以读出职责（schema / 记录 / 预聚合；`query/` 含 analytics、billing、sessions 等）。连接打开与 `ADAPTER_VERSION` 留在 `store` 根上。
- **Tauri command**：函数体可进 `commands` 子模块，**注册列表留在 crate 根**，因为 `generate_handler!` 要写全路径。command 内部只取状态、锁连接、按需载入价目，然后委托查询/摄取。

## 哨兵

摄取缓存 `store::ADAPTER_VERSION` 与对话记录 `CONVERSATION_ADAPTER_VERSION` 在纯重构中**不得改动**。改了就说明动了归一化输出。

「不为行数加机械门禁」与「为依赖方向加机械门禁」是两个独立决定。行数阈值会激励「把 900 行拆成两个 450 行」这类无语义收益的切分；依赖方向不会。依赖方向门禁当前只作用于对话记录目录（`src-tauri/src/conversation/`：禁止 `use super::*`，模块根不得定义白名单外的 `fn`），推广到存储层、查询层、数据模型等目录留待这条约束被验证之后。

## 后果

- 加字段时先看它属于哪个 domain 子模块。
- 新增来源时同时看编排层有没有又胀回去。
- 不为软红线加机械 CI 门禁。
- **已知超线文件（2026-09，待继续拆分）**：`conversation/toolbox.rs`、`ingest.rs`、`aggregate.rs`。超线不阻断 CI，但新增逻辑应优先进子模块而非继续堆高。
