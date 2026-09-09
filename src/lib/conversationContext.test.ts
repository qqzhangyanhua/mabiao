import { describe, expect, it } from "vitest";
import type { ConversationContextManifest } from "../types";
import {
  CONTEXT_LAYER_HINT,
  CONTEXT_LAYER_TITLE,
  charsToTokens,
  contextForbiddenCopy,
  contextItemCharText,
  contextItemMetaText,
  contextKindLabel,
  contextLayerItems,
  contextLayerNote,
  contextManifestSummary,
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
});
