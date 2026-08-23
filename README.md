# 码表 (Mabiao)

[English](README.en.md)

本机桌面应用：扫描各 AI 编程 CLI 留在本地的会话数据，归一成「消耗记录」，展示 token 用量与可选费用。**默认只读、不上传本机消耗记录。**

[![CI](https://github.com/qqzhangyanhua/mabiao/actions/workflows/ci.yml/badge.svg)](https://github.com/qqzhangyanhua/mabiao/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

> 还没有产品截图。欢迎从本机窗口导出总览 / 应用拆分 / 设置页，放到 `docs/screenshots/` 后开 PR。

## 支持的来源

码表扫的是本机目录，不代替各家官方仪表盘。路径可用环境变量覆盖，详见 [`docs/adr/0005-configurable-source-paths.md`](docs/adr/0005-configurable-source-paths.md)。

| 来源 | 本机 Token | 本机费用 | 说明 |
|------|:---:|:---:|------|
| Claude Code | ✅ | ✅ | 自带 `costUSD` |
| Codex | ✅ | ❌ | |
| pi | ✅ | ✅ | |
| opencode | ✅ | ✅ | |
| grok | ✅ | ✅ | |
| kimi / gemini / dsh / copilot | ✅ | ❌ | dsh 需解压；copilot 仅会话结束时累计 |
| Factory / droid | ✅ | ❌ | 会话累计，无模型名 |
| Cursor | ⚠️ | ❌ | 代码量 + 账号级用量（需主动提供已有凭证） |
| cursor-agent | ⚠️ | ❌ | Token 仅在包装落盘后可读 |
| qwen / amp | ❌ | ❌ | 本机无 Token（amp 用量在云端） |

口径、默认路径与限制见 [`CONTEXT.md`](CONTEXT.md)。

## 主要能力

- **总览与拆分**：按时 / 日 / 周 / 月看趋势，按来源、模型、Provider、项目、会话拆分；可导出 CSV / JSON，图表可另存图片
- **额度分开展示**：本机 5 小时 / 7 天估计窗（非官方配额）与 Claude / Codex / Cursor / Grok **官方额度**分开；菜单栏显示今日花费和最紧的官方百分比
- **Cursor 代码量**：编辑器 AI 行数独立统计，不并入 Token KPI
- **本机缓存**：sqlite 可备份、恢复、按来源重建；源文件被清理后记录归档仍计入，不会静默消失
- **预算通知**：可设月度预算（美元）；本机估算或官方额度过阈值时各弹一次系统通知

## 数据与隐私

- 默认只读扫描本机各来源会话目录，**不上传本机消耗记录**
- 聚合结果缓存在本机 sqlite，可在设置页备份、恢复或重建
- Cursor 账号用量与部分官方额度需要你主动提供本机已有凭证；不会改写会话正文
- Cursor 会话 token 写入 macOS 钥匙串（`keyring` 仅启用 `apple-native`）；Windows / Linux 打包后该入口可能不可用

## 下载安装

安装包由 GitHub Actions 打好后挂在 [Releases](https://github.com/qqzhangyanhua/mabiao/releases)。**当前 v0.1.0 / v0.1.1 仍是 draft**，公开页上看不到产物；正式 Publish 之前请从源码运行。

| 平台 | 产物 |
|------|------|
| macOS Apple Silicon | `.dmg`（`aarch64-apple-darwin`） |
| macOS Intel | `.dmg`（`x86_64-apple-darwin`） |
| Linux x64 | `.deb` 或 AppImage |
| Windows x64 | NSIS `.exe` |

当前构建**未做代码签名**。macOS 首次打开若提示无法验证开发者，在访达中右键 → 打开，或：

```bash
xattr -cr "/Applications/Mabiao.app"
```

Windows 可能被 SmartScreen 拦截，选择「仍要运行」即可。托盘与钥匙串以 macOS 为一等公民，Linux / Windows 差异见 [`docs/platforms.md`](docs/platforms.md)。

## 从源码构建

前置：

- Node.js 20+
- [pnpm](https://pnpm.io/) 9（仓库钉在 `pnpm@9.15.0`，请勿用 npm / yarn）
- [Rust](https://rustup.rs/) stable
- Linux 还需 WebKitGTK 等系统库，见 [`docs/platforms.md`](docs/platforms.md)

```bash
pnpm install
pnpm tauri dev
```

开发时会弹出原生窗口，标题为「码表」。本机打安装包：

```bash
pnpm tauri build
```

跨平台安装包请走 GitHub Release 流水线，不要用本机 `pnpm tauri build` 代替发版。

## 开发

```bash
pnpm lint       # ESLint
pnpm lint:fix   # 自动修复
pnpm format     # Prettier
pnpm test       # Vitest（src/lib 纯函数）
pnpm build      # tsc + vite build
```

复跑本机 token 字段探测（仅开发者机器）：

```bash
cargo run --bin probe --manifest-path src-tauri/Cargo.toml
```

| 文档 | 内容 |
|------|------|
| [`CONTEXT.md`](CONTEXT.md) | 领域词汇、各来源采集现状 |
| [`docs/platforms.md`](docs/platforms.md) | 跨平台构建、GitHub 打包与发版 |
| [`docs/adr/`](docs/adr/) | 架构决策 |
| [`AGENTS.md`](AGENTS.md) | 怎么测、怎么加 Adapter |
| [`docs/probe/`](docs/probe/) | 本机字段探测记录 |

## 贡献

欢迎 Issue 和 PR。新增来源 = 新增 Adapter：在 `domain.rs` 注册，实现 `src-tauri/src/adapters/<source>.rs`，加脱敏 fixture 与测试，并递增 `ADAPTER_VERSION`。验证命令与分层检查见 [`AGENTS.md`](AGENTS.md)。PR 建议先开 draft，CI 绿后再 mark ready。

## 许可证

[MIT](LICENSE)
