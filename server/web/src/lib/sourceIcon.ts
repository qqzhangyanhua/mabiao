/** 来源标 id 映射。放在本包内，不引用桌面端 src/。 */
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
  | "codebuddy"
  | "trae"
  | "goose"
  | "roo"
  | "kilo"
  | "amp"
  | "zed"
  | "cherry"
  | "openclaw"
  | "crush"
  | "aider"
  | "warp"
  | "windsurf"
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
  codebuddy: "codebuddy",
  codebuddy_ide: "codebuddy",
  trae: "trae",
  goose: "goose",
  roo: "roo",
  kilo: "kilo",
  amp: "amp",
  zed: "zed",
  cherry: "cherry",
  openclaw: "openclaw",
  crush: "crush",
  aider: "aider",
  warp: "warp",
  windsurf: "windsurf",
};

export function resolveSourceIconId(source: string): SourceIconId {
  return SOURCE_ICON_BY_ID[source] ?? "unknown";
}

export function listedSourceIconSlugs(): ReadonlySet<string> {
  return new Set(Object.keys(SOURCE_ICON_BY_ID));
}
