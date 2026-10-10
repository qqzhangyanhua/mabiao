import { describe, expect, it } from "vitest";
import { allowedRoute, homeRoute, parseRoute, routeHash } from "./route";

describe("parseRoute", () => {
  it("parses the three pages with or without slashes", () => {
    expect(parseRoute("#/overview")).toEqual({ page: "overview" });
    expect(parseRoute("#/members/")).toEqual({ page: "members" });
    expect(parseRoute("#/member/12")).toEqual({ page: "member", accountId: 12 });
  });

  it("returns null for anything else", () => {
    for (const hash of ["", "#", "#/", "#/member/abc", "#/member/", "#/member/1/2", "#/nope"]) {
      expect(parseRoute(hash), hash).toBeNull();
    }
  });

  it("round-trips through routeHash", () => {
    for (const route of [
      { page: "overview" },
      { page: "members" },
      { page: "member", accountId: 7 },
    ] as const) {
      expect(parseRoute(routeHash(route))).toEqual(route);
    }
  });
});

describe("allowedRoute", () => {
  it("lets admins go anywhere and defaults them to the overview", () => {
    expect(allowedRoute({ page: "member", accountId: 9 }, "admin", 1)).toEqual({
      page: "member",
      accountId: 9,
    });
    expect(allowedRoute(null, "admin", 1)).toEqual({ page: "overview" });
  });

  it("keeps members on their own page", () => {
    const own = { page: "member", accountId: 5 } as const;
    expect(homeRoute("member", 5)).toEqual(own);
    expect(allowedRoute(own, "member", 5)).toEqual(own);
    expect(allowedRoute({ page: "overview" }, "member", 5)).toEqual(own);
    expect(allowedRoute({ page: "members" }, "member", 5)).toEqual(own);
    expect(allowedRoute({ page: "member", accountId: 6 }, "member", 5)).toEqual(own);
    expect(allowedRoute(null, "member", 5)).toEqual(own);
  });
});
