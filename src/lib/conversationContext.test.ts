import { describe, expect, it } from "vitest";
import type { ConversationContextFirstUse, ConversationContextManifest } from "../types";
import { formatClock } from "./format";
import {
  CHANGED_AFTER_SESSION_NOTE,
  CONTEXT_LAYER_HINT,
  CONTEXT_LAYER_TITLE,
  charsToTokens,
  contextCompletenessNote,
  contextForbiddenCopy,
  contextHasInjectedSnapshot,
  contextInjectedDegraded,
  contextItemCharText,
  contextItemExpandable,
  contextItemKey,
  contextItemMetaText,
  contextItemsTokenSummary,
  contextKindLabel,
  contextLayerBadge,
  contextLayerHint,
  contextLayerItems,
  contextLayerNote,
  contextLayerTitle,
  contextManifestSummary,
  contextMcpInitSummary,
  contextMetricsFromCache,
  firstUseItemKey,
  firstUseMarkerText,
  isDisconnectedMcp,
  partitionContextItems,
} from "./conversationContext";

const manifest: ConversationContextManifest = {
  items: [
    {
      layer: "injected",
      kind: "instruction",
      id: "/workspace/proj/AGENTS.md",
      label: "AGENTS.md",
      path: "/workspace/proj/AGENTS.md",
      load_mode: "always",
      char_count: 12,
    },
    {
      layer: "observed",
      kind: "tool",
      id: "Read",
      label: "Read",
      load_mode: "observed",
      meta: { call_count: 2 },
    },
    {
      layer: "on_disk_possible",
      kind: "instruction",
      id: "AGENTS.md",
      label: "AGENTS.md",
      path: "/tmp/proj/AGENTS.md",
      meta: { byte_size: 12, display_path: "AGENTS.md" },
    },
  ],
  injected_note: "下列指令已注入本会话首轮上下文。",
  observed_note: null,
  on_disk_note: "下列文件与 MCP server 名来自磁盘扫描，可能生效 / 磁盘存在，不是本轮一定进了上下文。",
};

describe("conversationContext", () => {
  it("keeps the three evidence layers visually distinct in copy", () => {
    expect(CONTEXT_LAYER_TITLE.injected).toBe("已注入");
    expect(CONTEXT_LAYER_TITLE.observed).toBe("会话内已观测");
    expect(CONTEXT_LAYER_TITLE.on_disk_possible).toBe("可能生效 / 磁盘存在");
    expect(CONTEXT_LAYER_TITLE.on_disk_possible).not.toContain("项目上");
    expect(CONTEXT_LAYER_HINT.on_disk_possible).toContain("可能生效");
    expect(CONTEXT_LAYER_HINT.on_disk_possible).not.toContain("已注入");
    expect(contextForbiddenCopy(CONTEXT_LAYER_TITLE.injected, "injected")).toBe(false);
    expect(contextForbiddenCopy(CONTEXT_LAYER_TITLE.observed, "observed")).toBe(false);
    expect(contextForbiddenCopy(CONTEXT_LAYER_HINT.on_disk_possible, "on_disk_possible")).toBe(
      false,
    );
    expect(contextForbiddenCopy(manifest.injected_note ?? "", "injected")).toBe(false);
    expect(contextForbiddenCopy(manifest.on_disk_note ?? "", "on_disk_possible")).toBe(false);
    expect(CONTEXT_LAYER_TITLE.injected).toContain("已注入");
    expect(manifest.injected_note).toContain("已注入");
    expect(manifest.on_disk_note).not.toContain("已注入");
  });

  it("summarizes injected vs observed vs possible counts", () => {
    expect(contextManifestSummary(manifest)).toBe("已注入 1 · 已观测 1 · 可能生效 1");
    expect(contextLayerItems(manifest, "injected")[0]?.load_mode).toBe("always");
    expect(contextLayerItems(manifest, "observed")).toHaveLength(1);
    expect(contextLayerItems(manifest, "on_disk_possible")[0]?.kind).toBe("instruction");
    expect(contextLayerNote(manifest, "injected")).toContain("已注入");
    expect(contextLayerNote(manifest, "on_disk_possible")).not.toContain("已注入");
  });

  it("formats injected volume as estimated tokens with measured characters", () => {
    expect(charsToTokens(12)).toBe(3);
    expect(contextItemMetaText(manifest.items[0])).toBe("约 3 tok");
    expect(contextItemCharText(manifest.items[0])).toBe("12 字符");
  });

  it("formats tool counts and disk metadata without claiming injection", () => {
    expect(contextKindLabel("tool")).toBe("工具");
    expect(contextKindLabel("mcp_server")).toBe("MCP");
    expect(contextItemMetaText(manifest.items[1])).toBe("2 次");
    expect(contextItemMetaText(manifest.items[2])).toBe("12 B");
    expect(contextItemCharText(manifest.items[2])).toBeNull();
    expect(
      contextItemMetaText({
        layer: "on_disk_possible",
        kind: "mcp_server",
        id: "claude:docs",
        label: "docs",
        meta: { byte_size: 8, config_scope: "claude" },
      }),
    ).toBe("8 B · Claude");
    expect(
      contextItemMetaText({
        layer: "on_disk_possible",
        kind: "skill",
        id: "user:secret",
        label: "secret",
        meta: { byte_size: 4, config_scope: "user", disabled: true },
      }),
    ).toBe("4 B · 用户级 · 已禁用");
  });

  it("keeps Grok-style honest empty disk notes free of injection claims", () => {
    const grokEmpty =
      "未发现 Grok 会加载的指令、skills 或 MCP 配置。不扫描项目根 AGENTS.md 或 .cursor/rules。";
    expect(contextForbiddenCopy(grokEmpty, "on_disk_possible")).toBe(false);
    expect(grokEmpty).toContain("未发现");
    expect(grokEmpty).not.toContain("未扫描");
    expect(grokEmpty).not.toContain("已注入");
  });

  it("formats MCP injection status, noise, and init summary", () => {
    expect(
      contextMcpInitSummary({
        items: [],
        mcp_init_summary: "配置 4 台 / 连上 2 台 / 失败 1 台 / 共注入 3 个工具",
      }),
    ).toBe("配置 4 台 / 连上 2 台 / 失败 1 台 / 共注入 3 个工具");
    expect(contextMcpInitSummary({ items: [] })).toBeNull();
    expect(
      contextItemMetaText({
        layer: "injected",
        kind: "mcp_server",
        id: "idle",
        label: "idle",
        injection_status: "connected",
        char_count: 8,
        is_noise: true,
        meta: { tool_count: 1 },
      }),
    ).toBe("噪音 · 1 个工具 · 约 2 tok");
    const failed = {
      layer: "injected" as const,
      kind: "mcp_server" as const,
      id: "figma",
      label: "figma",
      injection_status: "auth_required" as const,
      is_noise: false,
      meta: { error_type: "auth_required" },
    };
    expect(isDisconnectedMcp(failed)).toBe(true);
    expect(contextItemMetaText(failed)).toBe("需鉴权 · 不占 token");
    expect(contextItemCharText(failed)).toBeNull();
  });

  it("partitions unused installs and editor builtin skills", () => {
    const unused = {
      layer: "on_disk_possible" as const,
      kind: "skill" as const,
      id: "user:deploy",
      label: "deploy",
      load_mode: "on_demand" as const,
      char_count: 12,
      is_unused_install: true,
      meta: { config_scope: "user" },
    };
    const glob = {
      layer: "on_disk_possible" as const,
      kind: "rule" as const,
      id: ".cursor/rules/glob.mdc",
      label: "glob.mdc",
      load_mode: "on_match" as const,
      char_count: 8,
    };
    const builtin = {
      layer: "on_disk_possible" as const,
      kind: "skill" as const,
      id: "editor_builtin:env-setup",
      label: "env-setup",
      load_mode: "always" as const,
      char_count: 20,
      meta: { config_scope: "editor_builtin" },
    };
    const groups = partitionContextItems([unused, glob, builtin]);
    expect(groups.primary.map((item) => item.id)).toEqual([".cursor/rules/glob.mdc"]);
    expect(groups.unusedInstalls.map((item) => item.id)).toEqual(["user:deploy"]);
    expect(groups.editorBuiltin.map((item) => item.id)).toEqual([
      "editor_builtin:env-setup",
    ]);
    expect(contextItemMetaText(glob)).toBe("路径命中 · 约 2 tok");
    expect(contextItemMetaText(builtin)).toBe("常驻 · 约 5 tok · 编辑器内置");
    expect(contextItemsTokenSummary(groups.editorBuiltin)).toBe("约 5 tok");
    expect(
      contextManifestSummary({
        items: [unused, glob, builtin],
      }),
    ).toBe("已注入 0 · 已观测 0 · 可能生效 2");
  });

  it("lets injected instruction files expand and keeps unused installs out of the summary", () => {
    expect(contextItemExpandable(manifest.items[0])).toBe(true);
    expect(contextItemExpandable(manifest.items[1])).toBe(false);
    expect(
      contextItemExpandable({
        layer: "injected",
        kind: "instruction",
        id: "unrecognized",
        label: "未识别 12 字符",
      }),
    ).toBe(false);
    expect(
      contextItemExpandable({
        layer: "injected",
        kind: "mcp_server",
        id: "docs",
        label: "docs",
      }),
    ).toBe(false);
    expect(contextManifestSummary(manifest)).not.toContain("白装了");
  });

  it("distinguishes missing Cursor snapshot copy from injected snapshot copy", () => {
    const degraded: ConversationContextManifest = {
      items: [],
      has_injected_snapshot: false,
      injected_note: "注入快照已过期（Cursor 只保留约 40 天），以下为按当前磁盘状态重建",
      volume_is_estimate: true,
    };
    expect(contextHasInjectedSnapshot(manifest)).toBe(true);
    expect(contextHasInjectedSnapshot(degraded)).toBe(false);
    expect(contextLayerTitle("injected", degraded)).toBe("注入快照已过期");
    expect(contextLayerBadge("injected", degraded)).toBe("已过期");
    expect(contextLayerHint("injected", degraded)).toContain("40 天");
    expect(contextLayerHint("injected", degraded)).toContain("重建");
    expect(contextLayerTitle("injected", manifest)).toBe(CONTEXT_LAYER_TITLE.injected);
    expect(contextLayerHint("injected", manifest)).toBe(CONTEXT_LAYER_HINT.injected);
    expect(contextManifestSummary(degraded)).toContain("快照已过期");

    expect(contextForbiddenCopy(degraded.injected_note ?? "", "injected")).toBe(false);
  });

  it("marks cached metrics as injected-from-cache, not degraded rebuild", () => {
    const cached: ConversationContextManifest = {
      items: [
        {
          layer: "injected",
          kind: "skill",
          id: "review",
          label: "review",
          char_count: 40,
        },
      ],
      has_injected_snapshot: true,
      metrics_from_cache: true,
      injected_note: "注入快照已清理，下列度量结果来自缓存。",
      volume_is_estimate: true,
    };
    expect(contextHasInjectedSnapshot(cached)).toBe(true);
    expect(contextMetricsFromCache(cached)).toBe(true);
    expect(contextInjectedDegraded("injected", cached)).toBe(false);
    expect(contextLayerTitle("injected", cached)).toBe("已注入（缓存）");
    expect(contextLayerBadge("injected", cached)).toBe("缓存");
    expect(contextLayerHint("injected", cached)).toContain("缓存");
    expect(contextLayerHint("injected", cached)).not.toContain("重建");
    expect(contextManifestSummary(cached)).toContain("已注入 1（缓存）");
    expect(contextForbiddenCopy(cached.injected_note ?? "", "injected")).toBe(false);
  });

  it("labels Cursor volume as an estimate and names stale disk files", () => {
    expect(contextItemMetaText(manifest.items[0])).toBe("约 3 tok");
    expect(contextItemMetaText(manifest.items[0], { volumeIsEstimate: true })).toBe(
      "估算约 3 tok",
    );
    expect(contextItemsTokenSummary(manifest.items, true)).toBe("估算约 3 tok");
    expect(
      contextItemMetaText({
        layer: "on_disk_possible",
        kind: "instruction",
        id: "AGENTS.md",
        label: "AGENTS.md",
        meta: {
          byte_size: 12,
          modified_at: "2026-09-09T12:00:00Z",
          changed_after_session: true,
        },
      }),
    ).toBe(`12 B · ${formatClock("2026-09-09T12:00:00Z")} · ${CHANGED_AFTER_SESSION_NOTE}`);
    expect(
      contextItemMetaText({
        layer: "on_disk_possible",
        kind: "instruction",
        id: "SKILL.md",
        label: "SKILL.md",
        meta: { modified_at: "2026-05-23T15:25:08.812559851+00:00" },
      }),
    ).toBe(formatClock("2026-05-23T15:25:08.812559851+00:00"));
  });

  it("surfaces completeness note only when present", () => {
    expect(contextCompletenessNote(manifest)).toBeNull();
    expect(
      contextCompletenessNote({
        items: [],
        completeness_note: "上下文采集不完整：gitRepos、mcp",
      }),
    ).toBe("上下文采集不完整：gitRepos、mcp");
  });

  it("formats first-use markers and jump keys without dumping the first-round list", () => {
    const marker: ConversationContextFirstUse = {
      event_id: "evt-skill",
      sequence: 4,
      item_id: "/tmp/home/.cursor/skills/review/SKILL.md",
      item_kind: "skill",
      item_layer: "injected",
      label: "review",
    };
    expect(firstUseMarkerText(marker)).toBe("本轮引入 review");
    expect(firstUseItemKey(marker)).toBe(
      contextItemKey("injected", "skill", "/tmp/home/.cursor/skills/review/SKILL.md"),
    );
    expect(firstUseMarkerText(marker)).not.toContain("AGENTS.md");
  });
});
