import { describe, expect, it } from "vitest";
import { hashSearch, parseViewQuery, viewQueryString } from "./query";

describe("hashSearch", () => {
  it("reads the query after the first question mark", () => {
    expect(hashSearch("#/overview?range=7d&mode=snapshot").get("range")).toBe("7d");
    expect(hashSearch("#/members").toString()).toBe("");
  });
});

describe("parseViewQuery", () => {
  it("keeps only known presets and modes", () => {
    expect(parseViewQuery("#/overview?range=7d&mode=snapshot&from=2026-03-01")).toEqual({
      preset: "7d",
      mode: "snapshot",
      fromDay: "2026-03-01",
    });
    expect(parseViewQuery("#/overview?range=year&mode=yen")).toEqual({});
  });
});

describe("viewQueryString", () => {
  it("omits custom dates unless the preset is custom", () => {
    expect(
      viewQueryString({
        preset: "30d",
        mode: "unified",
        fromDay: "2026-03-01",
        toDay: "2026-03-30",
      }),
    ).toBe("range=30d&mode=unified");
    expect(
      viewQueryString({
        preset: "custom",
        mode: "snapshot",
        fromDay: "2026-03-01",
        toDay: "2026-03-08",
      }),
    ).toBe("range=custom&mode=snapshot&from=2026-03-01&to=2026-03-08");
  });
});
