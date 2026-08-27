import { describe, expect, it } from "vitest";
import { prefillCandidatePrice, priceRowKey } from "./priceCandidate";
import type { PriceEntry } from "../types";

const candidate: PriceEntry = {
  model: "claude-sonnet-4-6",
  provider: null,
  input: 3 / 1_000_000,
  output: 15 / 1_000_000,
  cache_read: 0.3 / 1_000_000,
  cache_creation: 3.75 / 1_000_000,
  origin: "snapshot",
};

describe("prefillCandidatePrice", () => {
  it("appends a user draft keyed by the unpriced group, not the snapshot name", () => {
    const current: PriceEntry[] = [
      {
        model: "gpt-5",
        provider: null,
        input: 1,
        output: 2,
        cache_read: 0,
        cache_creation: 0,
      },
    ];
    const next = prefillCandidatePrice(
      current,
      { model: "claude-4.6-sonnet", provider: "anthropic" },
      candidate,
    );
    expect(next).toHaveLength(2);
    expect(next[1]).toEqual({
      model: "claude-4.6-sonnet",
      provider: "anthropic",
      input: candidate.input,
      output: candidate.output,
      cache_read: candidate.cache_read,
      cache_creation: candidate.cache_creation,
    });
    expect(next[1]?.origin).toBeUndefined();
    expect(current).toHaveLength(1);
  });

  it("replaces an existing draft for the same model and provider", () => {
    const current: PriceEntry[] = [
      {
        model: "claude-4.6-sonnet",
        provider: "anthropic",
        input: 0,
        output: 0,
        cache_read: 0,
        cache_creation: 0,
      },
    ];
    const next = prefillCandidatePrice(
      current,
      { model: "claude-4.6-sonnet", provider: "anthropic" },
      candidate,
    );
    expect(next).toHaveLength(1);
    expect(next[0]?.input).toBe(candidate.input);
    expect(next[0]?.output).toBe(candidate.output);
  });

  it("treats a blank provider as null", () => {
    const next = prefillCandidatePrice([], { model: "gpt-5-high", provider: "" }, candidate);
    expect(next[0]?.provider).toBeNull();
    expect(priceRowKey(next[0]?.model ?? "", next[0]?.provider ?? null)).toBe(
      priceRowKey("gpt-5-high", null),
    );
  });
});
