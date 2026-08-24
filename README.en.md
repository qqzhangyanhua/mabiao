# Mabiao (码表)

[中文](README.md) · [Website](https://mabiao.dev)

A local desktop app that scans AI coding CLI session files on your machine, normalizes them into **usage records**, and shows token usage, a work timeline, and the full event stream. **Read-only by default. Local usage records are not uploaded.**

[![CI](https://github.com/qqzhangyanhua/mabiao/actions/workflows/ci.yml/badge.svg)](https://github.com/qqzhangyanhua/mabiao/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/qqzhangyanhua/mabiao)](https://github.com/qqzhangyanhua/mabiao/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

<p align="center">
  <img src="docs/screenshots/app-preview.png" alt="Mabiao overview: local tokens, sessions, and official quota" width="920" />
</p>

| Work timeline | Conversation events | Menubar quota |
|:---:|:---:|:---:|
| <img src="docs/screenshots/timeline-preview.png" alt="Daily work timeline" /> | <img src="docs/screenshots/events-preview.png" alt="Conversation event stream" /> | <img src="docs/screenshots/tray-preview.png" alt="Menubar official quota" /> |

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
- **Work timeline**: lay sessions out as daily segments — duration, turns, and how much you ran in parallel
- **Conversation records**: read local bodies and the event stream on demand (prompt / thought / tool / reply); export Markdown / JSON
- **Quotas stay separate**: local 5-hour / 7-day estimate windows (not official caps) vs **official quota**. Built-in accounts include Claude / Codex / Cursor / Grok / Droid / Copilot and more; Settings also accepts custom providers. The menubar shows today's spend and the tightest official percentage
- **Global instructions**: lists the user-written instructions each source actually loads, kept out of token KPIs
- **Cursor code volume**: editor AI line counts are a separate dimension, not mixed into token KPIs
- **Local cache**: sqlite backup / restore / per-source rebuild; records stay archived (and counted) after source files rotate away
- **Budget alerts**: optional monthly budget (USD); one system notification per threshold for local estimates or official quota

## Data and privacy

- Default scan is read-only. **Local usage records are not uploaded**
- Aggregates live in local sqlite; backup, restore, or rebuild from Settings
- Cursor account usage and some official quotas need credentials you already have locally; session bodies are not rewritten
- Cursor session tokens go to the macOS Keychain (`keyring` is `apple-native` only). That path may be unavailable in Windows / Linux builds
- Editing global instructions writes the user's own instruction files through a whitelist; see [`docs/adr/0010-writing-user-owned-files.md`](docs/adr/0010-writing-user-owned-files.md)

## Install

GitHub Actions attaches installers to [Releases](https://github.com/qqzhangyanhua/mabiao/releases). The current public release is **[v0.1.2](https://github.com/qqzhangyanhua/mabiao/releases/tag/v0.1.2)**.

| Platform | Artifact |
|------|------|
| macOS Apple Silicon | `.dmg` (`aarch64-apple-darwin`) |
| macOS Intel | `.dmg` (`x86_64-apple-darwin`) |
| Linux x64 | `.deb` / AppImage / `.rpm` |
| Windows x64 | NSIS `.exe` / MSI |

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
