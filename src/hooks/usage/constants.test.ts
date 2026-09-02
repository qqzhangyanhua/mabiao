import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  AUTO_REFRESH_STORAGE_KEY,
  CURSOR_ACCOUNT_AUTO_REFRESH_MINUTES,
  CURSOR_ACCOUNT_AUTO_REFRESH_STORAGE_KEY,
  loadCursorAccountAutoRefresh,
  parseCursorAccountAutoRefresh,
} from "./constants";

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

describe("cursor account auto refresh", () => {
  beforeEach(() => {
    installMemoryStorage();
  });

  afterEach(() => {
    localStorage.clear();
  });

  it("uses a storage key distinct from local ingest auto refresh", () => {
    expect(CURSOR_ACCOUNT_AUTO_REFRESH_STORAGE_KEY).not.toBe(AUTO_REFRESH_STORAGE_KEY);
    expect(CURSOR_ACCOUNT_AUTO_REFRESH_STORAGE_KEY).toBe("mabiao:cursor-account-auto-refresh");
  });

  it("keeps a fixed interval that does not follow the 1/5/10 minute ingest timer", () => {
    expect(CURSOR_ACCOUNT_AUTO_REFRESH_MINUTES).toBe(10);
  });

  it("defaults to off and only treats on as enabled", () => {
    expect(parseCursorAccountAutoRefresh(null)).toBe(false);
    expect(parseCursorAccountAutoRefresh("off")).toBe(false);
    expect(parseCursorAccountAutoRefresh("1")).toBe(false);
    expect(parseCursorAccountAutoRefresh("5")).toBe(false);
    expect(parseCursorAccountAutoRefresh("10")).toBe(false);
    expect(parseCursorAccountAutoRefresh("on")).toBe(true);
  });

  it("reads the independent storage key and ignores the ingest timer value", () => {
    localStorage.setItem(AUTO_REFRESH_STORAGE_KEY, "1");
    expect(loadCursorAccountAutoRefresh()).toBe(false);
    localStorage.setItem(CURSOR_ACCOUNT_AUTO_REFRESH_STORAGE_KEY, "on");
    expect(loadCursorAccountAutoRefresh()).toBe(true);
  });
});
