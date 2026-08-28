import type { PriceEntry } from "../types";

export function priceRowKey(model: string, provider: string | null): string {
  return `${model}\u001f${provider ?? ""}`;
}

/** 把诊断候选的四个口径预填成用户价目草稿；不写 origin，确认保存后才是用户单价。 */
export function prefillCandidatePrice(
  current: PriceEntry[],
  group: { model: string; provider: string },
  candidate: PriceEntry,
): PriceEntry[] {
  const provider = group.provider.trim() === "" ? null : group.provider;
  const nextEntry: PriceEntry = {
    model: group.model,
    provider,
    input: candidate.input,
    output: candidate.output,
    cache_read: candidate.cache_read,
    cache_creation: candidate.cache_creation,
  };
  const key = priceRowKey(nextEntry.model, nextEntry.provider);
  const index = current.findIndex((row) => priceRowKey(row.model, row.provider) === key);
  if (index >= 0) {
    return current.map((row, i) => (i === index ? { ...row, ...nextEntry } : row));
  }
  return [...current, nextEntry];
}
