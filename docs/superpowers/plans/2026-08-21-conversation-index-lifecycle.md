# Conversation Index Lifecycle Implementation Plan

> **状态：已落地（历史计划）。** 对话索引生命周期已实现，见 ADR 0011 / 0014。下文路径与勾选框保留当时的实施步骤，**不要按本文件改代码**。现行约定见 `AGENTS.md`。已知过时处：`src-tauri/src/conversation.rs` 现为 `conversation/` 目录；仓库没有 `pnpm type-check`，类型检查走 `pnpm build`。

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 Codex 对话索引随既有摄取生命周期可靠更新，并让打开的详情在不重复解析无变化文件的前提下持续显示新增内容。

**Architecture:** `conversation_sessions` 保存可重建元数据、文件可用状态和 `mtime + size` 指纹；摄取仍由现有刷新与托盘心跳触发，不增加文件监听器。详情页用只读取文件元数据的轻量接口轮询修订号，仅在修订变化时重新加载正文并原子替换时间线；SQLite 和备份始终不保存正文。

**Tech Stack:** Rust、rusqlite、Tauri 2 commands、React、TypeScript、Vitest

---

### Task 1: 索引墓碑、指纹与结构化诊断

**Files:**
- Modify: `src-tauri/src/domain.rs`
- Modify: `src-tauri/src/store.rs`
- Modify: `src-tauri/src/conversation.rs`
- Modify: `src-tauri/src/ingest.rs`
- Modify: `src-tauri/src/tests/conversation.rs`
- Modify: `src/types.ts`

- [ ] **Step 1: 写删除保留、恢复和解析失败的失败测试**

  在 `src-tauri/src/tests/conversation.rs` 中把旧的“删除即移除”断言改为：文件删除并完成干净扫描后目录仍有同一 `source + session_id`，`file_available == false`，详情返回“原文件已删除”；原路径恢复后目录仍只有一条且 `file_available == true`。新增解析失败断言，检查最后正确标题不变，诊断只含 `event_type`、`line` 和原因。

- [ ] **Step 2: 运行定向测试并确认 RED**

  Run: `cargo test --manifest-path src-tauri/Cargo.toml conversation`

  Expected: FAIL，原因是 `ConversationSessionRow` 尚无 `file_available`，删除仍物理删除索引，诊断尚无结构化字段。

- [ ] **Step 3: 增加向后兼容的 SQLite 列与 DTO**

  在 `conversation_sessions` 增加：

  ```sql
  file_available INTEGER NOT NULL DEFAULT 1,
  source_file_mtime_ms INTEGER NOT NULL DEFAULT 0,
  source_file_size INTEGER NOT NULL DEFAULT 0
  ```

  `init_schema` 通过 `ensure_column` 迁移旧缓存。对外 `ConversationSessionRow` 只暴露 `file_available: bool`；文件指纹只留在 SQLite 内部。`IngestIssue` 增加可选的 `event_type` 与 `line`，其他来源写 `None`。

- [ ] **Step 4: 用指纹跳过未变解析并把删除改为墓碑**

  `refresh_codex_in_roots` 对每个路径读取元数据；当路径、mtime、size 与可用缓存一致时直接把会话加入 seen 集合。成功解析时 upsert 元数据、指纹并设置 `file_available = true`；干净扫描结束后把未见会话更新为 `file_available = false`，不删除行。任一解析失败时不执行墓碑对账，保留最后正确索引。

- [ ] **Step 5: 结构化解析错误且不复制正文**

  JSON 行错误返回 `event_type = "json_line"`、一基行号与解析原因；缺失会话元数据返回 `event_type = "session_meta"`。`ingest_all` 映射到 `conversation_issues`，不包含原始行文本。

- [ ] **Step 6: 运行定向测试并确认 GREEN**

  Run: `cargo test --manifest-path src-tauri/Cargo.toml conversation`

  Expected: PASS，覆盖删除墓碑、恢复不重复、解析失败保留和元数据搜索。

### Task 2: 详情修订探测

**Files:**
- Modify: `src-tauri/src/domain.rs`
- Modify: `src-tauri/src/conversation.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/tests/conversation.rs`
- Modify: `src/types.ts`

- [ ] **Step 1: 写追加、无变化与删除状态的失败测试**

  初次 `load_detail` 取得 `revision`；未改文件时 `detail_state(..., revision)` 返回 `changed == false`。追加一条完整 JSONL 事件后返回 `changed == true`，重新加载只得到一份完整且无重复的时间线；删除文件后返回 `file_available == false`。

- [ ] **Step 2: 运行单测并确认 RED**

  Run: `cargo test --manifest-path src-tauri/Cargo.toml conversation_detail_state`

  Expected: FAIL，原因是修订 DTO 与接口不存在。

- [ ] **Step 3: 实现可信索引驱动的修订接口**

  增加：

  ```rust
  pub struct ConversationDetailStateDto {
      pub revision: String,
      pub changed: bool,
      pub file_available: bool,
  }
  ```

  `load_detail` 返回当前 `revision`。`detail_state` 只接受 `source + session_id + known_revision`，后端从可信索引解析路径并校验扫描根目录；存在时只读取 metadata，缺失时返回不可用，不读取正文。

- [ ] **Step 4: 注册 Tauri command 并同步 TypeScript DTO**

  新命令 `get_conversation_detail_state` 使用只读连接；前端参数保持 `source`、`sessionId` 和 `knownRevision`，不提交路径。

- [ ] **Step 5: 运行单测和类型检查并确认 GREEN**

  Run: `cargo test --manifest-path src-tauri/Cargo.toml conversation_detail_state`

  Run: `pnpm type-check`

  Expected: 两条命令均 PASS。

### Task 3: 前端底部跟随与新增计数

**Files:**
- Create: `src/lib/conversationFollow.ts`
- Create: `src/lib/conversationFollow.test.ts`
- Modify: `src/components/Conversations.tsx`
- Modify: `src/styles.css`

- [ ] **Step 1: 写滚动跟随纯函数失败测试**

  覆盖三种行为：位于底部时新增内容触发跟随且未读数为零；离开底部时不滚动并累计新增数；点击“新事件”后清零并恢复跟随。

- [ ] **Step 2: 运行单文件测试并确认 RED**

  Run: `pnpm test src/lib/conversationFollow.test.ts`

  Expected: FAIL，原因是 `isNearConversationBottom` 与 `nextConversationFollowState` 尚不存在。

- [ ] **Step 3: 实现最小纯函数**

  `isNearConversationBottom` 用 `scrollHeight - scrollTop - clientHeight <= 40` 判定；`nextConversationFollowState` 根据前后事件数和 `wasAtBottom` 返回 `shouldScroll` 与 `unseenCount`，数量缩小时按重置处理。

- [ ] **Step 4: 接入详情轮询和稳定滚动容器**

  详情打开后每 2 秒调用修订接口；仅 `changed` 时重新加载详情并原子替换数组。滚动容器在底部时于下一帧滚到底；用户离开底部时保留位置并显示“新增 N 条事件”，点击后滚到底并清零。文件缺失时显示明确警告，保留已载入快照供当前页面阅读。

- [ ] **Step 5: 运行单文件测试与 TypeScript 检查并确认 GREEN**

  Run: `pnpm test src/lib/conversationFollow.test.ts`

  Run: `pnpm type-check`

  Expected: 两条命令均 PASS。

### Task 4: 完整验证、审查与提交

**Files:**
- Verify all files changed by Tasks 1-3

- [ ] **Step 1: 运行仓库规定的完整验证**

  Run: `pnpm lint`

  Run: `pnpm test`

  Run: `pnpm build`

  Run: `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`

  Run: `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`

  Run: `cargo test --manifest-path src-tauri/Cargo.toml`

  Expected: #03 新增测试全部通过；若完整 Rust 套件仍有固定 Windows 基线失败，必须在提交说明中列明并用固定点复现。

- [ ] **Step 2: 按固定点执行双轴代码审查**

  以开始 #03 前的 `e2e6a45` 为固定点，但审查输入只包含 #03 暂存补丁；Standards 轴检查 `AGENTS.md`、ADR 0003 和代码气味，Spec 轴逐条对照 `.scratch/conversation-records/issues/03-conversation-index-lifecycle.md`。

- [ ] **Step 3: 只暂存 #03 并提交当前分支**

  精确暂存计划、生命周期实现与测试，不包含工作区现有 #02 或其他并行修改。

  ```bash
  git commit -m "feat: add conversation index lifecycle"
  ```
