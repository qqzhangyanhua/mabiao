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
| 前端纯函数 | `pnpm test` | format、exportRows、viewCache、价目表等 |
| Rust 适配器 | `cargo test adapters` | 各 Source fixture → UsageRecord |
| 聚合 SQL parity | `cargo test parity` | `query.rs` vs `aggregate.rs` 逐字段对照 |
| 摄取缓存 | `cargo test ingest` | tempfile 模拟 home，不读真实 `~/.*` |
| Cursor 会话/账号 | `cargo test cursor` | transcript / 账号 JSON fixture |
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
2. 实现 `src-tauri/src/adapters/<source>.rs`
3. 添加脱敏 fixture → `src-tauri/tests/fixtures/`
4. 在 `tests/adapters.rs` 加单测（去重/累计口径必断言）
5. **递增** `store.rs::ADAPTER_VERSION`（否则旧缓存不重解析）

### 修改聚合 / 费用 SQL（`query.rs`）

1. 同步更新 `aggregate.rs` 等价实现
2. 跑 `cargo test parity`（`sql_queries_match_in_memory_aggregates`）
3. 费用优先级：`native_cost` > 用户价目 > LiteLLM 快照 > unpriced

### 修改前端 DTO

1. 先改 Rust `domain.rs`，再改 `src/types.ts`（字段名保持 snake_case）
2. `pnpm build` 必须通过 strict TS

### 修改摄取 / 备份

1. 跑 `cargo test ingest` 与 `cargo test backup`（若有）
2. 对照 `docs/adr/0003-trusted-ingestion-cache.md`

## 领域词汇（简述）

- **消耗记录 (Usage Record)**：归一化 token 条目，定义在 `domain.rs`
- **来源 (Source)**：codex、claude、pi… 不要用「工具/渠道」
- **代码量 (Code Volume)**：Cursor 行数统计，与 token 严格分区
- **官方额度 (Official Quota)**：账号级订阅限额，成员含内置账号（Claude / Codex / Cursor / Grok）与用户登记的**自定义提供商**，不进总览 token KPI
- **Cursor 会话**：agent-transcripts 行为统计，不进总览 token KPI

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
