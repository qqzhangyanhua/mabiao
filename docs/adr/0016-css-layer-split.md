# 前端样式按分层 + 按域拆分

`src/styles.css` 曾经是全部 webview 样式的唯一落点。改一处间距要在几千行里搜类名，层叠靠书写顺序，Cloud Agent 的上下文也被整文件拖垮。痛点是定位，不是命名冲突：类名已带域前缀，全库 `!important` 极少，没有特异性军备竞赛。

**决定**：保持全局类名 + BEM 风格 + CSS 自定义属性主题，做物理拆分。不引入 CSS Modules、Tailwind、CSS-in-JS，也不引入 stylelint。

## 目录

五层，按域切文件，入口 `src/styles.css` 只含 `@import` 与注释：

- 基础层 `styles/base/`：主题 token、reset
- 布局层 `styles/layout/`：应用外壳、尺寸断点
- 原子件层 `styles/ui/`：对应 `components/ui/`
- 共享层 `styles/shared/`：表格、色调、加载/空态
- 视图层 `styles/views/`：按界面域一文件（必要时前后两段）

不采用「一个组件一个样式文件」。跨组件复用的规则（面板、表格、来源色调）按域前缀归位，避免再长出一个谁都不敢碰的公共文件。

`prefers-reduced-motion` 跟随各自的动画定义，不集中。尺寸断点集中在 `layout/responsive.css`，聚合顺序排在最后。

## 门禁

单文件不超过 400 行；入口不得夹带规则。校验是 `src/lib/cssStructure.ts` 的纯函数，由既有 Vitest seam 读真实文件断言，不新增 lint 挂点。

## 后果

- 改对话记录样式打开对应域文件即可，不必搜整库。
- 入口只聚合，单文件重新胀到几千行会被测试拦住。
- webview **主壳**仍只在 `main.tsx` 引入 `styles.css` 这一处入口。周报 / 额度卡海报走 ADR 0019 的独立 CSS（`src/report/*.css`，由 `posterStyleRegistry` 引用），**不受**本 ADR 400 行门禁约束，但仍有 `posterCss.test.ts` 隔离校验。
