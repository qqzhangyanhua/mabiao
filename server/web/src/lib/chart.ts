const PALETTE = [
  "var(--chart-1)",
  "var(--chart-2)",
  "var(--chart-3)",
  "var(--chart-4)",
  "var(--chart-5)",
  "var(--chart-6)",
] as const;

/** 来源 key 与桌面端 tone 对齐；其余维度按序取图表色。 */
const SOURCE_COLOR: Record<string, string> = {
  claude: "var(--tone-claude)",
  codex: "var(--tone-codex)",
  pi: "var(--tone-pi)",
  omp: "var(--tone-omp)",
  opencode: "var(--tone-open)",
  kimi: "var(--tone-kimi)",
  gemini: "var(--tone-gemini)",
  grok: "var(--tone-grok)",
  qwen: "var(--tone-qwen)",
  cursor: "var(--tone-cursor)",
  cursor_agent: "var(--tone-cursor)",
  factory: "var(--tone-codex)",
  droid: "var(--tone-codex)",
  dsh: "var(--tone-open)",
  copilot: "var(--tone-cursor)",
  hermes: "var(--tone-pi)",
  agy: "var(--tone-gemini)",
  antigravity: "var(--tone-gemini)",
  qoder: "var(--tone-qwen)",
  qoder_cn: "var(--tone-qwen)",
  cline: "var(--tone-open)",
  workbuddy: "var(--tone-kimi)",
  zcode: "var(--tone-omp)",
  alma: "var(--tone-grok)",
};

export function sourceTone(source: string): string {
  return SOURCE_COLOR[source] ?? "var(--tone-cursor)";
}

export function sliceColor(key: string, index: number): string {
  return SOURCE_COLOR[key] ?? PALETTE[index % PALETTE.length] ?? "var(--chart-1)";
}
