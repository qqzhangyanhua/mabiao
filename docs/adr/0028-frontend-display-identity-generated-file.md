# 前端展示身份由 Rust 穷举登记，经测试生成静态文件

来源显示名和来源 slug 联合类型原先手写在前端（`src/lib/format.ts` 的显示名表、`src/types.ts` 的 `UsageSource`）。它们和 Rust 的权威名单（`Source::ALL`、`Source::application_name`、`OfficialQuotaProvider::ALL`、`OfficialQuotaProvider::display_name`）之间没有测试绑定。漏改不会编译失败，也不会测试失败，界面上会悄悄退化成原始 slug。

显示名查询是同步纯函数，调用点在组件和 node 环境下的测试里。项目规则不允许为了下发一张表引入全局状态库。

**决定**：前端展示身份由 Rust 穷举登记，经测试生成一个提交进仓库的静态 TypeScript 文件。本篇先落地显示名与 slug 列表。后续展示身份字段沿同一文件、同一条生成路径扩展，不另选交付方式。

## 生成文件

文件是 `src/generated/displayIdentity.ts`。全部导出都是 `as const` 字面量：

- 全部来源 slug，顺序与 `Source::ALL` 一致
- 全部官方额度提供方 slug，顺序与 `OfficialQuotaProvider::ALL` 一致
- 按 slug 索引的显示名
- 由 `SOURCE_SLUGS` 推导的来源 slug 联合类型 `UsageSource`

生成逻辑只在 `src-tauri/src/tests/display_identity.rs`，不进发布产物，运行时不需要。前端构建不依赖先跑 Rust。

该测试把生成结果当快照：和已提交文件逐字不一致就失败，失败信息给出重新生成的命令。设置 `MABIAO_UPDATE_DISPLAY_IDENTITY=1` 时改为重写文件。CI 不新增流水线步骤，已有的 `cargo test` 覆盖一致性。手改生成文件同样会失败。

重新生成分两步。带环境变量的那次只重写文件并直接通过，不能代替快照比对。写完后必须再跑一次不带该环境变量的同一条测试，第二次通过才算生成文件和 Rust 登记一致。

```
MABIAO_UPDATE_DISPLAY_IDENTITY=1 cargo test --manifest-path src-tauri/Cargo.toml --lib tests::display_identity::display_identity_file_matches_committed_snapshot -- --exact
cargo test --manifest-path src-tauri/Cargo.toml --lib tests::display_identity::display_identity_file_matches_committed_snapshot -- --exact
```

## 重名 slug

来源和官方额度提供方可以共用一个 slug。合并规则：

- 同一个 slug 两边都有时，显示名以来源的 `application_name` 为准。因此 `claude` 是 “Claude Code”，不是额度提供方的 “Claude”；`grok` 是 “Grok CLI”；`copilot` 是 “GitHub Copilot CLI”。
- 只在额度里出现的 slug（`cursor`、`droid`、`antigravity`、`devin`）用额度提供方的 `display_name`。

按这个规则生成的显示名，与改动前前端手写表的 slug 到名字一一相同。未知 slug 仍由 `sourceLabel` 原样返回，不进生成文件。

## 三种交付方式

- **启动时 IPC**：Rust command 在启动时把表发给 webview。否决。`sourceLabel` 是同步纯函数，有多处调用，其中包含 node 环境下的测试。改成 invoke 会让它们变成异步或改成接收参数，而且项目规则不允许为此引入全局状态库。webview 在 invoke 成功前也无法显示名字。
- **每个 DTO 自带字段**：每条带 slug 的 DTO 都加显示名。否决。DTO 种类太多；筛选条、旧缓存里的未知 slug、以及不经过某条 DTO 的纯函数仍然要自己查表。
- **静态生成文件**（采用）：编译期穷举登记，测试生成 `as const` 字面量，前端同步 import，类型从字面量推导。忘了重新生成由 `cargo test` 拦住。

## 前端怎么用

- 删除手写显示名表。`sourceLabel` 的签名不变，改为读生成文件的 `DISPLAY_NAMES`。
- `src/types.ts` 的手写镜像不再包含来源联合类型。`UsageSource` 从生成文件推导后 re-export，`SourceIngestReport.source` 仍用这个类型。
- 额度模块「可单独开关的来源」（`QUOTA_SOURCE_IDS`）保留手排顺序。vitest 只断言它在集合上等于全部来源加上 `cursor`。
- 官方额度设置里的提供方标签（`OFFICIAL_QUOTA_PROVIDER_LABELS`）不是这张显示名表，本篇不改。
- 管理网页 `server/web` 自持一份显示名，不在本决定范围。

## 后果

- 新增来源时，`application_name` 的穷举 `match` 缺分支会编译失败；重新生成展示身份文件后，前端显示名和 `UsageSource` 跟着变。不引入新的前端手写名单。
- 纯重构不改归一化输出，因此不改 `ADAPTER_VERSION` 与 `CONVERSATION_ADAPTER_VERSION`。
- 看到有人再手写来源显示名表或来源联合类型，或把显示名改成启动时 invoke，就是在违反本篇。
