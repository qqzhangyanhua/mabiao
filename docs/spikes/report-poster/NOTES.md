# 报告海报截图 spike（#150）

结论：**GO**。`modern-screenshot` 的 `foreignObject` 路线在 macOS WebKit 与 Chromium 上可以把受限 CSS 海报截成与屏上一致的 PNG。中文没有回落到默认字体，字重、字距、布局可用。外圆角落在 PNG 透明通道里（四角 alpha=0），内圆角与背景色 `#070b16` 与屏上一致。

详细检查项与复跑方式写在 [#150 评论](https://github.com/qqzhangyanhua/mabiao/issues/150)。架构约束见 [`docs/adr/0015-report-and-insights.md`](../../adr/0015-report-and-insights.md)。

## 复跑

```bash
pnpm dev
# 另开终端；需要已安装 Playwright 的 Python
python3 scripts/capture-report-spike.py webkit chromium
```

浏览器打开 `http://localhost:1420/report-spike.html`，点「截图」对比左右两栏。此页不进入产品导航，也不进生产构建。
