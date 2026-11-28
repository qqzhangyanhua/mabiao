import { BRAND_BY_SLUG, type BrandId } from "../generated/displayIdentity";

export type SourceIconId = BrandId | "unknown";

function isIdentitySlug(source: string): source is keyof typeof BRAND_BY_SLUG {
  return Object.hasOwn(BRAND_BY_SLUG, source);
}

export function resolveSourceIconId(source: string): SourceIconId {
  if (!isIdentitySlug(source)) {
    return "unknown";
  }
  return BRAND_BY_SLUG[source] ?? "unknown";
}
