# 报告海报截图 spike（#150）

结论：**GO**。`modern-screenshot` 的 `foreignObject` 路线在 macOS WebKit 与 Chromium 上可以把受限 CSS 海报截成与屏上一致的 PNG。中文没有回落到默认字体，字重、字距、布局可用。外圆角落在 PNG 透明通道里（四角 alpha=0），内圆角与背景色 `#070b16` 与屏上一致。

详细检查项与复跑方式写在 [#150 评论](https://github.com/qqzhangyanhua/mabiao/issues/150)。架构约束见 [`docs/adr/0015-report-and-insights.md`](../../adr/0015-report-and-insights.md)；周报内置视觉风格见 [`docs/adr/0019-weekly-report-poster-styles.md`](../../adr/0019-weekly-report-poster-styles.md)。

## 复跑

```bash
pnpm dev
# 另开终端；需要已安装 Playwright 的 Python
python3 scripts/capture-report-spike.py webkit chromium
```

浏览器打开 `http://localhost:1420/report-spike.html`，点「截图」对比左右两栏。此页不进入产品导航，也不进生产构建。页上的风格开关只服务 spike，不写分享偏好、不改对话框。

## 多风格对照（#170）

spike 从 `REPORT_POSTER_STYLES` 渲染**全部**已注册周报风格，共用 `fakePosterData`。新增风格后不必改 spike 清单。

- 浏览器：并排看「全部已注册风格」；对每种风格切换后点「截图」，核对屏上 vs foreignObject PNG（中文、玻璃模糊、霓虹发光、透明圆角、inline 装饰、按天柱、来源条）。`?style=` 可深链到某一风格。
- **打包后的 Tauri WKWebView**：分享对话框里对每种周报风格各复制一次，粘贴核对剪贴板 PNG 与预览一致。不要只在 Chromium 里看过就当 WKWebView 过了。
- CI **不做**像素快照。

## 极端数据整图（#158）

同一页底部有稀疏组合海报，数据走 `toPosterViewModel`，当前选中风格各画一套。`?case=single-night` / `?case=single-day` 把该夹具铺成全部已注册风格，方便并排目视。

检查项：七个槽位都在、没有「暂无数据」「——」「未命名会话」或 `$0.00`、单日仍是七根柱、单一来源是一条 100%、深夜 0% / 100% 文案读得通。截图见 `extreme/`。
