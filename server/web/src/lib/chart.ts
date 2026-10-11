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
  cursor_agent: "var(--tone-cursor)",
};

export function sliceColor(key: string, index: number): string {
  return SOURCE_COLOR[key] ?? PALETTE[index % PALETTE.length] ?? "var(--chart-1)";
}
