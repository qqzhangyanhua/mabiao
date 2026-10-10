import { describe, expect, it } from "vitest";
import { sourcePresenceSummary, type SourcePresenceRow } from "./sourcePresence";

function row(
  source: string,
  roots: Array<{ path: string; exists: boolean }>,
): SourcePresenceRow {
  return { source, application: source, roots };
}

describe("sourcePresenceSummary", () => {
  it("splits sources by whether any default dir exists", () => {
    const { found, missing } = sourcePresenceSummary([
      row("codex", [{ path: "/home/me/.codex/sessions", exists: true }]),
      row("pi", [{ path: "/home/me/.pi/agent/sessions", exists: false }]),
      row("claude", [
        { path: "/home/me/.claude/projects", exists: false },
        { path: "/home/me/.config/claude/projects", exists: true },
      ]),
    ]);
    expect(found.map((item) => item.source)).toEqual(["codex", "claude"]);
    expect(missing.map((item) => item.source)).toEqual(["pi"]);
  });
});
