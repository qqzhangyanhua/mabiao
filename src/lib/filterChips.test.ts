import { describe, expect, it } from "vitest";
import {
  clearDimensionFilters,
  filterChips,
  hasDimensionFilters,
  removeFilterChip,
  rawProviderName,
  withModelFilter,
  withProviderFilter,
} from "./filterChips";
import type { Filter } from "../types";

const filter: Filter = {
  from: null,
  to: null,
  sources: ["claude"],
  models: ["gpt-5"],
  projects: ["/proj/a"],
  providers: ["anthropic"],
};

describe("filterChips", () => {
  it("lists every selected dimension as a chip", () => {
    expect(filterChips(filter).map((chip) => chip.id)).toEqual([
      "project:/proj/a",
      "source:claude",
      "model:gpt-5",
      "provider:anthropic",
    ]);
  });

  it("clears only dimension filters", () => {
    const next = clearDimensionFilters({ ...filter, from: "2026-08-01" });
    expect(next.from).toBe("2026-08-01");
    expect(hasDimensionFilters(next)).toBe(false);
  });

  it("replaces the model dimension when merging a chart click", () => {
    expect(withModelFilter(filter, "claude-opus").models).toEqual(["claude-opus"]);
    expect(withModelFilter(filter, "claude-opus").sources).toEqual(["claude"]);
  });

  it("replaces the provider dimension when drilling into a breakdown row", () => {
    expect(withProviderFilter(filter, "tongban").providers).toEqual(["tongban"]);
    expect(withProviderFilter(filter, "tongban").models).toEqual(["gpt-5"]);
  });

  it("maps the unlabeled breakdown label back to an empty provider key", () => {
    expect(rawProviderName("（未标注）")).toBe("");
    expect(rawProviderName("tongban")).toBe("tongban");
  });

  it("removes a single chip", () => {
    const next = removeFilterChip(filter, { id: "source:claude", kind: "source", value: "claude" });
    expect(next.sources).toEqual([]);
    expect(next.models).toEqual(["gpt-5"]);
  });
});
