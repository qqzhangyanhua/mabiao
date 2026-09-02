# 探测结果：Hermes

实测时间：2026-09-02。本机 `~/.hermes/state.db`（WAL 模式）。只记录元数据字段位置，不摘录会话正文。

## 本机落盘路径

```text
~/.hermes/
├── state.db
├── state.db-wal
└── state.db-shm
```

用量在 SQLite，不在 `sessions/` 正文目录。扫描根可用 `HERMES_HOME` 覆盖，发现文件是 `<root>/state.db`。

## 选表

`session_model_usage` 按 `(session_id, model, billing_provider, billing_base_url, billing_mode, task)` 六元组存模型级累计 token。`sessions.*_tokens` 是会话合计，多模型会被并掉，适配器不用那几列。时间、项目只从 `sessions` 取 `started_at` / `cwd` / `git_repo_root`。

## 本机样本

- `sessions` 有数据；`session_model_usage` 仅两行，同属一个 `session_id`，模型名相同，靠 `billing_provider` / `task` 分开（其中一行 `task = title_generation`）。
- `started_at` 是 REAL unix 秒（含小数），例如 `1785986693.42276` → `2026-08-06 03:24:53 UTC`。
- `cost_source` 实测取值：`none`，以及 NULL。本机没有 `actual_cost_usd > 0` 的行。Hermes 源码里还会写其它来源名（测试里见过 `openrouter` / `subagent`）；适配器把「非空且不是 `none`」且 `actual_cost_usd > 0` 当作原生费用。
- `cwd` / `git_repo_root` 在本机这两行都是空。
- 主键字符串字段本机多为空串 `''`，不是 NULL。

## 只读打开

Hermes 自己用 `file:{path}?mode=ro` + `uri=True`。本适配器同样 `OPEN_READ_ONLY | OPEN_URI`，避免和 Hermes 写事务抢锁。WAL 侧车变化靠 `state.db-wal` / `state.db-shm` 指纹触发重解析。
