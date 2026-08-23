import { describe, expect, it } from "vitest";
import type { GlobalInstructionFile, GlobalInstructionSourceRow } from "../types";
import {
  canEditInstruction,
  canOpenInstruction,
  idleSourceLabel,
  isIdleSource,
  showsEvidenceBadge,
  showsLoadBadge,
  showsLoadStatus,
} from "./instructionAccess";

function file(overrides: Partial<GlobalInstructionFile> = {}): GlobalInstructionFile {
  return {
    kind: "file",
    display_path: "~/.claude/CLAUDE.md",
    abs_path: "/tmp/.claude/CLAUDE.md",
    byte_size: 4,
    modified_at: null,
    load_status: "loaded",
    evidence: "verified",
    content: "ok\n",
    error: null,
    note: null,
    action: null,
    editable: false,
    ...overrides,
  };
}

describe("canOpenInstruction", () => {
  it("allows disk-backed entries including directories", () => {
    expect(canOpenInstruction(file())).toBe(true);
    expect(
      canOpenInstruction(
        file({
          kind: "directory",
          display_path: "~/.claude/rules/",
          abs_path: "/tmp/.claude/rules",
        }),
      ),
    ).toBe(true);
  });

  it("hides the entry when the path is locally invisible", () => {
    expect(
      canOpenInstruction(
        file({
          display_path: "Cursor 账号级偏好",
          abs_path: "",
          load_status: "locally_invisible",
          action: "cursor_settings",
        }),
      ),
    ).toBe(false);
  });
});

describe("showsLoadStatus", () => {
  it("hides load status when the source has no global-instruction mechanism", () => {
    expect(
      showsLoadStatus(
        file({
          display_path: "无用户级全局指令机制",
          abs_path: "",
          load_status: "not_created",
          evidence: "no_mechanism",
        }),
      ),
    ).toBe(false);
    expect(showsLoadStatus(file({ load_status: "not_created" }))).toBe(true);
  });
});

describe("showsLoadBadge", () => {
  it("hides the default loaded state", () => {
    expect(showsLoadBadge(file({ load_status: "loaded" }))).toBe(false);
    expect(showsLoadBadge(file({ load_status: "not_created" }))).toBe(true);
    expect(
      showsLoadBadge(
        file({
          load_status: "not_created",
          evidence: "no_mechanism",
        }),
      ),
    ).toBe(false);
  });
});

describe("showsEvidenceBadge", () => {
  it("hides verified and keeps the exceptions", () => {
    expect(showsEvidenceBadge(file({ evidence: "verified" }))).toBe(false);
    expect(showsEvidenceBadge(file({ evidence: "inferred" }))).toBe(true);
    expect(showsEvidenceBadge(file({ evidence: "no_mechanism" }))).toBe(true);
  });
});

describe("canEditInstruction", () => {
  it("follows the backend editable flag", () => {
    expect(canEditInstruction(file({ editable: false }))).toBe(false);
    expect(canEditInstruction(file({ editable: true }))).toBe(true);
  });
});

describe("isIdleSource", () => {
  it("groups not-created and no-mechanism sources", () => {
    const idle: GlobalInstructionSourceRow = {
      source: "gemini",
      application: "Gemini",
      files: [file({ display_path: "~/.gemini/GEMINI.md", load_status: "not_created" })],
    };
    const loaded: GlobalInstructionSourceRow = {
      source: "claude",
      application: "Claude",
      files: [file({ load_status: "loaded" })],
    };
    const invisible: GlobalInstructionSourceRow = {
      source: "cursor",
      application: "Cursor",
      files: [file({ load_status: "locally_invisible", abs_path: "" })],
    };
    expect(isIdleSource(idle)).toBe(true);
    expect(idleSourceLabel(idle)).toBe("未创建");
    expect(isIdleSource(loaded)).toBe(false);
    expect(isIdleSource(invisible)).toBe(false);
  });

  it("labels no-mechanism sources as such", () => {
    const row: GlobalInstructionSourceRow = {
      source: "kimi",
      application: "Kimi",
      files: [file({ evidence: "no_mechanism", load_status: "not_created", abs_path: "" })],
    };
    expect(isIdleSource(row)).toBe(true);
    expect(idleSourceLabel(row)).toBe("无机制");
  });
});
