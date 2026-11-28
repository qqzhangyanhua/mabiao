import { DISPLAY_NAMES } from "../generated/displayIdentity";

function isDisplaySlug(source: string): source is keyof typeof DISPLAY_NAMES {
  return Object.hasOwn(DISPLAY_NAMES, source);
}

export function sourceLabel(source: string): string {
  return isDisplaySlug(source) ? DISPLAY_NAMES[source] : source;
}
