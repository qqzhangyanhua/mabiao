import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { WORK_NOTES_POSTER_STYLES } from "../workNotes/posterStyleRegistry";
import {
  defaultWorkNotesPosterPreference,
  loadWorkNotesPosterPreference,
  parseWorkNotesPosterPreference,
  saveWorkNotesPosterPreference,
  serializeWorkNotesPosterPreference,
  WORK_NOTES_POSTER_PREFERENCE_KEY,
} from "./workNotesPosterPreference";

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

describe("parseWorkNotesPosterPreference", () => {
  it("defaults when the payload is missing or unreadable", () => {
    expect(parseWorkNotesPosterPreference(null)).toEqual(defaultWorkNotesPosterPreference());
    expect(parseWorkNotesPosterPreference("")).toEqual(defaultWorkNotesPosterPreference());
    expect(parseWorkNotesPosterPreference("{")).toEqual(defaultWorkNotesPosterPreference());
    expect(parseWorkNotesPosterPreference("[]")).toEqual(defaultWorkNotesPosterPreference());
  });

  it("reads a stored work notes poster style id", () => {
    for (const style of WORK_NOTES_POSTER_STYLES) {
      expect(parseWorkNotesPosterPreference(JSON.stringify({ posterStyleId: style.id }))).toEqual({
        posterStyleId: style.id,
      });
    }
  });

  it("falls back when the id is missing, a report style, or malformed", () => {
    expect(parseWorkNotesPosterPreference(JSON.stringify({})).posterStyleId).toBe("folio-ruled");
    expect(
      parseWorkNotesPosterPreference(JSON.stringify({ posterStyleId: "dark-analytics" }))
        .posterStyleId,
    ).toBe("folio-ruled");
    expect(
      parseWorkNotesPosterPreference(JSON.stringify({ posterStyleId: "not-a-style" }))
        .posterStyleId,
    ).toBe("folio-ruled");
    expect(
      parseWorkNotesPosterPreference(JSON.stringify({ posterStyleId: "" })).posterStyleId,
    ).toBe("folio-ruled");
    expect(
      parseWorkNotesPosterPreference(JSON.stringify({ posterStyleId: 42 })).posterStyleId,
    ).toBe("folio-ruled");
  });
});

describe("work notes poster preference round-trip", () => {
  beforeEach(() => {
    installMemoryStorage();
  });

  afterEach(() => {
    localStorage.clear();
  });

  it("preserves style id through serialize and parse", () => {
    for (const style of WORK_NOTES_POSTER_STYLES) {
      const preference = { posterStyleId: style.id };
      expect(
        parseWorkNotesPosterPreference(serializeWorkNotesPosterPreference(preference)),
      ).toEqual(preference);
    }
  });

  it("persists the chosen style in localStorage", () => {
    expect(loadWorkNotesPosterPreference()).toEqual(defaultWorkNotesPosterPreference());
    saveWorkNotesPosterPreference({ posterStyleId: "dusk-brief" });
    expect(localStorage.getItem(WORK_NOTES_POSTER_PREFERENCE_KEY)).toContain("dusk-brief");
    expect(loadWorkNotesPosterPreference()).toEqual({ posterStyleId: "dusk-brief" });
  });
});
