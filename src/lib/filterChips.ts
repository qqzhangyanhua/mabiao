import type { Filter } from "../types";

export type FilterChip = {
  id: string;
  kind: "project" | "source" | "model" | "provider";
  value: string;
};

export function filterChips(filter: Filter): FilterChip[] {
  return [
    ...filter.projects.map((value) => ({
      id: `project:${value}`,
      kind: "project" as const,
      value,
    })),
    ...filter.sources.map((value) => ({ id: `source:${value}`, kind: "source" as const, value })),
    ...filter.models.map((value) => ({ id: `model:${value}`, kind: "model" as const, value })),
    ...filter.providers.map((value) => ({
      id: `provider:${value}`,
      kind: "provider" as const,
      value,
    })),
  ];
}

export function hasDimensionFilters(filter: Filter): boolean {
  return filterChips(filter).length > 0;
}

export function clearDimensionFilters(filter: Filter): Filter {
  return { ...filter, projects: [], sources: [], models: [], providers: [] };
}

export function withModelFilter(filter: Filter, model: string): Filter {
  return { ...filter, models: [model] };
}

export function withProviderFilter(filter: Filter, provider: string): Filter {
  return { ...filter, providers: [provider] };
}

/** 聚合行展示名「（未标注）」对应库里的空 provider。 */
export function rawProviderName(name: string): string {
  return name === "（未标注）" ? "" : name;
}

export function removeFilterChip(filter: Filter, chip: FilterChip): Filter {
  if (chip.kind === "project") {
    return { ...filter, projects: filter.projects.filter((item) => item !== chip.value) };
  }
  if (chip.kind === "source") {
    return { ...filter, sources: filter.sources.filter((item) => item !== chip.value) };
  }
  if (chip.kind === "model") {
    return { ...filter, models: filter.models.filter((item) => item !== chip.value) };
  }
  return { ...filter, providers: filter.providers.filter((item) => item !== chip.value) };
}
