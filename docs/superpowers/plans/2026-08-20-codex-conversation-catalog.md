# Codex 对话记录基础闭环 Implementation Plan

> **状态：已落地（历史计划）。** 对话记录目录已实现，见 ADR 0011 / 0014。下文路径与勾选框保留当时的实施步骤，**不要按本文件改代码**。现行约定见 `AGENTS.md`。已知过时处：`src-tauri/src/conversation.rs` 现为 `conversation/` 目录。

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 新增独立“对话记录”入口，索引 Codex 会话元数据并按需读取用户与助手正文，同时保持消耗记录和 Cursor 会话口径不变。

**Architecture:** 新建深模块 `conversation`，其接口负责刷新 Codex 元数据索引、分页查询和可信路径详情读取；调用者不接触 JSONL、路径或 SQLite 细节。SQLite 只保存目录元数据，详情每次从已索引且位于允许扫描根目录内的原文件读取。前端用独立懒加载视图调用两个 Tauri command，并在目录与详情状态之间切换。

**Tech Stack:** Rust、rusqlite、serde/serde_json、Tauri 2、React 19、TypeScript、Vitest。

---

### Task 1: 建立对话目录公共接缝与失败测试

**Files:**
- Create: `src-tauri/tests/fixtures/codex-conversation.jsonl`
- Create: `src-tauri/src/tests/conversation.rs`
- Modify: `src-tauri/src/tests/mod.rs`
- Modify: `src-tauri/src/test_support/mod.rs`
- Modify: `src-tauri/src/domain.rs`
- Modify: `src-tauri/src/store.rs`

- [ ] **Step 1: 添加脱敏 Codex 会话样本**

样本必须包含 `session_meta`、两个 `turn_context`、一条用户 `response_item`、两条助手 `response_item`，并给出明确的会话 ID、项目、模型和时间。

- [ ] **Step 2: 写高层失败测试**

```rust
#[test]
fn codex_conversation_catalog_indexes_and_loads_messages_without_caching_body() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let source = seed_codex_conversation(home);
    let conn = store::open_memory().unwrap();

    conversation::refresh_codex(&conn, home).unwrap();
    let page = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.rows[0].title, "发布 Tray 客户端版本支持图片编辑透传");

    let detail = conversation::load_detail(&conn, home, "codex", "conv-1").unwrap();
    assert_eq!(detail.messages.len(), 3);
    std::fs::remove_file(source).unwrap();
    assert!(conversation::load_detail(&conn, home, "codex", "conv-1").is_err());
}
```

- [ ] **Step 3: 运行测试确认 RED**

Run: `cargo test --manifest-path src-tauri/Cargo.toml conversation`

Expected: FAIL，因为 `conversation` 模块和 DTO 尚不存在。

- [ ] **Step 4: 添加最小 DTO 与索引表**

定义 `ConversationQuery`、`ConversationSessionRow`、`ConversationPage`、`ConversationMessage`、`ConversationDetailDto`。索引表以 `(source, session_id)` 为主键，保存标题、项目、模型、起止时间、原文件定位、能力 JSON 和支持状态，不包含正文列。

- [ ] **Step 5: 运行编译确认类型骨架正确**

Run: `cargo test --manifest-path src-tauri/Cargo.toml conversation --no-run`

Expected: 仍因缺少行为实现失败，但 DTO 与 schema 本身可编译。

### Task 2: 实现 Codex 刷新、查询和可信详情读取

**Files:**
- Create: `src-tauri/src/conversation.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/ingest.rs`
- Modify: `src-tauri/src/tests/conversation.rs`

- [ ] **Step 1: 实现小接口、深实现**

```rust
pub fn refresh_codex(conn: &Connection, home: &Path) -> Result<Vec<ConversationIndexIssue>, String>;
pub fn sessions_page(conn: &Connection, query: &ConversationQuery) -> Result<ConversationPage, String>;
pub fn load_detail(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
) -> Result<ConversationDetailDto, String>;
```

刷新时只索引 Codex 会话；标题优先读取来源标题，否则截断第一条用户消息。基础详情优先读取 `response_item.payload.type=message`，只保留 `user` 和 `assistant` 文本；若文件没有这类消息，再回退 `event_msg` 的用户/助手文本。

- [ ] **Step 2: 实现元数据分页和搜索**

搜索只匹配标题、来源、项目、模型、会话 ID、开始时间和结束时间；默认按结束时间倒序，页码最小为 1，页大小限制在合理上限。

- [ ] **Step 3: 实现可信路径校验**

详情只接受来源和会话 ID。模块从索引取路径，对目标文件和当前 Codex 扫描根目录做规范化，并拒绝不在允许根目录内的路径；文件缺失或 JSONL 损坏时返回中文错误。

- [ ] **Step 4: 接入现有摄取刷新**

`ingest_all` 成功扫描来源时刷新 Codex 对话索引；单个对话索引失败记录为来源诊断，不阻断其他来源。对话索引不递增消耗记录的 `ADAPTER_VERSION`。

- [ ] **Step 5: 运行聚焦测试确认 GREEN**

Run: `cargo test --manifest-path src-tauri/Cargo.toml conversation`

Expected: 所有 conversation 测试通过。

- [ ] **Step 6: 补充搜索与路径拒绝测试**

覆盖标题/项目/模型/会话 ID/时间搜索、分页，以及把索引路径替换为扫描根目录外文件后详情必须失败。

- [ ] **Step 7: 再跑聚焦测试**

Run: `cargo test --manifest-path src-tauri/Cargo.toml conversation`

Expected: 所有新增行为测试通过。

### Task 3: 暴露 Tauri 命令并接入独立 React 视图

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/types.ts`
- Create: `src/components/Conversations.tsx`
- Modify: `src/views/lazyViews.tsx`
- Modify: `src/App.tsx`
- Modify: `src/components/Sidebar.tsx`
- Modify: `src/components/Topbar.tsx`
- Modify: `src/hooks/viewCache.ts`
- Modify: `src/hooks/viewCache.test.ts`
- Modify: `src/hooks/useKeyboardShortcuts.ts`
- Modify: `src/styles.css`

- [ ] **Step 1: 先扩展视图状态测试**

```ts
expect(parseViewHash("#conversations")).toBe("conversations");
expect(viewsInvalidatedBy("conversations")).toEqual([]);
```

- [ ] **Step 2: 运行单文件测试确认 RED**

Run: `pnpm test src/hooks/viewCache.test.ts`

Expected: FAIL，因为 `conversations` 尚未注册为 View。

- [ ] **Step 3: 添加 Tauri DTO 和命令**

新增分页查询命令和详情命令。详情命令参数只有 `source`、`sessionId`，不接受前端文件路径；两个命令都在线程池中使用只读数据库连接。

- [ ] **Step 4: 添加前端类型与懒加载视图**

目录展示标题、来源、项目、模型、起止时间、能力和实验性状态，支持搜索与分页。选中条目后切换到独立详情，展示会话元数据、返回按钮以及按时间排列的用户/助手纯文本消息。

- [ ] **Step 5: 注册独立导航**

新增 `conversations` 视图、Sidebar “对话记录”入口、标题副标题、hash 路由和快捷键序列。该视图不显示用量筛选 Topbar，也不参与用量视图缓存失效。

- [ ] **Step 6: 运行前端聚焦测试与类型检查**

Run: `pnpm test src/hooks/viewCache.test.ts`

Expected: PASS。

Run: `pnpm exec tsc --noEmit`

Expected: PASS。

### Task 4: 全量验证、双轴评审与提交

**Files:**
- Review: Ticket #01 涉及的全部变更

- [ ] **Step 1: 运行仓库要求的前端验证**

Run: `pnpm lint`

Run: `pnpm test`

Run: `pnpm build`

Expected: 全部退出码为 0。

- [ ] **Step 2: 运行仓库要求的 Rust 验证**

Run: `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`

Run: `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`

Run: `cargo test --manifest-path src-tauri/Cargo.toml`

Expected: 全部退出码为 0。

- [ ] **Step 3: 按固定基线做双轴代码评审**

以实现前的 `HEAD` 为 fixed point，并行检查仓库标准与 Ticket #01 规格；发现问题后修复并重新运行相关验证。

- [ ] **Step 4: 只暂存 Ticket #01 相关文件并提交**

```bash
git add docs/superpowers/plans/2026-08-20-codex-conversation-catalog.md src src-tauri/src src-tauri/tests/fixtures/codex-conversation.jsonl
git commit -m "feat: add codex conversation catalog"
```

提交前检查暂存区，不包含用户已有的 `src-tauri/Cargo.toml` 工作区状态。
