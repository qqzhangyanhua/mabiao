import { describe, expect, it } from "vitest";
import {
  defaultSharePreference,
  parseSharePreference,
  serializeSharePreference,
} from "./sharePreference";

describe("parseSharePreference", () => {
  it("defaults to week when the payload is missing or unreadable", () => {
    expect(parseSharePreference(null)).toEqual(defaultSharePreference());
    expect(parseSharePreference("")).toEqual({ kind: "week", quotaProvider: null });
    expect(parseSharePreference("{")).toEqual({ kind: "week", quotaProvider: null });
    expect(parseSharePreference("[]")).toEqual({ kind: "week", quotaProvider: null });
    expect(parseSharePreference('"quota"')).toEqual({ kind: "week", quotaProvider: null });
  });

  it("defaults to week when kind is missing or unknown", () => {
    expect(parseSharePreference(JSON.stringify({ quotaProvider: "cursor" }))).toEqual({
      kind: "week",
      quotaProvider: null,
    });
    expect(parseSharePreference(JSON.stringify({ kind: "template", quotaProvider: "cursor" }))).toEqual({
      kind: "week",
      quotaProvider: null,
    });
  });

  it("reads kind and quota account and ignores a leftover week offset", () => {
    expect(
      parseSharePreference(
        JSON.stringify({ kind: "quota", quotaProvider: "custom:abc", offset: 3 }),
      ),
    ).toEqual({ kind: "quota", quotaProvider: "custom:abc" });
    expect(parseSharePreference(JSON.stringify({ kind: "week", quotaProvider: "cursor" }))).toEqual({
      kind: "week",
      quotaProvider: "cursor",
    });
    expect(parseSharePreference(JSON.stringify({ kind: "quota" }))).toEqual({
      kind: "quota",
      quotaProvider: null,
    });
  });
});

describe("serializeSharePreference", () => {
  it("writes only kind and quota account, never a week offset", () => {
    expect(JSON.parse(serializeSharePreference({ kind: "quota", quotaProvider: "cursor" }))).toEqual({
      kind: "quota",
      quotaProvider: "cursor",
    });
    expect(serializeSharePreference({ kind: "week", quotaProvider: null })).not.toContain("offset");
  });
});
