import { afterEach, beforeEach, describe, expect, it } from "vitest";
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
  it("defaults to week when the payload is missing or unreadable", () => {
    expect(parseSharePreference(null)).toEqual(defaultSharePreference());
    expect(parseSharePreference("")).toEqual(defaultSharePreference());
    expect(parseSharePreference("{")).toEqual(defaultSharePreference());
    expect(parseSharePreference("[]")).toEqual(defaultSharePreference());
    expect(parseSharePreference('"quota"')).toEqual(defaultSharePreference());
  });

  it("defaults to week when kind is missing or unknown", () => {
    expect(parseSharePreference(JSON.stringify({ quotaProvider: "cursor" }))).toEqual({
      kind: "week",
      quotaProvider: null,
      posterStyleId: "dark-analytics",
    });
    expect(
      parseSharePreference(JSON.stringify({ kind: "template", quotaProvider: "cursor" })),
    ).toEqual({
      kind: "week",
      quotaProvider: null,
      posterStyleId: "dark-analytics",
    });
  });

  it("reads kind and quota account and ignores a leftover week offset", () => {
    expect(
      parseSharePreference(
        JSON.stringify({ kind: "quota", quotaProvider: "custom:abc", offset: 3 }),
      ),
    ).toEqual({ kind: "quota", quotaProvider: "custom:abc", posterStyleId: "dark-analytics" });
    expect(parseSharePreference(JSON.stringify({ kind: "week", quotaProvider: "cursor" }))).toEqual(
      {
        kind: "week",
        quotaProvider: "cursor",
        posterStyleId: "dark-analytics",
      },
    );
    expect(parseSharePreference(JSON.stringify({ kind: "quota" }))).toEqual({
      kind: "quota",
      quotaProvider: null,
      posterStyleId: "dark-analytics",
    });
  });

  it("reads a stored poster style id", () => {
    expect(
      parseSharePreference(JSON.stringify({ kind: "week", posterStyleId: "dark-analytics" })),
    ).toEqual({
      kind: "week",
      quotaProvider: null,
      posterStyleId: "dark-analytics",
    });
    expect(
      parseSharePreference(JSON.stringify({ kind: "week", posterStyleId: "light-glass" })),
    ).toEqual({
      kind: "week",
      quotaProvider: null,
      posterStyleId: "light-glass",
    });
    expect(
      parseSharePreference(JSON.stringify({ kind: "week", posterStyleId: "bauhaus-print" })),
    ).toEqual({
      kind: "week",
      quotaProvider: null,
      posterStyleId: "bauhaus-print",
    });
    expect(
      parseSharePreference(JSON.stringify({ kind: "week", posterStyleId: "newsprint" })),
    ).toEqual({
      kind: "week",
      quotaProvider: null,
      posterStyleId: "newsprint",
    });
    expect(
      parseSharePreference(JSON.stringify({ kind: "week", posterStyleId: "ink-wash" })),
    ).toEqual({
      kind: "week",
      quotaProvider: null,
      posterStyleId: "ink-wash",
    });
    expect(
      parseSharePreference(JSON.stringify({ kind: "week", posterStyleId: "ticket-stub" })),
    ).toEqual({
      kind: "week",
      quotaProvider: null,
      posterStyleId: "ticket-stub",
    });
    expect(
      parseSharePreference(JSON.stringify({ kind: "week", posterStyleId: "fuse-bead" })),
    ).toEqual({
      kind: "week",
      quotaProvider: null,
      posterStyleId: "fuse-bead",
    });
    expect(
      parseSharePreference(JSON.stringify({ kind: "week", posterStyleId: "cast-concrete" })),
    ).toEqual({
      kind: "week",
      quotaProvider: null,
      posterStyleId: "cast-concrete",
    });
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
  it("writes kind, quota account, and poster style id, never a week offset", () => {
    expect(
      JSON.parse(
        serializeSharePreference({
          kind: "quota",
          quotaProvider: "cursor",
          posterStyleId: "dark-analytics",
        }),
      ),
    ).toEqual({
      kind: "quota",
      quotaProvider: "cursor",
      posterStyleId: "dark-analytics",
    });
    expect(
      serializeSharePreference({
        kind: "week",
        quotaProvider: null,
        posterStyleId: "dark-analytics",
      }),
    ).not.toContain("offset");
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
    const preference = {
      kind: "week" as const,
      quotaProvider: null,
      posterStyleId: "dark-analytics" as const,
    };
    expect(parseSharePreference(serializeSharePreference(preference))).toEqual(preference);
    const lightGlass = {
      kind: "week" as const,
      quotaProvider: null,
      posterStyleId: "light-glass" as const,
    };
    expect(parseSharePreference(serializeSharePreference(lightGlass))).toEqual(lightGlass);
    const bauhausPrint = {
      kind: "week" as const,
      quotaProvider: null,
      posterStyleId: "bauhaus-print" as const,
    };
    expect(parseSharePreference(serializeSharePreference(bauhausPrint))).toEqual(bauhausPrint);
    const newsprint = {
      kind: "week" as const,
      quotaProvider: null,
      posterStyleId: "newsprint" as const,
    };
    expect(parseSharePreference(serializeSharePreference(newsprint))).toEqual(newsprint);
    const inkWash = {
      kind: "week" as const,
      quotaProvider: null,
      posterStyleId: "ink-wash" as const,
    };
    expect(parseSharePreference(serializeSharePreference(inkWash))).toEqual(inkWash);
    const ticketStub = {
      kind: "week" as const,
      quotaProvider: null,
      posterStyleId: "ticket-stub" as const,
    };
    expect(parseSharePreference(serializeSharePreference(ticketStub))).toEqual(ticketStub);
    const fuseBead = {
      kind: "week" as const,
      quotaProvider: null,
      posterStyleId: "fuse-bead" as const,
    };
    expect(parseSharePreference(serializeSharePreference(fuseBead))).toEqual(fuseBead);
    const castConcrete = {
      kind: "week" as const,
      quotaProvider: null,
      posterStyleId: "cast-concrete" as const,
    };
    expect(parseSharePreference(serializeSharePreference(castConcrete))).toEqual(castConcrete);
  });

  it("loads stored preferences from localStorage and falls back when missing or malformed", () => {
    expect(loadSharePreference()).toEqual(defaultSharePreference());

    localStorage.setItem(
      SHARE_PREFERENCE_STORAGE_KEY,
      serializeSharePreference({
        kind: "week",
        quotaProvider: null,
        posterStyleId: "dark-analytics",
      }),
    );
    expect(loadSharePreference()).toEqual({
      kind: "week",
      quotaProvider: null,
      posterStyleId: "dark-analytics",
    });

    localStorage.setItem(
      SHARE_PREFERENCE_STORAGE_KEY,
      JSON.stringify({ kind: "week", posterStyleId: "unknown-style" }),
    );
    expect(loadSharePreference()).toEqual(defaultSharePreference());
  });

  it("persists poster style id through save and load", () => {
    saveSharePreference({
      kind: "week",
      quotaProvider: null,
      posterStyleId: "dark-analytics",
    });
    expect(loadSharePreference().posterStyleId).toBe("dark-analytics");
    saveSharePreference({
      kind: "week",
      quotaProvider: null,
      posterStyleId: "light-glass",
    });
    expect(loadSharePreference().posterStyleId).toBe("light-glass");
    saveSharePreference({
      kind: "week",
      quotaProvider: null,
      posterStyleId: "bauhaus-print",
    });
    expect(loadSharePreference().posterStyleId).toBe("bauhaus-print");
    saveSharePreference({
      kind: "week",
      quotaProvider: null,
      posterStyleId: "newsprint",
    });
    expect(loadSharePreference().posterStyleId).toBe("newsprint");
    saveSharePreference({
      kind: "week",
      quotaProvider: null,
      posterStyleId: "ink-wash",
    });
    expect(loadSharePreference().posterStyleId).toBe("ink-wash");
    saveSharePreference({
      kind: "week",
      quotaProvider: null,
      posterStyleId: "ticket-stub",
    });
    expect(loadSharePreference().posterStyleId).toBe("ticket-stub");
    saveSharePreference({
      kind: "week",
      quotaProvider: null,
      posterStyleId: "fuse-bead",
    });
    expect(loadSharePreference().posterStyleId).toBe("fuse-bead");
    saveSharePreference({
      kind: "week",
      quotaProvider: null,
      posterStyleId: "cast-concrete",
    });
    expect(loadSharePreference().posterStyleId).toBe("cast-concrete");
  });
});
