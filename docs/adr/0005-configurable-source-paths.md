# 数据源扫描路径可配置

摄取路径此前直接写死在 `ingest.rs` 里（`home.join(".codex/sessions")` 之类），完全依赖各工具的默认安装位置。这在两类场景下会漏数据：

- 工具本身支持自定义数据目录（Codex 的 `CODEX_HOME`、Claude Code 的 `CLAUDE_CONFIG_DIR` 等），用户改过之后本工具扫不到；
- Claude Code 在部分安装方式（比如某些包管理器打包、非官方安装脚本）下把会话写到 XDG 目录 `~/.config/claude/projects`，而不是官方默认的 `~/.claude/projects`；写死单一路径的用户会“悄悄”丢失一部分数据且没有任何提示。

## 决定

每个 Source 的扫描根目录都可以用一个专属环境变量整体覆盖，命名尽量对齐 `ccusage` 等同类工具的既有约定（同时装了多个统计工具的用户可以复用同一份配置）：

| Source | 环境变量 | 默认路径 |
|---|---|---|
| Codex | `CODEX_HOME` | `~/.codex`（扫 `sessions/`） |
| Claude Code | `CLAUDE_CONFIG_DIR` | `~/.claude` **和** `~/.config/claude`（都扫 `projects/`） |
| Pi | `PI_AGENT_DIR` | `~/.pi/agent/sessions` |
| OMP | `OMP_AGENT_DIR` | `~/.omp/agent/sessions` |
| OpenCode | `OPENCODE_DATA_DIR` | `~/.local/share/opencode`（扫 `opencode.db`） |
| Kimi | `KIMI_DATA_DIR` | `~/.kimi`（扫 `sessions/` + `kimi.json`） |
| dsh | `DSH_HOME` | `~/.dsh`（扫 `sessions/`） |
| Gemini | `GEMINI_DATA_DIR` | `~/.gemini/tmp` |
| Grok | `GROK_HOME` | `~/.grok`（扫 `sessions/`） |
| Qwen | `QWEN_DATA_DIR` | `~/.qwen`（扫 `tmp/`） |
| Factory/droid | `FACTORY_SESSIONS_DIR` | `~/.factory/sessions` |
| cursor-agent | `CURSOR_AGENT_USAGE_DIR` | token 包装默认 `~/.cursor-agent-usage`（可选）。会话与 IDE 共用 `~/.cursor/chats`、`~/.cursor/projects`，不由此变量改 |
| GitHub Copilot CLI | `COPILOT_HOME` | `~/.copilot`（扫 `session-state/`） |
| Hermes | `HERMES_HOME` | `~/.hermes`（扫 `state.db`） |

环境变量的值可以是逗号分隔的多个绝对路径，会全部扫描并合并到同一次摄取/对账里（不是相互独立的多份缓存）。覆盖是整体替换默认值，不是追加；默认的多路径（目前只有 Claude Code 的 XDG 双路径）在显式设置对应环境变量后也不再自动附加。

路径拼接规则和默认值保持一致：环境变量给的是「根目录」，内部仍然按各 Source 原来的规则拼接子路径（例如 `CODEX_HOME=/x` 实际扫 `/x/sessions`）。

设置页的来源健康表始终展示当前实际生效的扫描路径（`root_path`，多个路径用「, 」连接），覆盖后这里会如实反映，不需要用户去猜路径拼接结果对不对。

设置页「扫描路径」可以为每个 Source 填写绝对根目录，语义与环境变量相同（整体替换、逗号分隔、再按原规则拼接叶子路径）。优先级：设置页覆盖 > 环境变量 > 默认路径。从 Dock / Finder 打开时读不到用户 shell 里 export 的值，这是漏数的真实原因；环境变量仍保留给终端启动和脚本，设置页用来给 GUI 一份绝对路径。

## Consequences

- 环境变量只在应用启动/摄取时读取一次性生效；桌面 App 从 Dock/Finder 打开时，用户 shell 里的 export 经常读不到。设置页把绝对路径写进 `scan_paths.json`，不依赖进程环境。
- 路径解析逻辑（`ingest::source_scan_dirs_with`）和覆盖表构造（`env_overrides` / `scan_paths`）分离，测试直接构造覆盖表验证拼接规则，不触碰进程级环境变量，避免并行测试互相污染。
- 多目录合并进同一次对账意味着两个目录中的任意一个文件消失都会被检测到并处理（依据 ADR 0004 归档，不物理删除）；目录之间没有优先级或去重语义，如果用户配置了实际指向同一份数据的重复路径，会重复计入统计。
