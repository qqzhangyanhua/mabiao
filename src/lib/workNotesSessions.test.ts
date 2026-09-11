import { describe, expect, it } from "vitest";
import type { WorkNotesSessionChoice } from "../types";
import {
  allSessionKeys,
  selectedSessionRefs,
  workNotesSessionKey,
} from "./workNotesSessions";

function choice(
  overrides: Partial<WorkNotesSessionChoice> = {},
): WorkNotesSessionChoice {
  return {
    source: "codex",
    session_id: "a",
    title: "对齐口径",
    project: "statistics",
    started_at: "2026-08-18T10:00:00+08:00",
    total_tokens: 2000,
    cached: false,
    ...overrides,
  };
}

describe("workNotesSessionKey", () => {
  it("keeps source and session id apart", () => {
    expect(workNotesSessionKey({ source: "codex", session_id: "a" })).toBe("codex\0a");
  });
});

describe("selectedSessionRefs", () => {
  it("keeps listed order and drops unchecked rows", () => {
    const sessions = [
      choice({ session_id: "a" }),
      choice({ source: "claude", session_id: "a" }),
      choice({ session_id: "b" }),
    ];
    const keys = new Set([
      workNotesSessionKey(sessions[0]),
      workNotesSessionKey(sessions[2]),
    ]);
    expect(selectedSessionRefs(sessions, keys)).toEqual([
      { source: "codex", session_id: "a" },
      { source: "codex", session_id: "b" },
    ]);
  });
});

describe("allSessionKeys", () => {
  it("covers every listed session", () => {
    const sessions = [choice({ session_id: "a" }), choice({ session_id: "b" })];
    expect(allSessionKeys(sessions).size).toBe(2);
  });
});
