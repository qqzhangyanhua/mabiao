# Mabiao (码表)

[中文](README.md)

A local desktop app that scans AI coding CLI session files on your machine, normalizes them into **usage records**, and shows token usage with optional cost. **Read-only by default. Local usage records are not uploaded.**

[![CI](https://github.com/qqzhangyanhua/mabiao/actions/workflows/ci.yml/badge.svg)](https://github.com/qqzhangyanhua/mabiao/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

> Screenshots are not in the repo yet. Export Overview / App breakdown / Settings from a running window into `docs/screenshots/` and open a PR.

## Supported sources

Mabiao reads local directories. It does not replace vendor dashboards. Scan roots can be overridden with environment variables; see [`docs/adr/0005-configurable-source-paths.md`](docs/adr/0005-configurable-source-paths.md).

| Source | Local tokens | Local cost | Notes |
|------|:---:|:---:|------|
| Claude Code | ✅ | ✅ | Native `costUSD` |
| Codex | ✅ | ❌ | |
| pi | ✅ | ✅ | |
| opencode | ✅ | ✅ | |
| grok | ✅ | ✅ | |
| kimi / gemini / dsh / copilot | ✅ | ❌ | dsh needs decompress; copilot totals only at session end |
| Factory / droid | ✅ | ❌ | Session totals, no model name |
| Cursor | ⚠️ | ❌ | Code volume + account usage (you paste existing credentials) |
| cursor-agent | ⚠️ | ❌ | Tokens only after a wrapper writes them to disk |
| qwen / amp | ❌ | ❌ | No local tokens (amp usage is cloud-only) |

Dimensions, default paths, and limits: [`CONTEXT.md`](CONTEXT.md).

## Features

- **Overview and breakdowns**: trends by hour / day / week / month; split by source, model, provider, project, and session; export CSV / JSON; save charts as images
- **Quotas stay separate**: local 5-hour / 7-day estimate windows (not official caps) vs Claude / Codex / Cursor / Grok **official quota**; menubar shows today's spend and the tightest official percentage
- **Cursor code volume**: editor AI line counts are a separate dimension, not mixed into token KPIs
- **Local cache**: sqlite backup / restore / per-source rebuild; records stay archived (and counted) after source files rotate away
- **Budget alerts**: optional monthly budget (USD); one system notification per threshold for local estimates or official quota

## Data and privacy

- Default scan is read-only. **Local usage records are not uploaded**
- Aggregates live in local sqlite; backup, restore, or rebuild from Settings
- Cursor account usage and some official quotas need credentials you already have locally; session bodies are not rewritten
- Cursor session tokens go to the macOS Keychain (`keyring` is `apple-native` only). That path may be unavailable in Windows / Linux builds

## Install

GitHub Actions attaches installers to [Releases](https://github.com/qqzhangyanhua/mabiao/releases). **v0.1.0 and v0.1.1 are still drafts**, so the public Releases page has no assets. Run from source until a release is published.

| Platform | Artifact |
|------|------|
| macOS Apple Silicon | `.dmg` (`aarch64-apple-darwin`) |
| macOS Intel | `.dmg` (`x86_64-apple-darwin`) |
| Linux x64 | `.deb` or AppImage |
| Windows x64 | NSIS `.exe` |

Builds are **unsigned**. On first macOS open, right-click → Open in Finder, or:

```bash
xattr -cr "/Applications/Mabiao.app"
```

Windows SmartScreen may block the installer; choose “Run anyway”. Menubar and keychain treat macOS as first-class; Linux / Windows differences are in [`docs/platforms.md`](docs/platforms.md).

## Build from source

Prerequisites:

- Node.js 20+
- [pnpm](https://pnpm.io/) 9 (pinned to `pnpm@9.15.0`; do not use npm / yarn)
- [Rust](https://rustup.rs/) stable
- Linux also needs WebKitGTK and related system libs; see [`docs/platforms.md`](docs/platforms.md)

```bash
pnpm install
pnpm tauri dev
```

A native window titled 「码表」 should open. Local installer:

```bash
pnpm tauri build
```

Ship cross-platform packages through the GitHub Release workflow. Do not treat a local `pnpm tauri build` as a release.

## Development

```bash
pnpm lint
pnpm lint:fix
pnpm format
pnpm test       # Vitest (pure functions in src/lib)
pnpm build      # tsc + vite build
```

Re-run local token-field probing (developer machine only):

```bash
cargo run --bin probe --manifest-path src-tauri/Cargo.toml
```

| Doc | What it covers |
|------|------|
| [`CONTEXT.md`](CONTEXT.md) | Domain terms and per-source coverage |
| [`docs/platforms.md`](docs/platforms.md) | Cross-platform build and release |
| [`docs/adr/`](docs/adr/) | Architecture decisions |
| [`AGENTS.md`](AGENTS.md) | How to test and add an adapter |
| [`docs/probe/`](docs/probe/) | Local field-probe notes |

## Contributing

Issues and PRs are welcome. A new source is a new adapter: register it in `domain.rs`, implement `src-tauri/src/adapters/<source>.rs`, add redacted fixtures and tests, and bump `ADAPTER_VERSION`. See [`AGENTS.md`](AGENTS.md) for the checklist. Open PRs as drafts and mark ready after CI is green.

## License

[MIT](LICENSE)
