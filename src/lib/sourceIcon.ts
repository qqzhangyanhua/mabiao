import type { SourceIconId } from "./type";

const SOURCE_ICON_BY_ID: Record<string, SourceIconId> = {
  claude: "claude",
  codex: "codex",
  grok: "grok",
  gemini: "gemini",
  kimi: "kimi",
  qwen: "qwen",
  copilot: "copilot",
  opencode: "opencode",
  factory: "factory",
  droid: "factory",
  antigravity: "gemini",
  pi: "pi",
  omp: "omp",
  dsh: "dsh",
  cursor: "cursor",
  cursor_agent: "cursor",
  hermes: "hermes",
};

export function resolveSourceIconId(source: string): SourceIconId {
  return SOURCE_ICON_BY_ID[source] ?? "unknown";
}
