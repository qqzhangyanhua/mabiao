# AGENTS.md — Cloud / CI

Tauri 2（Rust 核心 + React webview）。Cloud / CI 没有本机 AI CLI 数据，也没有 GUI。

## 每次改代码

```bash
pnpm install --frozen-lockfile   # lockfile 变了才跑
pnpm lint
pnpm test
pnpm build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

包管理只用 pnpm。以上全部通过才算改完。迭代时按层收窄 Rust 测试：

| 层 | 命令 |
|----|------|
| Adapter | `cargo test adapters` |
| 聚合 / 报告时段 | `cargo test parity` |
| 摄取 | `cargo test ingest` |
| Cursor 会话 / 账号 | `cargo test cursor` |
| 官方额度 | `cargo test quota` |
| 对话记录 | `cargo test conversation` |
| 全局指令 | `cargo test instructions` |
| 备份 | `cargo test backup` |
| 工作纪要 | `cargo test work_notes` |

Rust 测试在 `src-tauri/src/tests/`，辅助在 `src-tauri/src/test_support/`。

## Cloud 跳过

- 桌面与安装包：不跑 `pnpm tauri dev` / `pnpm tauri build`。安装包由 `.github/workflows/release.yml` 打，平台差异见 `docs/platforms.md`。
- 会话数据用 `src-tauri/tests/fixtures/` + `tempfile`（`ingest_all_fixtures_is_stable_on_refresh`），不读 `~/.codex`、`~/.claude` 等家目录。
- Cursor 账号与官方额度只跑 fixture。Probe（`cargo run --bin probe`）只在开发者机器跑，结果在 `docs/probe/`。

## 层清单

改 **Adapter**、**聚合**、**DTO**、**摄取**、**扫描路径**、**Cursor 账号**、**Cursor 会话**、**官方额度**、**对话记录**、**全局指令**、**用户文件**、**报告**、**工作纪要**、**样式**、**Rust 模块边界** 时，读 [`docs/agent-layers.md`](docs/agent-layers.md)，做完该层每一项，再跑对应测试。动手前读该层点名的 ADR。

写用户拥有的文件只走 ADR 0010（mtime 校验、写前备份、原子 rename、白名单）。其它路径只读。

术语以 [`CONTEXT.md`](CONTEXT.md) 为准。写到 **消耗记录**、**来源**、**Adapter**、**代码量**、**官方额度**、**Cursor 会话**、**对话记录**、**全局指令**、**工作时间线**、**报告**、**洞察**、**工作纪要**、**纪要引擎** 时，先对过对应词条。决策以 [`docs/adr/`](docs/adr/) 为准。

## 分支、PR、发版

- 功能分支：`cursor/<描述>-eedd`，`git push -u origin <branch>`
- PR 先 draft，CI 绿再 mark ready
- 发版：同步 `package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml` 的 version → 推 `v*` tag 或 Actions 手动 **Release** → 检查 draft 的 macOS / Linux / Windows 产物后 Publish。安装包只由这条流水线打。
