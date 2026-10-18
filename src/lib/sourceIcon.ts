import { BRAND_BY_SLUG, type BrandId } from "../generated/displayIdentity";

export type SourceIconId = BrandId | "unknown";

/** 品牌到配色 class。少一个品牌时 tsc 失败。Antigravity（agy）沿用 Gemini 的配色。 */
const BRAND_TONE = {
  claude: "tone-claude",
  codex: "tone-codex",
  grok: "tone-grok",
  gemini: "tone-gemini",
  kimi: "tone-kimi",
  qwen: "tone-qwen",
  copilot: "tone-copilot",
  opencode: "tone-open",
  factory: "tone-factory",
  pi: "tone-pi",
  omp: "tone-omp",
  dsh: "tone-dsh",
  cursor: "tone-cursor",
  hermes: "tone-hermes",
  agy: "tone-gemini",
  qoder: "tone-qoder",
  cline: "tone-cline",
  workbuddy: "tone-workbuddy",
  zcode: "tone-zcode",
  alma: "tone-alma",
  codebuddy: "tone-codebuddy",
  trae: "tone-trae",
  goose: "tone-goose",
  roo: "tone-roo",
  kilo: "tone-kilo",
  amp: "tone-amp",
  zed: "tone-zed",
  cherry: "tone-cherry",
  openclaw: "tone-openclaw",
  crush: "tone-crush",
  aider: "tone-aider",
} as const satisfies Record<BrandId, string>;

export function brandToneClass(brand: BrandId): string {
  return BRAND_TONE[brand];
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
