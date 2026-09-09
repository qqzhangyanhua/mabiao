import type {
  ConversationContextInjectionStatus,
  ConversationContextItem,
  ConversationContextKind,
  ConversationContextLayer,
  ConversationContextLoadMode,
  ConversationContextManifest,
} from "../types";
import { formatTokens } from "./format";

/** 与 `work_notes::estimate::CHARS_PER_TOKEN` 同口径。 */
export const CHARS_PER_TOKEN = 4;

export const CONTEXT_LAYER_TITLE: Record<ConversationContextLayer, string> = {
  injected: "已注入",
  observed: "会话内已观测",
  on_disk_possible: "可能生效 / 磁盘存在",
};

export const CONTEXT_LAYER_HINT: Record<ConversationContextLayer, string> = {
  injected: "来自本会话首轮注入快照，不是磁盘扫描。",
  observed: "只来自本会话 transcript / 事件索引，不是注入证明。",
  on_disk_possible: "磁盘存在，可能生效，不是本轮一定进了上下文。",
};

export const CONTEXT_LAYER_BADGE: Record<ConversationContextLayer, string> = {
  injected: "已注入",
  observed: "已观测",
  on_disk_possible: "可能生效 / 磁盘存在",
};

export const CHANGED_AFTER_SESSION_NOTE = "会话后已改动，当时内容可能不同";


const KIND_LABELS: Record<ConversationContextKind, string> = {
  tool: "工具",
  system_status: "系统状态",
  error: "错误",
  skill: "Skill",
  instruction: "指令",
  rule: "规则",
  mcp_server: "MCP",
};

const INJECTION_STATUS_LABELS: Record<ConversationContextInjectionStatus, string> = {
  connected: "已连上",
  failed: "连接失败",
  auth_required: "需鉴权",
  disabled: "已禁用",
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
  editor_builtin: "编辑器内置",
};

const LOAD_MODE_LABELS: Record<ConversationContextLoadMode, string> = {
  always: "常驻",
  on_match: "路径命中",
  on_demand: "按需",
  manual: "手动",
  observed: "已观测",
};

export function charsToTokens(chars: number): number {
  return Math.ceil(chars / CHARS_PER_TOKEN);
}

export function contextKindLabel(kind: ConversationContextKind): string {
  return KIND_LABELS[kind];
}

export function contextLayerItems(
  manifest: ConversationContextManifest,
  layer: ConversationContextLayer,
): ConversationContextItem[] {
  return manifest.items.filter((item) => item.layer === layer);
}

export function contextLayerNote(
  manifest: ConversationContextManifest,
  layer: ConversationContextLayer,
): string | null | undefined {
  if (layer === "injected") {
    return manifest.injected_note;
  }
  if (layer === "observed") {
    return manifest.observed_note;
  }
  return manifest.on_disk_note;
}

export function contextManifestSummary(manifest: ConversationContextManifest): string {
  const injected = contextLayerItems(manifest, "injected").length;
  const observed = contextLayerItems(manifest, "observed").length;
  const possible = contextLayerItems(manifest, "on_disk_possible").length;
  const unused = manifest.items.filter((item) => item.is_unused_install).length;
  const injectedLabel = contextMetricsFromCache(manifest)
    ? `已注入 ${injected}（缓存）`
    : contextHasInjectedSnapshot(manifest)
      ? `已注入 ${injected}`
      : "快照已过期";
  const base = `${injectedLabel} · 已观测 ${observed} · 可能生效 ${possible}`;
  return unused > 0 ? `${base} · 白装了 ${unused}` : base;
}

export function contextHasInjectedSnapshot(manifest: ConversationContextManifest): boolean {
  return manifest.has_injected_snapshot !== false;
}

export function contextMetricsFromCache(manifest: ConversationContextManifest): boolean {
  return manifest.metrics_from_cache === true;
}

export function contextInjectedDegraded(
  layer: ConversationContextLayer,
  manifest: ConversationContextManifest,
): boolean {
  return layer === "injected" && !contextHasInjectedSnapshot(manifest);
}

export function contextLayerTitle(
  layer: ConversationContextLayer,
  manifest: ConversationContextManifest,
): string {
  if (contextInjectedDegraded(layer, manifest)) {
    return "注入快照已过期";
  }
  if (layer === "injected" && contextMetricsFromCache(manifest)) {
    return "已注入（缓存）";
  }
  return CONTEXT_LAYER_TITLE[layer];
}

export function contextLayerBadge(
  layer: ConversationContextLayer,
  manifest: ConversationContextManifest,
): string {
  if (contextInjectedDegraded(layer, manifest)) {
    return "已过期";
  }
  if (layer === "injected" && contextMetricsFromCache(manifest)) {
    return "缓存";
  }
  return CONTEXT_LAYER_BADGE[layer];
}

export function contextLayerHint(
  layer: ConversationContextLayer,
  manifest: ConversationContextManifest,
): string {
  if (contextInjectedDegraded(layer, manifest)) {
    return "Cursor 只保留约 40 天会话存储。没有首轮注入快照，当前内容为按当前磁盘状态重建。";
  }
  if (layer === "injected" && contextMetricsFromCache(manifest)) {
    return "注入快照已不在源文件里。下列条目名与体积来自缓存的度量结果。";
  }
  return CONTEXT_LAYER_HINT[layer];
}


export function contextCompletenessNote(manifest: ConversationContextManifest): string | null {
  const note = manifest.completeness_note?.trim();
  return note ? note : null;
}

function volumeText(chars: number, estimated: boolean): string {
  const body = `约 ${formatTokens(charsToTokens(chars))} tok`;
  return estimated ? `估算${body}` : body;
}


export function contextItemMetaText(
  item: ConversationContextItem,
  options?: { volumeIsEstimate?: boolean },
): string | null {
  const meta = item.meta;
  const parts: string[] = [];
  if (item.is_noise) {
    parts.push("噪音");
  }
  if (item.layer === "on_disk_possible" && item.load_mode) {
    parts.push(LOAD_MODE_LABELS[item.load_mode]);
  }
  if (item.injection_status && item.injection_status !== "connected") {
    parts.push(INJECTION_STATUS_LABELS[item.injection_status]);
  }
  if (isDisconnectedMcp(item)) {
    parts.push("不占 token");
  }
  const callCount = meta ? readFiniteNumber(meta.call_count) : null;
  if (callCount !== null) {
    parts.push(`${callCount} 次`);
  }
  const toolCount = meta ? readFiniteNumber(meta.tool_count) : null;
  if (toolCount !== null) {
    parts.push(`${toolCount} 个工具`);
  }
  const charCount = readFiniteNumber(item.char_count);
  if (charCount !== null) {
    parts.push(volumeText(charCount, options?.volumeIsEstimate === true));
  } else {
    const byteSize = meta ? readFiniteNumber(meta.byte_size) : null;
    if (byteSize !== null) {
      parts.push(`${byteSize} B`);
    }
  }
  const errorType = meta ? readString(meta.error_type) : null;
  if (errorType && item.injection_status !== "auth_required") {
    parts.push(errorType);
  }
  const modifiedAt = meta ? readString(meta.modified_at) : null;
  if (modifiedAt) {
    parts.push(modifiedAt);
  }
  if (meta?.changed_after_session === true) {
    parts.push(CHANGED_AFTER_SESSION_NOTE);
  }
  const scope = meta ? readString(meta.config_scope) : null;
  if (scope) {
    parts.push(SCOPE_LABELS[scope] ?? scope);
  }
  if (meta?.disabled === true) {
    parts.push("已禁用");
  }
  return parts.length > 0 ? parts.join(" · ") : null;
}

export function contextItemCharText(item: ConversationContextItem): string | null {
  const charCount = readFiniteNumber(item.char_count);
  if (charCount === null) {
    return null;
  }
  return `${charCount.toLocaleString("zh-CN")} 字符`;
}

function readFiniteNumber(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function readString(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value : null;
}

export function contextForbiddenCopy(text: string, layer: ConversationContextLayer): boolean {
  if (layer === "injected") {
    return false;
  }
  return text.includes("已注入");
}

export function contextMcpInitSummary(
  manifest: ConversationContextManifest,
): string | null {
  const summary = manifest.mcp_init_summary?.trim();
  return summary ? summary : null;
}

export function isDisconnectedMcp(item: ConversationContextItem): boolean {
  return (
    item.kind === "mcp_server" &&
    item.injection_status != null &&
    item.injection_status !== "connected"
  );
}

export function isEditorBuiltin(item: ConversationContextItem): boolean {
  return (
    item.id.startsWith("editor_builtin:") ||
    readString(item.meta?.config_scope) === "editor_builtin"
  );
}

export function isUnusedInstall(item: ConversationContextItem): boolean {
  return item.is_unused_install === true;
}

export function contextItemsTokenSummary(
  items: ConversationContextItem[],
  volumeIsEstimate = false,
): string | null {
  const chars = items.reduce((sum, item) => {
    const count = readFiniteNumber(item.char_count);
    return count === null ? sum : sum + count;
  }, 0);
  if (chars <= 0) {
    return null;
  }
  return volumeText(chars, volumeIsEstimate);
}

export function partitionContextItems(items: ConversationContextItem[]): {
  primary: ConversationContextItem[];
  unusedInstalls: ConversationContextItem[];
  editorBuiltin: ConversationContextItem[];
  disconnectedMcp: ConversationContextItem[];
} {
  const disconnectedMcp = items.filter(isDisconnectedMcp);
  const rest = items.filter((item) => !isDisconnectedMcp(item));
  const editorBuiltin = rest.filter(isEditorBuiltin);
  const unusedInstalls = rest.filter(
    (item) => isUnusedInstall(item) && !isEditorBuiltin(item),
  );
  const primary = rest.filter((item) => !isEditorBuiltin(item) && !isUnusedInstall(item));
  return { primary, unusedInstalls, editorBuiltin, disconnectedMcp };
}
