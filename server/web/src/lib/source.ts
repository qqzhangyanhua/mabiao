/** 来源 slug → 界面名。管理网页自持一份，不读桌面端生成文件。见 ADR 0028。 */
const SOURCE_LABEL: Record<string, string> = {
  claude: "Claude Code",
  codex: "Codex",
  factory: "Droid",
  droid: "Droid",
  pi: "Pi",
  omp: "OMP",
  opencode: "OpenCode",
  kimi: "Kimi CLI",
  dsh: "DeepSeek Harness",
  gemini: "Gemini CLI",
  grok: "Grok CLI",
  qwen: "Qwen Code",
  cursor: "Cursor",
  cursor_agent: "Cursor Agent",
  copilot: "GitHub Copilot CLI",
  hermes: "Hermes",
  agy: "Antigravity",
  antigravity: "Antigravity",
  qoder: "Qoder",
  qoder_cn: "Qoder CN",
  cline: "Cline",
  workbuddy: "WorkBuddy",
  zcode: "ZCode",
  alma: "Alma",
  codebuddy: "CodeBuddy",
  codebuddy_ide: "CodeBuddy IDE",
  trae: "Trae CLI",
  goose: "Goose",
  roo: "Roo Code",
  kilo: "Kilo Code",
  amp: "Amp",
  zed: "Zed",
  cherry: "Cherry Studio",
  openclaw: "OpenClaw",
  crush: "Crush",
  aider: "Aider",
  warp: "Warp",
  windsurf: "Windsurf",
  devin: "Devin",
};

export function sourceLabel(source: string): string {
  return SOURCE_LABEL[source] ?? source;
}

export function listedSourceLabels(): ReadonlySet<string> {
  return new Set(Object.keys(SOURCE_LABEL));
}
