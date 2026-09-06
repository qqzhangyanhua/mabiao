import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  defaultWorkNotesPreference,
  engineSelectLabel,
  installedWorkNoteEngines,
  loadWorkNotesPreference,
  parseWorkNotesPreference,
  resolveWorkNotesEngine,
  saveWorkNotesPreference,
  serializeWorkNotesPreference,
  WORK_NOTES_PREFERENCE_STORAGE_KEY,
  workNotesEngineLabel,
} from "./workNotesPreference";
import type { DetectedEngine } from "../types";

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

const CLAUDE: DetectedEngine = {
  id: "claude",
  program: "claude",
  writes_session_dir: false,
  installed: true,
  version: "2.0.0",
};

const CODEX_MISSING: DetectedEngine = {
  id: "codex",
  program: "codex",
  writes_session_dir: false,
  installed: false,
  version: null,
};

const GROK_WRITES: DetectedEngine = {
  id: "grok",
  program: "grok",
  writes_session_dir: true,
  installed: true,
  version: "1",
};

describe("parseWorkNotesPreference", () => {
  it("defaults when the payload is missing or unreadable", () => {
    expect(parseWorkNotesPreference(null)).toEqual(defaultWorkNotesPreference());
    expect(parseWorkNotesPreference("")).toEqual(defaultWorkNotesPreference());
    expect(parseWorkNotesPreference("{")).toEqual(defaultWorkNotesPreference());
    expect(parseWorkNotesPreference("[]")).toEqual(defaultWorkNotesPreference());
  });

  it("reads a stored engine, per-engine models, and detection results", () => {
    expect(
      parseWorkNotesPreference(
        JSON.stringify({
          engineId: "claude",
          models: { claude: "sonnet", codex: "gpt-5.1-codex" },
          detected: [CLAUDE, CODEX_MISSING],
        }),
      ),
    ).toEqual({
      engineId: "claude",
      models: { claude: "sonnet", codex: "gpt-5.1-codex" },
      detected: [CLAUDE, CODEX_MISSING],
    });
  });

  it("drops malformed detected engines and non-string models", () => {
    expect(
      parseWorkNotesPreference(
        JSON.stringify({
          engineId: "claude",
          models: { claude: "sonnet", bad: 1 },
          detected: [CLAUDE, { id: "x" }, "nope"],
        }),
      ),
    ).toEqual({
      engineId: "claude",
      models: { claude: "sonnet" },
      detected: [CLAUDE],
    });
  });
});

describe("installedWorkNoteEngines", () => {
  it("hides CLIs that were not found", () => {
    expect(installedWorkNoteEngines([CLAUDE, CODEX_MISSING]).map((engine) => engine.id)).toEqual([
      "claude",
    ]);
  });
});

describe("resolveWorkNotesEngine", () => {
  it("keeps the last engine when it is still installed", () => {
    expect(resolveWorkNotesEngine("claude", [CLAUDE])).toBe("claude");
  });

  it("falls back to the first installed engine", () => {
    expect(resolveWorkNotesEngine("codex", [CLAUDE])).toBe("claude");
  });

  it("returns null when nothing is installed", () => {
    expect(resolveWorkNotesEngine("codex", [])).toBeNull();
  });
});

describe("engine labels", () => {
  it("names known engines and marks ones that write sessions", () => {
    expect(workNotesEngineLabel("codex")).toBe("Codex");
    expect(workNotesEngineLabel("claude")).toBe("Claude");
    expect(engineSelectLabel(CLAUDE)).toBe("Claude");
    expect(engineSelectLabel(GROK_WRITES)).toBe("grok（会写会话）");
  });
});

describe("load and save work notes preference", () => {
  beforeEach(() => {
    installMemoryStorage();
  });

  afterEach(() => {
    localStorage.clear();
  });

  it("round-trips through localStorage", () => {
    const preference = {
      engineId: "claude",
      models: { claude: "sonnet" },
      detected: [CLAUDE],
    };
    saveWorkNotesPreference(preference);
    expect(localStorage.getItem(WORK_NOTES_PREFERENCE_STORAGE_KEY)).toBe(
      serializeWorkNotesPreference(preference),
    );
    expect(loadWorkNotesPreference()).toEqual(preference);
  });
});
