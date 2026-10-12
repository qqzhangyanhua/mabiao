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

/** 纵轴上限取 1/2/2.5/5/10 × 10^n 里不小于 max 的最小值，四等分后刻度是整数。 */
export function niceCeil(max: number): number {
  if (!(max > 0)) return 0;
  const magnitude = 10 ** Math.floor(Math.log10(max));
  const step = [1, 2, 2.5, 5, 10].find((m) => m * magnitude >= max) ?? 10;
  return step * magnitude;
}

/** 迷你趋势线的 SVG 折线点，坐标落在 width × height 内，留 pad 防描边被裁。 */
export function sparkPoints(values: number[], width: number, height: number, pad = 2): string {
  if (values.length === 0) return "";
  const max = Math.max(...values);
  const min = Math.min(...values);
  const span = max - min || 1;
  const stepX = values.length > 1 ? (width - pad * 2) / (values.length - 1) : 0;
  return values
    .map((value, index) => {
      const x = pad + index * stepX;
      const y = height - pad - ((value - min) / span) * (height - pad * 2);
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");
}
