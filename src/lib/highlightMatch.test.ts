import { describe, expect, it } from "vitest";
import { splitHighlight } from "./highlightMatch";


describe("splitHighlight", () => {
  it("splits the first case-insensitive hit", () => {
    expect(splitHighlight("Changed AUTH in login", "auth")).toEqual({
      before: "Changed ",
      match: "AUTH",
      after: " in login",
    });
  });

  it("returns null when the query is missing", () => {
    expect(splitHighlight("hello", "auth")).toBeNull();
    expect(splitHighlight("hello", "  ")).toBeNull();
  });
});
