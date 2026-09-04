import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { REPORT_POSTER_STYLES } from "../report/posterStyleRegistry";
import {
  defaultSharePreference,
  loadSharePreference,
  parseSharePreference,
  saveSharePreference,
  SHARE_PREFERENCE_STORAGE_KEY,
  serializeSharePreference,
} from "./sharePreference";

function installMemoryStorage() {
  const store = new Map<string, string>();
  const memory: Storage = {
    get length() {
      return store.size;
    },
    clear() {
      store.clear();
    },
    getItem(key) {
      return store.get(key) ?? null;
    },
    key(index) {
      return [...store.keys()][index] ?? null;
    },
    removeItem(key) {
      store.delete(key);
    },
    setItem(key, value) {
      store.set(key, value);
    },
  };
  Object.defineProperty(globalThis, "localStorage", {
    configurable: true,
    value: memory,
  });
}

describe("parseSharePreference", () => {
  it("defaults when the payload is missing or unreadable", () => {
    expect(parseSharePreference(null)).toEqual(defaultSharePreference());
    expect(parseSharePreference("")).toEqual(defaultSharePreference());
    expect(parseSharePreference("{")).toEqual(defaultSharePreference());
    expect(parseSharePreference("[]")).toEqual(defaultSharePreference());
    expect(parseSharePreference('"quota"')).toEqual(defaultSharePreference());
  });

  it("defaults when kind is unknown", () => {
    expect(parseSharePreference(JSON.stringify({ kind: "template" }))).toEqual(
      defaultSharePreference(),
    );
  });

  it("keeps the poster style when a leftover quota kind is stored", () => {
    expect(
      parseSharePreference(
        JSON.stringify({ kind: "quota", quotaProvider: "custom:abc", posterStyleId: "fuse-bead" }),
      ),
    ).toEqual({ posterStyleId: "fuse-bead" });
    expect(parseSharePreference(JSON.stringify({ kind: "week", quotaProvider: "cursor" }))).toEqual({
      posterStyleId: "dark-analytics",
    });
  });

  it("reads a stored poster style id", () => {
    for (const style of REPORT_POSTER_STYLES) {
      expect(
        parseSharePreference(JSON.stringify({ kind: "week", posterStyleId: style.id })),
      ).toEqual({ posterStyleId: style.id });
    }
  });

  it("falls back to dark-analytics for missing, unknown, or malformed style ids", () => {
    expect(parseSharePreference(JSON.stringify({ kind: "week" })).posterStyleId).toBe(
      "dark-analytics",
    );
    expect(
      parseSharePreference(JSON.stringify({ kind: "week", posterStyleId: "not-a-style" }))
        .posterStyleId,
    ).toBe("dark-analytics");
    expect(
      parseSharePreference(JSON.stringify({ kind: "week", posterStyleId: "purple-glass" }))
        .posterStyleId,
    ).toBe("dark-analytics");
    expect(
      parseSharePreference(JSON.stringify({ kind: "week", posterStyleId: "cyber-neon" }))
        .posterStyleId,
    ).toBe("dark-analytics");
    expect(
      parseSharePreference(JSON.stringify({ kind: "week", posterStyleId: "" })).posterStyleId,
    ).toBe("dark-analytics");
    expect(
      parseSharePreference(JSON.stringify({ kind: "week", posterStyleId: 42 })).posterStyleId,
    ).toBe("dark-analytics");
    expect(
      parseSharePreference(JSON.stringify({ kind: "week", posterStyleId: null })).posterStyleId,
    ).toBe("dark-analytics");
  });
});

describe("serializeSharePreference", () => {
  it("writes week kind and poster style id, never a week offset or quota account", () => {
    expect(JSON.parse(serializeSharePreference({ posterStyleId: "light-glass" }))).toEqual({
      kind: "week",
      quotaProvider: null,
      posterStyleId: "light-glass",
    });
    expect(serializeSharePreference({ posterStyleId: "dark-analytics" })).not.toContain("offset");
  });
});

describe("share preference round-trip", () => {
  beforeEach(() => {
    installMemoryStorage();
  });

  afterEach(() => {
    localStorage.clear();
  });

  it("preserves poster style id through serialize and parse", () => {
    for (const style of REPORT_POSTER_STYLES) {
      const preference = { posterStyleId: style.id };
      expect(parseSharePreference(serializeSharePreference(preference))).toEqual(preference);
    }
  });

  it("loads stored preferences from localStorage and falls back when missing or malformed", () => {
    expect(loadSharePreference()).toEqual(defaultSharePreference());

    localStorage.setItem(
      SHARE_PREFERENCE_STORAGE_KEY,
      serializeSharePreference({ posterStyleId: "dark-analytics" }),
    );
    expect(loadSharePreference()).toEqual({ posterStyleId: "dark-analytics" });

    localStorage.setItem(
      SHARE_PREFERENCE_STORAGE_KEY,
      JSON.stringify({ kind: "week", posterStyleId: "unknown-style" }),
    );
    expect(loadSharePreference()).toEqual(defaultSharePreference());
  });

  it("persists poster style id through save and load", () => {
    for (const style of REPORT_POSTER_STYLES) {
      saveSharePreference({ posterStyleId: style.id });
      expect(loadSharePreference().posterStyleId).toBe(style.id);
    }
  });
});
