import { BRAND_BY_SLUG, type BrandId } from "../generated/displayIdentity";

export type SourceIconId = BrandId | "unknown";

/** 拥有独立配色的品牌；未列入的品牌统一回落到中性 `tone-other`。Antigravity（agy）沿用 Gemini 配色。 */
const CUSTOM_BRAND_TONES: Partial<Record<BrandId, string>> = {
  claude: "tone-claude",
  codex: "tone-codex",
  grok: "tone-grok",
  gemini: "tone-gemini",
  kimi: "tone-kimi",
  qwen: "tone-qwen",
  opencode: "tone-open",
  pi: "tone-pi",
  omp: "tone-omp",
  agy: "tone-gemini",
};

const FALLBACK_BRAND_TONE = "tone-other";

export function brandToneClass(brand: BrandId): string {
  return CUSTOM_BRAND_TONES[brand] ?? FALLBACK_BRAND_TONE;
}

function isIdentitySlug(source: string): source is keyof typeof BRAND_BY_SLUG {
  return Object.hasOwn(BRAND_BY_SLUG, source);
}

export function resolveSourceIconId(source: string): SourceIconId {
  if (!isIdentitySlug(source)) {
    return "unknown";
  }
  return BRAND_BY_SLUG[source] ?? "unknown";
}
