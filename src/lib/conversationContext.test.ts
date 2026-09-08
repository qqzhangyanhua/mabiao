import { describe, expect, it } from "vitest";
import type { ConversationContextManifest } from "../types";
import {
  CONTEXT_LAYER_HINT,
  CONTEXT_LAYER_TITLE,
  contextForbiddenCopy,
  contextItemMetaText,
  contextKindLabel,
  contextLayerItems,
  contextManifestSummary,
} from "./conversationContext";

const manifest: ConversationContextManifest = {
  items: [
    {
      layer: "observed",
      kind: "tool",
      id: "Read",
      label: "Read",
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
  observed_note: null,
  on_disk_note: "下列文件与 MCP server 名来自磁盘扫描，可能生效 / 磁盘存在，不是本轮一定进了上下文。",
};

describe("conversationContext", () => {
  it("keeps the two evidence layers visually distinct in copy", () => {
    expect(CONTEXT_LAYER_TITLE.observed).toBe("会话内已观测");
    expect(CONTEXT_LAYER_TITLE.on_disk_possible).toBe("项目上可能生效");
    expect(CONTEXT_LAYER_HINT.on_disk_possible).toContain("可能生效");
    expect(CONTEXT_LAYER_HINT.on_disk_possible).not.toContain("已注入");
    expect(contextForbiddenCopy(CONTEXT_LAYER_TITLE.observed)).toBe(false);
    expect(contextForbiddenCopy(CONTEXT_LAYER_HINT.on_disk_possible)).toBe(false);
    expect(contextForbiddenCopy(manifest.on_disk_note ?? "")).toBe(false);
  });

  it("summarizes observed vs possible counts", () => {
    expect(contextManifestSummary(manifest)).toBe("已观测 1 · 可能生效 1");
    expect(contextLayerItems(manifest, "observed")).toHaveLength(1);
    expect(contextLayerItems(manifest, "on_disk_possible")[0]?.kind).toBe("instruction");
  });

  it("formats tool counts and disk metadata without claiming injection", () => {
    expect(contextKindLabel("tool")).toBe("工具");
    expect(contextKindLabel("mcp_server")).toBe("MCP");
    expect(contextItemMetaText(manifest.items[0])).toBe("2 次");
    expect(contextItemMetaText(manifest.items[1])).toBe("12 B");
  });
});
