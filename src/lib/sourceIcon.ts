export type SourceIconId =
  | "claude"
  | "codex"
  | "grok"
  | "gemini"
  | "kimi"
  | "qwen"
  | "copilot"
  | "opencode"
  | "factory"
  | "pi"
  | "omp"
  | "dsh"
  | "cursor"
  | "hermes"
  | "agy"
  | "qoder"
  | "cline"
  | "workbuddy"
  | "zcode"
  | "alma"
  | "unknown";

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
  antigravity: "agy",
  agy: "agy",
  pi: "pi",
  omp: "omp",
  dsh: "dsh",
  cursor: "cursor",
  cursor_agent: "cursor",
  hermes: "hermes",
  qoder: "qoder",
  qoder_cn: "qoder",
  cline: "cline",
  workbuddy: "workbuddy",
  zcode: "zcode",
  alma: "alma",
};

export function resolveSourceIconId(source: string): SourceIconId {
  return SOURCE_ICON_BY_ID[source] ?? "unknown";
}
