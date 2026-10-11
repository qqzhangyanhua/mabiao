import { describe, expect, it } from "vitest";
import { buildHash, allowedRoute, homeRoute, parseRoute, routeHash } from "./route";

describe("parseRoute", () => {
  it("parses the three pages with or without slashes", () => {
    expect(parseRoute("#/overview")).toEqual({ page: "overview" });
    expect(parseRoute("#/members/")).toEqual({ page: "members" });
    expect(parseRoute("#/member/12")).toEqual({ page: "member", accountId: 12 });
  });

  it("parses the project, session and pricing pages", () => {
    expect(parseRoute("#/projects")).toEqual({ page: "projects" });
    expect(parseRoute("#/project/4")).toEqual({ page: "project", projectId: 4 });
    expect(parseRoute("#/session/31/")).toEqual({ page: "session", sessionId: 31 });
    expect(parseRoute("#/pricing")).toEqual({ page: "pricing" });
    expect(parseRoute("#/timeline")).toEqual({ page: "timeline" });
  });

  it("ignores the hash query string used for range and cost mode", () => {
    expect(parseRoute("#/overview?range=7d&mode=unified")).toEqual({ page: "overview" });
    expect(parseRoute("#/member/12?range=custom&from=2026-03-01&to=2026-03-08")).toEqual({
      page: "member",
      accountId: 12,
    });
  });

  it("returns null for anything else", () => {
    for (const hash of ["", "#", "#/", "#/member/abc", "#/member/", "#/member/1/2", "#/nope", "#/project/x", "#/session/", "#/session/1/2"]) {
      expect(parseRoute(hash), hash).toBeNull();
    }
  });

  it("round-trips through routeHash", () => {
    for (const route of [
      { page: "overview" },
      { page: "members" },
      { page: "member", accountId: 7 },
      { page: "projects" },
      { page: "project", projectId: 8 },
      { page: "session", sessionId: 9 },
      { page: "pricing" },
      { page: "timeline" },
    ] as const) {
      expect(parseRoute(routeHash(route))).toEqual(route);
    }
  });

  it("buildHash appends the view query", () => {
    expect(
      buildHash(
        { page: "members" },
        { preset: "7d", mode: "unified", fromDay: "2026-03-01", toDay: "2026-03-07" },
      ),
    ).toBe("#/members?range=7d&mode=unified");
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
    expect(allowedRoute({ page: "pricing" }, "member", 5)).toEqual(own);
    expect(allowedRoute({ page: "member", accountId: 6 }, "member", 5)).toEqual(own);
    expect(allowedRoute(null, "member", 5)).toEqual(own);
  });

  it("lets members open project and session pages (the server scopes the data)", () => {
    for (const route of [
      { page: "projects" },
      { page: "project", projectId: 3 },
      { page: "session", sessionId: 3 },
      { page: "timeline" },
    ] as const) {
      expect(allowedRoute(route, "member", 5)).toEqual(route);
    }
  });
});
