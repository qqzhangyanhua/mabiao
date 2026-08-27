# 跨平台构建与运行

码表基于 **Tauri 2**，核心逻辑跨平台；菜单栏托盘以 **macOS** 为一等公民。安装包由 [`.github/workflows/release.yml`](../.github/workflows/release.yml) 在 GitHub Actions 上打好，挂到 [Releases](https://github.com/qqzhangyanhua/mabiao/releases)。

## 支持矩阵

| 平台 | GitHub 打包 | 菜单栏托盘 | 说明 |
|------|-------------|------------|------|
| macOS Apple Silicon / Intel | ✅ `.dmg` | ✅ | 关闭窗口后托盘继续刷新今日花费；未签名，需右键打开 |
| Linux x64 | ✅ `.deb` / AppImage | ⚠️ 未专门适配 | CI 与 Release 都装 `webkit2gtk`；托盘可能表现为状态栏图标 |
| Windows x64 | ✅ NSIS `.exe` | ❌ | `windows_subsystem = "windows"` 隐藏控制台；无 macOS `Reopen` / template icon |
| Linux ARM | ❌ 未纳入矩阵 | ⚠️ | 公开仓库可用 `ubuntu-22.04-arm`，需要时再加 |

官方额度的凭证都是读各客户端本机已有的登录态，跨平台可用；例外是 Antigravity——它的登录态由 zalando go-keyring 写进 macOS 钥匙串，`macos_keychain_password()` 在非 macOS 上直接返回 `None`，所以这一家在 Windows / Linux 上只能靠客户端 `state.vscdb` 里那份，读不到就整行不可用。其余本机文件摄取不受影响。

## macOS（推荐）

```bash
pnpm install
pnpm tauri dev      # 开发
pnpm tauri build    # 发布 .app
```

## Linux

依赖（与 CI 一致，Debian/Ubuntu 示例）：

```bash
sudo apt-get install -y \
  libwebkit2gtk-4.1-dev libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev patchelf \
  xdg-utils libfuse2
```

```bash
pnpm install
pnpm tauri build
```

产物格式取决于 Tauri bundle 配置（`.deb` / AppImage 等）。**托盘**：代码使用 Tauri tray API，Linux 上可能表现为状态栏图标，但未作为一等公民测试。

## Windows

需安装 [Tauri 前置依赖](https://v2.tauri.app/start/prerequisites/)（WebView2、VS Build Tools 等）。

```bash
pnpm install
pnpm tauri build
```

`main.rs` 在 release 下使用 `windows_subsystem = "windows"` 隐藏控制台窗口。无 macOS 专属 `Reopen` 处理，点击任务栏图标行为取决于系统默认。

## GitHub Actions 打包

工作流：`.github/workflows/release.yml`（`tauri-apps/tauri-action@v1`）。

**触发**

1. 把 `package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml` 的 `version` 改成同一号（例如 `0.1.0`）
2. 推送 tag：`git tag v0.1.0 && git push origin v0.1.0`
3. 或在 GitHub **Actions → Release → Run workflow** 手动跑（会按配置里的 version 建 `v__VERSION__` tag）

产物写入 **draft** Release「码表 vX.Y.Z」，同时上传 Actions artifact（保留 14 天）。核对 dmg / deb / exe 无误后再在 Releases 页点 Publish。

**仓库设置**

- Settings → Actions → General → Workflow permissions → **Read and write permissions**（否则 `tauri-action` 无法建 Release）
- 当前**不要求** Apple / Windows 签名 secret；需要公证时再补 `APPLE_*` / 证书类环境变量
- 未做自动更新（`includeUpdaterJson: false`），没有 updater 插件

**本机对照**

```bash
pnpm install
pnpm tauri build
# macOS 指定架构：
pnpm tauri build -- --target aarch64-apple-darwin
pnpm tauri build -- --target x86_64-apple-darwin
```

## 开发约定

- 包管理：**pnpm**（`tauri.conf.json` 的 `beforeDevCommand` / `beforeBuildCommand` 亦使用 `pnpm run`）
- 新增平台相关 UI 时，用 `#[cfg(target_os = "...")]` 隔离，并在本文件更新支持矩阵
- Cloud Agent / CI 详见根目录 `AGENTS.md`；Cloud 上不要跑 `pnpm tauri build` / Release 工作流
