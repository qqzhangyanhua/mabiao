# 层清单

从 `AGENTS.md` 按层跳入。做完该节每一项，节末命令通过，才进入下一层或收工。

## Adapter

新增或改 Usage Source 的消耗记录解析。

1. 在 `domain::Source` 加变体（`src-tauri/src/domain/usage.rs`，`domain.rs` re-export）。
2. 实现 `src-tauri/src/adapters/<source>.rs`：扫描、发现、解析；需要时加 sidecar 指纹或 `prepare_dir` / `prepare_file`。
3. 在 `adapters/mod.rs` 的 `USAGE_ADAPTERS` 加一行，含 `path_env`（漏了完备性测试会红）。
4. 脱敏 fixture 放到 `src-tauri/tests/fixtures/`。
5. `tests/adapters.rs` 断言去重与累计口径。
6. 归一化输出变了就递增 `store.rs::ADAPTER_VERSION`。

来源差异写在 Adapter 表与 `adapters/<source>.rs`。`ingest.rs` 保持来源无关。

完成：`USAGE_ADAPTERS` 有该行，fixture 与去重/累计断言在，`cargo test adapters` 绿。

## 聚合

改 `query.rs` 的费用或时段 SQL。

1. 同步改 `aggregate.rs`。
2. 费用优先级：`native_cost` > 用户价目 > LiteLLM 快照 > unpriced。
3. `cargo test parity`（`sql_queries_match_in_memory_aggregates`）绿。

## DTO

1. 先改 Rust `domain`（`domain.rs` 全量 re-export），再改 `src/types.ts`，字段 snake_case。
2. `pnpm build` 绿。

## 摄取

改摄取、备份或归档。

1. 对账路径：`ingest.rs` → `store::records::reconcile_source` → `archive_records_for_file`。`files_failed > 0` 或目录读取失败则跳过对账。
2. 归档只打 `archived_at`，源文件留在原地。
3. 读 ADR 0003、0004。
4. `cargo test ingest` 与 `cargo test backup` 绿。

## 扫描路径

1. 默认路径与 env 名写在 `USAGE_ADAPTERS`，由设置页或环境变量覆盖。`ingest.rs` 不拼 `home.join(...)`。
2. 改默认路径或 env 名时同步 fixture 与 `tests/adapters.rs`。逗号多路径、Claude XDG 双路径见 ADR 0005。
3. `cargo test adapters` 与 `cargo test ingest` 绿。

## Cursor 账号

独立维度，写入 `cursor_account_*`，与消耗记录分区。

1. 凭证只读本机 Cursor `state.vscdb`。
2. 刷新用独立按钮或设置页自动刷新；不挂 `ingest_all`、不挂启动摄取定时器。
3. fixture 测 parser 与去重。读 ADR 0006。
4. `cargo test cursor` 绿。

## Cursor 会话

1. 行为 KPI 写入 `cursor_sessions`。transcript 正文走对话记录（`source=cursor_agent`，ADR 0011）。
2. 版本哨兵是 `cursor_session_meta.schema_version`，与 `ADAPTER_VERSION` 分开。
3. 读 ADR 0007。`cargo test cursor` 绿。

## 官方额度

1. 内置账号在 `domain::OfficialQuotaProvider::ALL`（`src-tauri/src/domain/quota.rs`）。新增一家：加 `official_quota/<provider>.rs`，并接到 `detect.rs` 与 `fetch.rs`。
2. 凭证只读各客户端已有登录态。自定义提供商密钥单独文件，备份排除该文件。
3. `tray.rs::sync_official_quota` 在刷新、全量摄取后、每 5 分钟 stale 检查时取数（受退避约束）。最紧一档走 `official_quota::tightest_window`：无 `resets_at` 的自定义充值余额不参与；有重置时间的自定义预算窗参与。
4. fixture 测响应解析。读 ADR 0008、0012、0013。
5. `cargo test quota` 绿。

## 对话记录

1. 事件 `text`/`name` 进 `conversation_events`（ADR 0011）；目录搜索走 FTS 派生表（ADR 0014）。`details` 按需读原文件。正文留在本机索引，备份与上传不含正文。
2. Cursor Agent transcript 走 `conversation/cursor.rs`，`source=cursor_agent`，与 `cursor_sessions`（ADR 0007）分区。
3. `cargo test conversation` 绿（含增量、回填、正文搜索）。

## 全局指令

1. 实时读盘。口径是「该 Source 真正会加载的」。
2. 改用户文件走「用户文件」节。
3. 读 ADR 0009。`cargo test instructions` 绿。

## 用户文件

1. 全应用一个写入入口，路径在白名单内。
2. 每次写入同时：写入前 mtime 校验、写入前备份到应用数据目录、同目录临时文件再 `rename`。
3. 读 ADR 0010。`cargo test instructions` 绿。

## 报告

1. 洞察规则写在 Rust `report`；前端 `reportCopy` 只把 payload 映射成文案。
2. 数字只取消耗记录。
3. 新增时段聚合同步 `query.rs` 与 `aggregate.rs`，`cargo test parity` 绿。
4. 海报 CSS 只走 `src/report/*.css` 与 `posterStyleRegistry`（ADR 0019）。
5. 分享入口只出报告（ADR 0020）。读到 ADR 0018「周报 | 额度」时以 0020 为准。

## 工作纪要

读区间内的对话正文，经本机 CLI 总结成结构化条目。不是报告，不复用 `ReportPeriod`。

1. 入口三个：`work_notes::preview`（只读，收 `&Connection`）、`work_notes::generate`（区间纪要）、`work_notes::summarize_session`（单条会话摘要）。后两个收 `ConnectionSource` 而不是连接——「读连接取输入 → 放开连接调 CLI → 有东西可写才取写连接」这套编排在 `work_notes/pipeline.rs` 与 `session.rs` 里，command 不参与；`now`、runner、job、应用数据目录仍由调用方注入。`ConnectionSource` 的实现不得同时持有读与写 guard。command 只做 `begin`（互斥闸门，错误要同步回前端）→ spawn → 入口 → `finish`。生成是后台任务，进度写进 AppState，前端轮询。三种区间、60/150 规模闸门、成本预估在 Rust 判定，webview 只呈现。
2. runner 边界是「执行一条已经拼好的 `EngineCommand`，回一段 stdout」。argv 拼装（只读、禁审批、不落盘、schema、会落盘引擎的 session id）必须跑在这条缝里。
3. 本机 CLI 只读、禁工具，工作目录固定在应用数据目录下的专用空目录。不自建模型 HTTP 通路，不存密钥。
4. 硬数字只复用现有 `query` 函数，不新增聚合。
5. 读 ADR 0021、0011、0002。`cargo test work_notes` 绿。真 spawn 冒烟 `#[ignore]`，不进 CI。
6. 引擎清单只列 `which` 探测到的。探测只走设置页手动按钮（`which` + `--version`），启动时不 spawn。加引擎只改 `work_notes::engines` 的 profile 表，不改编排。
7. 会落盘的引擎必须可识别：能钉 session id 就钉死，否则靠专用工作目录。识别后只从后续纪要输入剔除、在对话记录打「码表生成」标记；不从 token KPI 扣除，不删改用户会话文件。

## 样式

1. 按 `src/styles/` 分层、按域拆文件。入口 `src/styles.css` 只含 `@import`。
2. 单文件 ≤ 400 行。门禁是 `src/lib/cssStructure.ts` + Vitest。
3. 海报 CSS 走「报告」。读 ADR 0016。`pnpm test` 绿。

## Rust 模块边界

1. 生产代码 800 行软红线。`domain` 子模块全量 re-export。
2. Tauri command 注册列表留在 crate 根。纯重构保持 `store::ADAPTER_VERSION` 与 `CONVERSATION_ADAPTER_VERSION` 不变。
3. 读 ADR 0017。`cargo clippy` 与相关测试绿。
