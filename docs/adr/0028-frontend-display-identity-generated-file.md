# 前端展示身份与品牌图形由 Rust 穷举登记，经测试双端生成静态文件

来源显示名和来源 slug 联合类型原先手写在前端（`src/lib/format.ts` 的显示名表、`src/types.ts` 的 `UsageSource`）。它们和 Rust 的权威名单（`Source::ALL`、`Source::application_name`、`OfficialQuotaProvider::ALL`、`OfficialQuotaProvider::display_name`）之间没有测试绑定。漏改不会编译失败，也不会测试失败，界面上会悄悄退化成原始 slug。

后来远程服务管理网页 `server/web` 未纳入该生成缝，导致连续新增十余个来源后管理网页仍退化成原始 slug、通用图标和兜底配色，只能手工复制数百行显示名、品牌 SVG、位图资源、图表色表，以及一条跨目录读桌面端生成文件的 `node:fs` 正则测试。同时桌面端仍有 23 个与中性 `tone-other` 完全相同的空壳品牌 CSS 别名选择器，以及手写的无正文对话来源名单。

显示名、图标与图表色查询都是同步纯函数，调用点在组件和 node 环境下的测试里。项目规则不允许为了下发静态表引入全局状态库或 pnpm workspace，且 `server/Dockerfile` 独立拷贝并构建 `server/web`。

**决定**：前端展示身份、无正文对话来源元数据、共享品牌图形（含位图 data URI）与确定性品牌图表色由 Rust 穷举登记，经测试同时生成提交进仓库的静态 TypeScript 文件到桌面端 `src/generated/` 与远程服务管理网页 `server/web/src/generated/`。桌面端品牌徽章 CSS 配色 class 仍留在前端样式，且仅保留有独立色板的品牌，其余品牌直接回落到中性 `tone-other`。

## 生成文件

每个目标目录生成两份逐字相同的静态文件（保持每份远低于管理网页 400 行有效代码的 ESLint `max-lines` 门禁），全部数据导出为只读字面量或只读常量：

1. `src/generated/displayIdentity.ts` 与 `server/web/src/generated/displayIdentity.ts`：
   - 全部来源 slug（`SOURCE_SLUGS`，顺序与 `Source::ALL` 一致）
   - 全部官方额度提供方 slug（`OFFICIAL_QUOTA_PROVIDER_SLUGS`，顺序与 `OfficialQuotaProvider::ALL` 一致）
   - 全部品牌 ID（`BRAND_IDS`，顺序与 `Brand::ALL` 一致）
   - 本机不存对话正文的来源 slug（`BODYLESS_CONVERSATION_SOURCES`，由 `CONVERSATION_ADAPTERS::has_conversation_body` 推导，顺序与 `Source::ALL` 一致）
   - 按 slug 索引的显示名（`DISPLAY_NAMES`）
   - 按 slug 索引的品牌 ID（`BRAND_BY_SLUG`，没有品牌时为 `null`）
   - 按来源 slug 索引的恢复命令模板（`RESUME_TEMPLATES`）
   - 按品牌 ID 索引的确定性图表色（`BRAND_CHART_COLORS`，`#rrggbb`）与未知/无品牌默认色（`DEFAULT_BRAND_CHART_COLOR`）
   - 推导类型 `UsageSource` 与 `BrandId`
2. `src/generated/brandMarks.ts` 与 `server/web/src/generated/brandMarks.ts`：
   - 矢量路径与位图标记类型 `BrandSvgPath`、`BrandMark`
   - 未知/无品牌占位图形 `UNKNOWN_BRAND_MARK`
   - 按 `BrandId` 穷举索引的 `BRAND_MARKS`：矢量品牌输出 `viewBox` + 结构化路径数组，4 个位图/复合 SVG 品牌（`agy`、`workbuddy`、`zcode`、`alma`）由生成器从唯一权威目录 `src-tauri/assets/source-icons/` 读取并编码为 base64 data URI，不再在桌面端和管理网页各自存重复二进制副本

生成逻辑只在 `src-tauri/src/tests/display_identity/`（`mod.rs` 与 `marks.rs`），属于 `#[cfg(test)]`，不编进发布产物，运行时不需要。前端与 Docker 构建不依赖先跑 Rust。

该测试把 4 份生成文件都当快照：任一份缺失或与渲染结果逐字不一致就失败，失败信息给出重新生成的命令。设置 `MABIAO_UPDATE_DISPLAY_IDENTITY=1` 时原子重写全部 4 份文件。同时另配单元测试校验有效代码行数 ≤ 400、图表色均为 `#rrggbb`、且 `src-tauri/assets/source-icons/` 与 `marks.rs` 引用清单一一对应（无孤儿资源）。CI 不新增流水线步骤，已有的 `cargo test` 覆盖一致性。手改任意生成文件都会失败。

重新生成分两步。带环境变量的那次只重写文件并直接通过，不能代替快照比对。写完后必须再跑一次不带该环境变量的同一条测试，第二次通过才算生成文件和 Rust 登记一致。

```
MABIAO_UPDATE_DISPLAY_IDENTITY=1 cargo test --manifest-path src-tauri/Cargo.toml --lib tests::display_identity::display_identity_file_matches_committed_snapshot -- --exact
cargo test --manifest-path src-tauri/Cargo.toml --lib tests::display_identity::display_identity_file_matches_committed_snapshot -- --exact
```

## 重名 slug

来源和官方额度提供方可以共用一个 slug。合并规则：

- 同一个 slug 两边都有时，显示名以来源的 `application_name` 为准。因此 `claude` 是 “Claude Code”，不是额度提供方的 “Claude”；`grok` 是 “Grok CLI”；`copilot` 是 “GitHub Copilot CLI”。
- 只在额度里出现的 slug（`cursor`、`droid`、`antigravity`、`devin`）用额度提供方的 `display_name`。

按这个规则生成的显示名，与改动前前端手写表的 slug 到名字一一相同。未知 slug 仍由 `sourceLabel` 原样返回，不进生成文件。

品牌用同一条合并规则：同一个 slug 两边都有时以来源的 `brand()` 为准。只在额度里出现的 slug 用额度提供方的 `brand()`。Devin 没有品牌，生成值是 `null`。

## 品牌与视觉

品牌是一个 AI 产品。来源和官方额度提供方用穷举 `match` 登记所属品牌（`Source::brand`、`OfficialQuotaProvider::brand`）。`qoder_cn` 属于 Qoder，`cursor_agent` 属于 Cursor，`codebuddy_ide` 属于 CodeBuddy；额度的 droid 属于 factory，antigravity 属于 agy，cursor 属于 cursor。

生产 Rust 模块（`domain::Brand`）保持纯领域枚举，不含 SVG 或 CSS。品牌矢量图形、唯一位图资源清单和确定性图表色表集中在测试专用生成器 `src-tauri/src/tests/display_identity/marks.rs` 与 `src-tauri/assets/source-icons/`：

- 图形表对 `Brand` 做穷举 `match`，新增品牌未补图形时 `cargo test` 编译失败。
- 图表色为核心品牌保留识别色（`agy` 沿用 `gemini`），其余品牌按 `brand.as_str()` 的 FNV-1a 哈希确定性落入均衡色板，增删或重排枚举不会漂移。
- 桌面端徽章 CSS 配色 class（`src/lib/sourceIcon.ts`、`src/styles/shared/tones.css`）留在前端：只有具备独立色板的品牌写入 `CUSTOM_BRAND_TONES` 与 `.tone-*` 规则，其余品牌统一回落到 `tone-other`，不再为每个中性品牌写无意义的 CSS 别名选择器。

## 三种交付方式

- **启动时 IPC**：Rust command 在启动时把表发给 webview。否决。`sourceLabel` 是同步纯函数，有多处调用，其中包含 node 环境下的测试。改成 invoke 会让它们变成异步或改成接收参数，而且项目规则不允许为此引入全局状态库。webview 在 invoke 成功前也无法显示名字。
- **每个 DTO 自带字段**：每条带 slug 的 DTO 都加显示名。否决。DTO 种类太多；筛选条、旧缓存里的未知 slug、以及不经过某条 DTO 的纯函数仍然要自己查表。
- **双端静态生成文件**（采用）：编译期穷举登记，测试双写静态只读文件到桌面端与管理网页各自目录，两套前端各自同步 import、零跨包相对路径、无需 pnpm workspace，管理网页仍可被 Docker 单独构建。忘了重新生成由 `cargo test` 拦住。

## 前端怎么用

- **桌面端 (`src/`)**：
  - `sourceLabel` 读 `DISPLAY_NAMES`；`sessionResumeHint` 读 `RESUME_TEMPLATES`；`officialQuotaProviderLabel` / `OFFICIAL_QUOTA_PROVIDER_IDS` 读 `DISPLAY_NAMES` / `OFFICIAL_QUOTA_PROVIDER_SLUGS`；`src/types.ts` 从生成文件 re-export `UsageSource`。
  - `QUOTA_SOURCE_IDS` 保留手排顺序，vitest 断言它在集合上等于 `[...SOURCE_SLUGS, "cursor"]`。
  - `src/lib/conversationCatalogItems.ts` 直接使用生成的 `BODYLESS_CONVERSATION_SOURCES` 并动态拼出空目录说明文案，不再手写无正文来源数组。
  - `src/components/sourceGlyph.tsx` 直接渲染 `src/generated/brandMarks.ts` 的 `BRAND_MARKS` / `UNKNOWN_BRAND_MARK`，删除桌面端独立的 `src/assets/source-icons/`。
  - `src/lib/sourceIcon.ts` 仅维护有独立色板的 `CUSTOM_BRAND_TONES`，其余品牌统一回落 `tone-other`，删除 `src/styles/shared/tones.css` 的 23 个中性别名选择器。
- **远程服务管理网页 (`server/web/`)**：
  - `src/lib/source.ts` 读 `server/web/src/generated/displayIdentity.ts` 的 `DISPLAY_NAMES`。
  - `src/lib/sourceIcon.ts` 读 `BRAND_BY_SLUG` 与 `BrandId`。
  - `src/lib/chart.ts` 经 `BRAND_BY_SLUG` + `BRAND_CHART_COLORS` / `DEFAULT_BRAND_CHART_COLOR` 解析品牌图表色，删除手写 38 项 `SOURCE_COLOR` 表与不再使用的 `--tone-*` 变量。
  - `src/components/sourceGlyph.tsx` 直接渲染 `server/web/src/generated/brandMarks.ts`，删除重复的 `server/web/src/assets/source-icons/`。
  - 删除跨目录读桌面端生成文件的 `node:fs` 正则测试；一致性由 Rust `cargo test` 机械保证。

## 后果

- 新增来源或额度提供方时，`application_name` / `display_name` / `brand` 的穷举 `match` 以及 `CONVERSATION_ADAPTERS` 的 `has_conversation_body` 缺一处会编译失败；引入新品牌时 `tests/display_identity/marks.rs` 的穷举 `match` 也会编译失败。重新生成一次即可同步更新桌面端和管理网页的全部展示身份、品牌图形、无正文来源列表与图表色。
- 新品牌在桌面端默认使用中性 `tone-other`，仅当需要专属色板时才加 `CUSTOM_BRAND_TONES` 与 `tones.css` 规则。
- 纯重构不改归一化输出，因此不改 `ADAPTER_VERSION` 与 `CONVERSATION_ADAPTER_VERSION`。
- 看到有人再手写来源显示名表、按 slug 索引的图标/图表色表、手写无正文对话来源名单、跨包复制二进制图标资源、或写跨目录正则测试，就是在违反本篇。
