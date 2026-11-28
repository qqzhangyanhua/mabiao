import {
  BRAND_BY_SLUG,
  BRAND_CHART_COLORS,
  DEFAULT_BRAND_CHART_COLOR,
  type BrandId,
} from "../generated/displayIdentity";

const PALETTE = [
  "var(--chart-1)",
  "var(--chart-2)",
  "var(--chart-3)",
  "var(--chart-4)",
  "var(--chart-5)",
  "var(--chart-6)",
] as const;

function brandOfSlug(slug: string): BrandId | null {
  if (!Object.hasOwn(BRAND_BY_SLUG, slug)) {
    return null;
  }
  return BRAND_BY_SLUG[slug as keyof typeof BRAND_BY_SLUG];
}

export function sourceTone(source: string): string {
  const brand = brandOfSlug(source);
  return brand ? BRAND_CHART_COLORS[brand] : DEFAULT_BRAND_CHART_COLOR;
}

export function sliceColor(key: string, index: number): string {
  const brand = brandOfSlug(key);
  if (brand) {
    return BRAND_CHART_COLORS[brand];
  }
  return PALETTE[index % PALETTE.length] ?? "var(--chart-1)";
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
