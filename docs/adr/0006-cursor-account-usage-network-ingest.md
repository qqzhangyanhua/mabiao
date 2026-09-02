# Cursor 账号用量走联网采集、独立维度

> 原文件名 `0004-cursor-account-usage-network-ingest.md`，2026-08-18 重编号为 0006，避免与归档 ADR 0004 撞号。

Cursor 的真实 token 用量只存在于云端账号，本机会话文件里没有。用户需要看到 Cursor 实际消耗了多少 token，但这与「本机离线扫描本地文件」的立身前提冲突。

**决定**：新增独立维度「Cursor 账号用量 (Cursor Account Usage)」。由 Rust 侧携带从本机 Cursor 客户端读到的 `WorkosCursorSessionToken`，调用 Cursor 非公开仪表盘接口 `POST /api/dashboard/get-filtered-usage-events`，把账号级事件解析后写入独立缓存表。self-serve 计划只采 token、不采费用。默认仍只在用户点「刷新」时联网；设置里可打开独立自动刷新。

这是对 ADR 0001 / 0002 / 0003 的**显式破例**，边界如下：

- **仅此一处**：其它来源仍只扫描本机文件。不得把联网采集扩散成通用摄取路径。
- **独立 opt-in**：刷新是独立按钮，不进入 `ingest_all` / 启动摄取 / 本机会话的 1/5/10 分钟定时器。离线时只读上次缓存，不阻塞其它来源。用户可在设置 Cursor 页打开独立自动刷新（默认关，固定 10 分钟，独立存储键），仍不进本机 KPI / 5 小时窗。
- **独立维度**：数据不进入 `UsageRecord`、`Source` 枚举或本机 token 聚合。处理方式对齐「代码量」。
- **凭证不落明文**：会话 token 不写 `prices.json` 或其它可读配置，也不进备份包。

**2026-08-21 修订（凭证来源）**：手动粘贴 cookie 的通路整个删掉，凭证只有一个来源——本机 Cursor 客户端的登录态。Cursor 把 WorkOS 会话 JWT 明文写在自己的 `state.vscdb`（`ItemTable` 的 `cursorAuth/accessToken`）里并自行续期，只读该文件、按 `<sub 里 | 之后那段>%3A%3A<jwt>` 拼出 cookie 值即可。

- **没有手动兜底**：不再有粘贴框、`save_cursor_session_token` 命令和钥匙串，`keyring` 依赖已移除。代价是没装 Cursor 客户端（或读不到）就用不了这个维度——接受，因为这个维度本来就是给 Cursor 用户看的。
- 只读，不写 Cursor 的任何文件。`state.vscdb` 有几百 MB 且 Cursor 常驻占用，不能复制；WAL 下只读连接不阻塞写者，直接原地只读打开。不用 `immutable=1` 降级——它跳过 WAL，会读到陈旧值（实测复制出来的库读到的仍是上一个订阅档位）。
- value 列可能是 TEXT 也可能是 BLOB，两种都要认。
- 读不到 / 解析失败 / 已过期（含 60s 容差）一律降级为可读中文提示，指向「去 Cursor 客户端登录」。
- 这仍然是**读本机文件**，没有扩大联网面，和 Grok 读 `~/.grok/auth.json` 同构。

- **缓存语义**：没有本机文件指纹。用 `时间戳+模型+各口径token+isHeadless` 做事件去重；解析失败不以残缺结果覆盖旧缓存（对齐 ADR 0003「最后一次正确结果」精神）。

**理由**：账号级数据语义（全设备、全时段、云端）与本机会话用量不是一回事，硬塞进统一聚合会污染「本机 token 总量」。接口又必须带 `Cookie` 头，webview `fetch` 做不到，所以联网落在 Rust。

## Consequences

- 接口是逆向的、非官方，Cursor 随时可改坏（2026-07-31 已追溯清零 self-serve 费用字段）。失败必须降级为可读中文提示。
- cookie 会过期，但跟着 Cursor 客户端自动续期，用户正常无感。客户端登出或换号后，下次刷新会提示去 Cursor 重新登录。自动解密浏览器 Cookies 仍不在范围内——实测 Cursor 自己的 Electron cookie jar 里根本没有 cursor.com 的 cookie，只能去解 Chrome/Edge，不值得。
- `usage.sqlite` 新增 `cursor_account_usage` / `cursor_account_meta`，可独立清空，不参与 `ADAPTER_VERSION` 对账。
- 事件表明细只读上述缓存表，分页下发，不重新联网。与本机会话没有关联键。
- 概览 7 天滚动用量可单独挂一行 Cursor 账号汇总（`source=cursor`），费用走用户价目 / LiteLLM 快照兜底；仍不进入 `UsageRecord`、本机 token KPI 或 5 小时计费窗。

**2026-09-02 修订（独立自动刷新）**：本机会话自动刷新仍不拉账号用量。账号用量若要定时联网，必须走设置 Cursor 页的独立开关，不得复用 1/5/10 分钟摄取定时器。开启后立即拉一次，之后每 10 分钟再拉；启动摄取路径仍然不碰这个维度。
