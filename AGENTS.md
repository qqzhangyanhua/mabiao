# AGENTS.md — Cloud Agent / CI 开发指南

本仓库是 **Tauri 2 桌面应用**（Rust 核心 + React webview）。Cloud Agent 环境通常**没有**本机 AI CLI 会话数据，也**无法**启动完整 GUI 做端到端点击测试。请按下面分层验证改动。

## 必跑命令（每次改代码后）

```bash
pnpm install --frozen-lockfile   # 首次或 lockfile 变更后
pnpm lint
pnpm test                        # Vitest，src/lib/*.test.ts
pnpm build                       # tsc + vite build

cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

依赖安装统一用 **pnpm**（不要用 npm/yarn）。

## 什么可以在 Cloud / CI 里测

| 层级 | 命令 | 覆盖 |
|------|------|------|
| 前端纯函数 | `pnpm test` | format、exportRows、viewCache、价目表等（`src/lib/*.test.ts` 与 `src/hooks/*.test.ts`） |
| Rust 适配器 | `cargo test adapters` | 各 Source fixture → UsageRecord |
| 聚合 SQL parity | `cargo test parity` | `query.rs` vs `aggregate.rs` 逐字段对照 |
| 摄取缓存 | `cargo test ingest` | tempfile 模拟 home，不读真实 `~/.*` |
| Cursor 会话/账号 | `cargo test cursor` | transcript / 账号 JSON fixture |
| 官方额度 | `cargo test quota` | 各家响应 fixture、退避、告警去重、自定义提供商 |
| 对话记录 | `cargo test conversation` | 事件索引、增量、分页、正文 FTS、各来源正文解析 |
| 全局指令 | `cargo test instructions` | 加载判定、冲突、体检、白名单写入 |
| 备份 / 恢复 | `cargo test backup` | 往返、拒绝坏包、legacy 包兼容 |
| 构建 | `pnpm build` | 类型检查 + chunk 拆分 |

Rust 测试按模块拆分在 `src-tauri/src/tests/`，共享辅助函数在 `src-tauri/src/test_support/`。

## 什么不能 / 不应在 Cloud 里测

- **`pnpm tauri dev` / `pnpm tauri build` / 菜单栏托盘**：需图形会话与本机数据；Cloud 上跳过 GUI walkthrough 与完整打包。跨平台安装包由 `.github/workflows/release.yml` 在 GitHub-hosted runner 上打（见 `docs/platforms.md`）。
- **真实 `~/.codex`、`~/.claude` 等路径**：禁止依赖；用 `src-tauri/tests/fixtures/` + `tempfile`（见 `ingest_all_fixtures_is_stable_on_refresh`）。
- **Cursor 账号联网拉取**：需本机 Cursor 客户端登录态；只测 parser 与 store fixture。
- **Probe 本机字段**：`cargo run --bin probe` 仅在开发者机器上跑，结果写入 `docs/probe/`。

## 改不同层时的检查清单

### 新增 / 修改 Adapter

1. 在 `domain.rs::Source` 注册变体
2. 实现 `src-tauri/src/adapters/<source>.rs`（扫描目录、发现、解析；需要时再加辅助指纹 / 目录级或文件级派生上下文）
3. 在 `adapters/mod.rs` 的 `USAGE_ADAPTERS` 表加一行，含路径环境变量（漏填会让完备性测试失败）
4. 添加脱敏 fixture → `src-tauri/tests/fixtures/`
5. 在 `tests/adapters.rs` 加单测（去重/累计口径必断言）
6. 若改了归一化输出，**递增** `store.rs::ADAPTER_VERSION`（否则旧缓存不重解析）

不要在 `ingest.rs` 里按来源加分叉。

### 修改聚合 / 费用 SQL（`query.rs`）

1. 同步更新 `aggregate.rs` 等价实现
2. 跑 `cargo test parity`（`sql_queries_match_in_memory_aggregates`）
3. 费用优先级：`native_cost` > 用户价目 > LiteLLM 快照 > unpriced

### 修改前端 DTO

1. 先改 Rust `domain.rs`，再改 `src/types.ts`（字段名保持 snake_case）
2. `pnpm build` 必须通过 strict TS

### 修改摄取 / 备份

1. 跑 `cargo test ingest` 与 `cargo test backup`
2. 对照 `docs/adr/0003-trusted-ingestion-cache.md`

### 修改官方额度 / 自定义提供商

1. 内置九家在 `domain.rs::OfficialQuotaProvider::ALL`（Claude / Codex / Cursor / Grok / Droid / Antigravity / OpenCode / Copilot / Devin）；新增一家要同时补 `official_quota/<provider>.rs`、`detect.rs` 的凭证探测与 `fetch.rs` 的分派
2. 凭证只读各客户端本机已有的登录态，不要加手动粘贴通路、不要写钥匙串；自定义提供商的密钥单独存一份文件，**不进备份**
3. 用 fixture 测响应解析，不打真实接口（Cloud 上没有登录态）；跑 `cargo test quota`
4. 对照 `docs/adr/0008-official-quota-dimension.md`、`0012-custom-quota-providers.md`、`0013-custom-quota-implemented-presets.md`

### 修改对话记录 / 事件索引

1. 事件 `text`/`name` 进 `conversation_events`（ADR 0011），目录搜索走 FTS 派生表（ADR 0014）。`details` 仍按需读原文件、不进索引。正文不进备份、不上传
2. 跑 `cargo test conversation`（含增量、回填与正文搜索）
3. 对照 `docs/adr/0011-conversation-event-index.md`、`0014-conversation-body-search.md`

### 修改报告 / 洞察

1. 洞察规则只写 Rust `report` 模块；前端 `reportCopy` 只把 payload 映射成文案，不算数、不排名、不选槽位
2. 报告数字只来自消耗记录；禁止把代码量、官方额度、Cursor 账号用量并进 token 口径
3. 新增时段聚合必须同步 `query.rs` 与 `aggregate.rs`，跑 `cargo test parity`
4. 海报 CSS 不得使用 `color-mix` / `backdrop-filter`，不得复用主样式表
5. 对照 `docs/adr/0015-report-and-insights.md`

### 修改可分享卡片 / 官方额度卡

1. 周报口径仍走 ADR 0015；额度卡另读官方额度出口，禁止把百分比并进 token，禁止为分享新开取数通道
2. 额度卡 view model 是前端纯函数：webview 不算窗口百分比、不算撞线、不自己定义过期阈值
3. 海报 CSS 同样禁止 `color-mix` / `backdrop-filter`，不得复用主样式表
4. 对照 `docs/adr/0018-shareable-cards.md`

## 领域词汇（简述）

- **消耗记录 (Usage Record)**：归一化 token 条目，定义在 `domain.rs`
- **来源 (Source)**：codex、claude、pi… 不要用「工具/渠道」
- **代码量 (Code Volume)**：Cursor 行数统计，与 token 严格分区
- **官方额度 (Official Quota)**：账号级订阅限额，成员含内置九家账号（Claude / Codex / Cursor / Grok / Droid / Antigravity / OpenCode / Copilot / Devin）与用户登记的**自定义提供商**，不进总览 token KPI
- **Cursor 会话**：agent-transcripts 行为统计，不进总览 token KPI
- **对话记录 (Conversation Record)**：目录元数据 + 事件正文索引（可搜提问/回复/工具名与输出）；不进备份、不进 token KPI
- **全局指令 (Global Instruction)**：某个 Source 真正会跨项目加载的用户手写指令，不进 token KPI；避免用「规则 / 记忆」
- **工作时间线 (Work Timeline)**：单日会话区间铺开，不是又一份 token KPI
- **报告 (Report)**：已结束自然周期内消耗记录的可分享海报；不是独立数据维度，不把代码量 / 官方额度 / Cursor 账号用量并进 token
- **洞察 (Insight)**：报告里的结构化事实，Rust 产生、前端只措辞
- **可分享卡片 (Share Card)**：为转发而排版的图（周报或官方额度卡）；共用剪贴板，数字不合并；不是模板

详见 `CONTEXT.md` 与 `docs/adr/`。各平台构建与托盘差异见 `docs/platforms.md`。

## 分支与 PR

- 功能分支命名：`cursor/<描述>-eedd`
- 推送：`git push -u origin <branch>`
- PR 默认 **draft**；CI 绿后再 mark ready

## 发版

1. 同步 `package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml` 的 version
2. 推送 `v*` tag，或在 Actions 手动跑 **Release**
3. 检查 draft Release 的 macOS / Linux / Windows 产物后 Publish
4. Cloud / 本机 CI **不要**用 `pnpm tauri build` 代替这条流水线
