import type {
  ConversationContextItem,
  ConversationContextKind,
  ConversationContextLayer,
  ConversationContextManifest,
} from "../types";

export const CONTEXT_LAYER_TITLE: Record<ConversationContextLayer, string> = {
  observed: "会话内已观测",
  on_disk_possible: "可能生效 / 磁盘存在",
};

export const CONTEXT_LAYER_HINT: Record<ConversationContextLayer, string> = {
  observed: "只来自本会话 transcript / 事件索引，不是注入证明。",
  on_disk_possible: "磁盘存在，可能生效，不是本轮一定进了上下文。",
};

const KIND_LABELS: Record<ConversationContextKind, string> = {
  tool: "工具",
  system_status: "系统状态",
  error: "错误",
  skill: "Skill",
  instruction: "指令",
  rule: "规则",
  mcp_server: "MCP",
};

const SCOPE_LABELS: Record<string, string> = {
  user: "用户级",
  project: "项目级",
  grok: "Grok",
  "grok-project": "Grok 项目级",
  claude: "Claude",
  cursor: "Cursor",
  "cursor-project": "Cursor 项目级",
  mcp_json: ".mcp.json",
  config: "配置 paths",
};

export function contextKindLabel(kind: ConversationContextKind): string {
  return KIND_LABELS[kind];
}

export function contextLayerItems(
  manifest: ConversationContextManifest,
  layer: ConversationContextLayer,
): ConversationContextItem[] {
  return manifest.items.filter((item) => item.layer === layer);
}

export function contextManifestSummary(manifest: ConversationContextManifest): string {
  const observed = contextLayerItems(manifest, "observed").length;
  const possible = contextLayerItems(manifest, "on_disk_possible").length;
  return `已观测 ${observed} · 可能生效 ${possible}`;
}

export function contextItemMetaText(item: ConversationContextItem): string | null {
  const meta = item.meta;
  if (!meta) {
    return null;
  }
  const parts: string[] = [];
  const callCount = readFiniteNumber(meta.call_count);
  if (callCount !== null) {
    parts.push(`${callCount} 次`);
  }
  const byteSize = readFiniteNumber(meta.byte_size);
  if (byteSize !== null) {
    parts.push(`${byteSize} B`);
  }
  const modifiedAt = readString(meta.modified_at);
  if (modifiedAt) {
    parts.push(modifiedAt);
  }
  const scope = readString(meta.config_scope);
  if (scope) {
    parts.push(SCOPE_LABELS[scope] ?? scope);
  }
  if (meta.disabled === true) {
    parts.push("已禁用");
  }
  return parts.length > 0 ? parts.join(" · ") : null;
}

function readFiniteNumber(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function readString(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value : null;
}

export function contextForbiddenCopy(text: string): boolean {
  return text.includes("已注入");
}
