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

1. 同步改 `aggregate.rs`。Cursor 账号用量叠加走 crate 根 `cursor_overlay`，不进 `aggregate`。
2. 费用优先级：`native_cost` > 用户价目 > LiteLLM 快照 > unpriced。规则本体在 `crates/pricing/`（价目匹配、`price_usage`），`cost.rs` 只做批量折叠与未定价诊断；改优先级先改那个 crate，再核对 `query.rs` 的 SQL 与 `aggregate.rs` 仍一致。`domain` 里的 `PriceEntry` / `PriceTable` / `PriceOrigin` / `CostSource` / `DerivedCost` 是该 crate 的 re-export，不在 `domain` 另定义。
3. `cargo test --manifest-path crates/pricing/Cargo.toml` 与 `cargo test parity`（`sql_queries_match_in_memory_aggregates`）绿。

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

1. 事件 `text`/`name` 进 `conversation_events`（ADR 0011）；目录搜索走 FTS 派生表（ADR 0014）。Codex / pi / omp 能按行重建的消息、工具结果、计划只存 `line_offset` + `text_hash`，读时回源文件取，对不上就整份解析；库内 `event_id` 存推导形态，直接读列要过 `restore_event_id`；倒排不存原文，写入方显式插入（ADR 0025）。`details` 按需读原文件。正文留在本机索引，备份与上传不含正文。上下文清单度量写入 `conversation_context_metrics`（条目名与数字，不含正文），备份时剔除，与 FTS 同等待遇。
2. Cursor Agent transcript 走 `conversation/cursor.rs`，`source=cursor_agent`，与 `cursor_sessions`（ADR 0007）分区。
3. 会话行注水只有 `conversation/hydrate.rs` 一条：源文件清单、消耗记录汇总、Cursor 会话补模型、工作纪要打标。目录页、正文搜索、单条详情共用它，别在某一条路上单独补一项。没有价目表时传 `None` 跳过消耗记录汇总。
4. `cargo test conversation` 绿（含增量、回填、正文搜索）。

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
2. 七个槽位数字只取消耗记录。Cursor 账号用量可另开分区（ADR 0023），不得并进槽位数字。
3. 新增时段聚合同步 `query.rs` 与 `aggregate.rs`，`cargo test parity` 绿。
4. 海报 CSS 只走 `src/report/*.css` 与 `posterStyleRegistry`（ADR 0019）。
5. 分享入口只出报告（ADR 0020）。读到 ADR 0018「周报 | 额度」时以 0020 为准。

## 工作纪要

读区间内的对话正文，经本机 CLI 总结成结构化条目。不是报告，不复用 `ReportPeriod`。

1. 入口三个：`work_notes::preview`（只读，收 `&Connection`）、`work_notes::generate`（区间纪要）、`work_notes::summarize_session`（单条会话摘要）。后两个收 `ConnectionSource` 而不是连接——「读连接取输入 → 放开连接调 CLI → 有东西可写才取写连接」这套编排在 `work_notes/pipeline.rs` 与 `session.rs` 里，command 不参与；`now`、runner、job、应用数据目录仍由调用方注入。`ConnectionSource` 的实现不得同时持有读与写 guard。command 只做 `begin`（互斥闸门，错误要同步回前端）→ spawn → 入口 → `finish`。生成是后台任务，进度写进 AppState，前端轮询。四种区间、60/150 规模闸门、成本预估在 Rust 判定，webview 只呈现。闸门按勾选后的会话数算。`WorkNotesParams.sessions` 为空表示区间内全部合格会话；非空则只处理这些，正文也只拉选中的。预览列出可勾选会话，不读正文。
2. runner 边界是「执行一条已经拼好的 `EngineCommand`，回一段 stdout」。argv 拼装（只读、禁审批、不落盘、schema、会落盘引擎的 session id）必须跑在这条缝里。
3. 本机 CLI 只读、禁工具，工作目录固定在应用数据目录下的专用空目录。不自建模型 HTTP 通路，不存密钥。
4. 硬数字只复用现有 `query` 函数，不新增聚合。
5. 读 ADR 0021、0011、0002。`cargo test work_notes` 绿。真 spawn 冒烟 `#[ignore]`，不进 CI。
6. 引擎清单只列 `which` 探测到的。探测只走设置页手动按钮（`which` + `--version`），启动时不 spawn。加引擎只改 `work_notes::engines` 的 profile 表，不改编排。
7. 会落盘的引擎必须可识别：能钉 session id 就钉死，否则靠专用工作目录。识别后只从后续纪要输入剔除、在对话记录打「码表生成」标记；不从 token KPI 扣除，不删改用户会话文件。判定规则与落痕表都在 `work_notes/identity.rs`，对话记录只经 `work_notes::decorate_sessions` 取结果，不自己拼规则。

## 推送协议

`crates/push-protocol/`：推送的线上传输格式，桌面端与远程服务共用。它是传输格式，不是平行数据模型（ADR 0026、0017）。

1. 只依赖 serde / sha2，不依赖 `mabiao`；桌面端把 `domain` 转换成它的类型，不反过来。
2. 改了字段语义或删了字段就递增 `PROTOCOL_VERSION`；只加带 `#[serde(default)]` 的可选字段不用。
3. 消耗记录指纹 `usage_fingerprint` 有钉死的期望值测试。改算法等于让历史指纹全部失配，必须同时递增 `FINGERPRINT_TAG`。
4. 服务端入口先调 `PushSessionRequest::validate`：版本检查，且只有 `injected` 层可带原文。
5. 它不在 workspace 里，`src-tauri` 的命令不覆盖它，用 `AGENTS.md` 里 `--manifest-path crates/<crate>/Cargo.toml` 那三条。

完成：三条命令绿，且 `cargo test --manifest-path src-tauri/Cargo.toml` 仍绿。

## 推送

桌面端把本机数据单向送到远程服务（ADR 0026）。这是码表唯一的出站数据通道。

1. 只在用户显式发起时发送；成员自己打开的每日自动推送默认关，不复用本机摄取定时器。
2. 不推：Cursor 账号用量、官方额度、代码量、全局指令。其它任何路径都不得把对话正文送出本机。
3. 正文与注入原文推送前按内置规则打码；`injected` 层原文推送时现场读源快照，不落库；`on_disk_possible` 层只推条目、路径与体积。
4. 密码不落盘；token 单独存 0600 文件，不进备份；强制 https，回环地址例外。
5. 线上格式在「推送协议」层；费用快照与定价来源用 `crates/pricing`（逐条 `price_usage`，批量用与它逐条等价的 `price_usage_cached`），不另写计价。
6. 读 ADR 0026、0011、0025。

### 远程服务登录与凭证（`src-tauri/src/remote_server/`）

1. 地址校验只有 `address::normalize_base_url` 一份（强制 https，`localhost` / 回环地址例外）；设置页提示、登录、之后每次联网都过它，前端不重写。
2. 联网只在 `client.rs`：不跟随重定向（密码与 token 不能被 307 带走），回环地址不走代理，错误一律翻成中文人话。webview 不直接请求远程服务。
3. 配置（`remote_server.json`：地址、账号、设备 ID、设备名）与 token（`remote_server_token.json`，0600，创建即 0600 再 `rename`；0600 仅 Unix 生效，Windows 沿用目录默认权限，与自定义提供商凭证一致）分两份文件；密码不落盘，`LoginInput` 的 Debug 不输出它。
4. 这两份文件都不在备份白名单里，也不被恢复覆盖：设备 ID 是「这台机器」的身份，不能跟着备份换机器。新增备份项时别把它们加进去。
5. token 只有服务端明确回 `token_expired` 才标成 `rejected` 并清掉；断网、5xx 不改本机登录态。
6. 测试：`cargo test remote_server`，对本机回环上的桩服务器跑真实 HTTP。

### 读会话、打码、分批发送（`src-tauri/src/push/`、`conversation/push_source.rs`）

入口是对话记录页的「推送」对话框：选区间 → 预览（`push::preview`，不联网）→ 确认后 `push::run`。

1. 选会话与读正文只在 `conversation::push_source`：区间按**重叠**整场选（结束 ≥ from 且开始 ≤ to，两端含，已归档也列出）；正文走事件索引，ADR 0025 外置正文按 `text_hash` 校验，读不回或对不上就**整场跳过**并给原因，不退回去推旧正文，也不推半截。事件 `details` 不出本机。
2. 上下文清单三层：`injected` 原文推送时现场读源快照（`context_content::injected_contents`），不落库；`on_disk_possible` 只推条目、路径、体积；快照已清理（`metrics_from_cache`）只推缓存度量并标 `from_cache`，不带原文。没有真实注入快照（Cursor 按磁盘重建）不得当注入原文推。
3. 打码只在 `push/redact.rs` 一处（标题、事件正文、注入原文都过它）；路径与项目名不打码。加规则必须同时补「该打」与「不该打」两类用例，并保持幂等。
4. git remote 只读 `<项目>/.git/config`（`push/git_remote.rs`），不跑 git；URL 里的凭据推送前剥掉。
5. 协议转换只在 `push/payload.rs`；`occurred_at` 不是 RFC 3339 的消耗记录本机挡掉（服务端会整批拒收）。
6. 一场会话一个请求；失败（可重试）与跳过（本机读不全）分开列。`token_expired` 立即停，剩下的记为失败并提示重登。同一时刻只允许一次预览或推送（命令层的 `RunGuard`）。本机历史只记数字，不记正文。
7. 测试：`cargo test push`（桩服务器在 `test_support/http_stub.rs`，与 `remote_server` 共用）；前端纯函数在 `src/lib/pushRange.ts`。

完成：「推送协议」层命令、`cargo test push`、`cargo test remote_server`、`pnpm test` 与 `src-tauri` 全量测试绿。

## 远程服务

`server/`：axum + sqlx + PostgreSQL，独立 Cargo 项目，不在 workspace 里。部署文件在 `deploy/`。

1. **没有注册接口。** 账号只能由命令行 `mabiao-server create-admin` 或管理员接口创建；管理员接口只建成员。账号只停用不删除，停用时立即吊销该账号全部 token。
2. 密码用 argon2id 哈希；token 是 256 位随机数，库里只存 SHA-256，有效期 30 天。登录对「账号不存在 / 密码错 / 已停用」返回同一个错误。
3. 数据隔离只有一个判定：`AuthedAccount::can_access`。每个按账号归属的数据接口先过它；管理员专属 handler 用 `AdminAccount` 参数。新增接口必须配「成员访问别人数据被拒」的测试。
4. 凡是 body 带 `protocol_version` 的请求（登录、两个推送接口）先过 `push_protocol::check_protocol_version`，新增这类接口要补不兼容版本的测试；错误体一律是 `push_protocol::ApiError`。
5. SQL 用运行时的 `sqlx::query`，不用 `query!` 宏，免得编译要连库。迁移在 `server/migrations/`，已有迁移文件不改，改 schema 只加新文件。
6. 测试连真 PostgreSQL（`DATABASE_URL`），`#[sqlx::test]` 每个测试一个临时库，连接用户要能建库。
7. 命令在 `AGENTS.md`；CI 的 `server` 作业同时 `docker build -f server/Dockerfile .`。

### 推送接收

接口：`POST /api/v1/push/session`、`POST /api/v1/push/usage`、`DELETE /api/v1/sessions/{id}`、`GET /api/v1/admin/coverage`。

1. 账号永远取自 token，body 里没有账号字段。会话键是 (账号, 设备, 来源, session_id)，同一个 `session_id` 在不同设备、成员、来源下是不同的行。
2. 会话整场覆盖：同键再推就替换整条（变长、变短都一样），events 存 JSONB，不做增量合并。
3. 消耗记录按 `usage_fingerprint` 去重（账号 + 设备 + 指纹唯一，`ON CONFLICT DO NOTHING`）。费用快照与定价来源按客户端发来的原样存，不改写；统一费用另算另存（见「团队价目与统一费用」）。一批里有一条非法就整批拒绝。
4. 项目：优先按规范化 git remote 归并（去凭据、协议、端口、大小写、`.git`，凭据不入库），没有 remote 用目录名兜底，都没有则不挂项目。同一路径原先只有目录兜底、后来拿到 git remote 时，`projects::point_path_at` 把历史会话与用量一并改挂；已归到 git 项目的路径不降级，路径换了 remote 也只影响之后的推送，旧历史留在旧项目。原始路径保留在 `project_paths`。
5. 「最后推送时间」「已覆盖到哪天」由 `coverage.rs` 实时从数据算，不单独存，删除后自动跟着变。
6. 删除：成员删自己的，管理员删任意的（走 `can_access`）；只删会话，不动消耗记录和项目。服务端不自动删任何东西；桌面端本机删除不得触发远端删除。
7. 推送路由单独放宽 body 上限（`PUSH_BODY_LIMIT_BYTES`），消耗记录单批 ≤ `MAX_USAGE_RECORDS_PER_REQUEST`。
8. 收窄测试：`cargo test --manifest-path server/Cargo.toml --test push`。

### 团队价目与统一费用（`server/src/team_pricing.rs`、`usage_query.rs`）

接口：`GET /api/v1/admin/pricing`、`PUT /api/v1/admin/pricing/prices`、`DELETE /api/v1/admin/pricing/prices/{id}`、`POST /api/v1/admin/pricing/recompute`、`GET /api/v1/usage`。

1. 计价规则只在 `crates/pricing/`，服务端不另写：逐条一律走 `price_usage_cached`，不开签名模糊匹配（那是 Cursor 账号事件专用）。优先级：来源自带 `native_cost` > 团队价目精确匹配 > 按 model 兜底（团队价目或内置快照）> 未定价。改优先级先改那个 crate。
2. 生效价表 = 团队价目 + 内置 LiteLLM 快照，拼法 `effective_table` 与桌面端 `litellm::merge` 同一规则（团队配了某模型的任意单价，该模型就不再引入快照兜底；按大小写敏感的 model 名排除，是有意与桌面端一致的）。快照文件与桌面端共用 `src-tauri/assets/litellm_prices.json`（`include_str!`），所以 Docker 上下文要带它，不要在 `server/` 另放一份。
3. 统一费用存在 `usage_records.unified_cost` / `unified_pricing_source` / `unified_cost_source` 三列，**不覆盖**客户端的 `cost_snapshot` / `pricing_source`；查询接口两组并列返回。三列全空表示「没算过」。
4. 什么时候算：推送消耗记录入库时算；管理员改 / 删团队价目时，在同一事务里重算同名模型（不分大小写）已入库的记录；`POST …/recompute` 全量重算（升级后内置快照变了时用）；`serve` 启动时补算「没算过」的旧数据。
5. 推送消耗记录取咨询锁的共享锁，改价目与重算取排他锁（`lock_shared` / `lock_exclusive`）：重算期间进来的推送要等它提交，不会带着旧价目的结果落库后无人再算。新增写 `usage_records` 的路径要先取共享锁、用 `load_table` 读价表。
6. 价目接口全是管理员专属（`AdminAccount`）。`GET /api/v1/usage` 走 `can_access`：成员只看自己、指定别人的 `account_id` 得 403，管理员默认看全体；`from` 含、`to` 不含，`totals` 是过滤条件下的全部合计、不受分页影响。
7. 单价是每 token 的价格，范围 0 到 `MAX_PRICE_PER_TOKEN`，挡掉「每百万 token」误填。
8. 收窄测试：`cargo test --manifest-path server/Cargo.toml --test pricing`。

完成：fmt、clippy、`cargo test --manifest-path server/Cargo.toml` 三条绿。

### 网页用的聚合与会话列表（`server/src/summary.rs`、`sessions::list`）

接口：`GET /api/v1/usage/summary`、`GET /api/v1/sessions`。

1. 聚合在服务端做，网页只展示。`summary` 只对 `usage_records` 求和（统一费用、客户端快照并列），不另算价格；改计价先改 `crates/pricing/` 与 `team_pricing`。
2. 两个接口都走 `routes::scope_account`（即 `can_access`）：成员只看自己、指定别人的 `account_id` 得 403，管理员默认看全体。新增同类接口要复用它并配越权测试。
3. `from` 含、`to` 不含，RFC 3339；按天的日界由 `tz_offset_minutes`（-720 到 840，东为正）决定，默认 UTC。每个维度最多 `MAX_BREAKDOWN_ROWS` 行，按统一费用从高到低。
4. 会话列表只给目录元数据，永远不带 `events` 与上下文清单；正文属于会话详情。
5. 收窄测试：`cargo test --manifest-path server/Cargo.toml --test summary`。

### 管理网页（`server/web/`）

React + Vite + Tailwind，独立前端项目（自己的 `package.json` 与 `pnpm-lock.yaml`，不在根 pnpm 里，根 `pnpm lint` 忽略它）。TypeScript strict、禁止 `any`，单文件 ≤ 400 行由 eslint `max-lines` 卡。

1. 响应类型在 `src/api/types.ts`，与 `server/src/api.rs` 一一对应；改了服务端响应就同步改它。
2. 登录后 token 只放 `sessionStorage`。401 一律退回登录页；登录请求自己的 401 不算。
3. hash 路由（`#/overview`、`#/members`、`#/member/:id`），所以服务端静态托管不做「未知路径回退 index.html」。成员越权的路由被 `lib/route.ts::allowedRoute` 拉回自己页面，真正的拦截仍在服务端。
4. 费用口径（统一价 / 客户端快照）只改展示，不重新请求。导出 CSV 两种费用都带；`lib/csv.ts` 会给以 `= + - @` 开头的文本加 `'`，防公式注入。
5. 服务端用 `--web-dir` / `MABIAO_WEB_DIR` 托管 `dist`（`router_with_web`）；镜像里已设好。静态托管测试：`cargo test --manifest-path server/Cargo.toml --test web`。
6. 在 `server/web/` 下跑：`pnpm install --frozen-lockfile`、`pnpm lint`、`pnpm test`、`pnpm build`。

完成：服务端三条命令绿，且网页四条命令绿。

## 样式

1. 按 `src/styles/` 分层、按域拆文件。入口 `src/styles.css` 只含 `@import`。
2. 单文件 ≤ 400 行。门禁是 `src/lib/cssStructure.ts` + Vitest。
3. 海报 CSS 走「报告」。读 ADR 0016。`pnpm test` 绿。

## Rust 模块边界

1. 生产代码 800 行软红线。`domain` 子模块全量 re-export。
2. Tauri command 注册列表留在 crate 根。纯重构保持 `store::ADAPTER_VERSION` 与 `CONVERSATION_ADAPTER_VERSION` 不变。
3. 读 ADR 0017。`cargo clippy` 与相关测试绿。
