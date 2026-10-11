import type { Role } from "../api/types";
import { viewQueryString, type ViewQuery } from "./query";

export type Route =
  | { page: "overview" }
  | { page: "members" }
  | { page: "member"; accountId: number }
  | { page: "projects" }
  | { page: "project"; projectId: number }
  | { page: "session"; sessionId: number }
  | { page: "pricing" }
  | { page: "timeline" };

function parseId(text: string | undefined): number | null {
  if (!text) return null;
  const id = Number(text);
  return Number.isSafeInteger(id) ? id : null;
}

export function parseRoute(hash: string): Route | null {
  const withoutHash = hash.replace(/^#\/?/, "");
  const path = (withoutHash.split("?")[0] ?? "").replace(/\/$/, "");
  if (path === "overview") return { page: "overview" };
  if (path === "members") return { page: "members" };
  if (path === "projects") return { page: "projects" };
  if (path === "pricing") return { page: "pricing" };
  if (path === "timeline") return { page: "timeline" };
  const member = parseId(/^member\/(\d+)$/.exec(path)?.[1]);
  if (member !== null) return { page: "member", accountId: member };
  const project = parseId(/^project\/(\d+)$/.exec(path)?.[1]);
  if (project !== null) return { page: "project", projectId: project };
  const session = parseId(/^session\/(\d+)$/.exec(path)?.[1]);
  if (session !== null) return { page: "session", sessionId: session };
  return null;
}

export function routeHash(route: Route): string {
  switch (route.page) {
    case "overview":
      return "#/overview";
    case "members":
      return "#/members";
    case "member":
      return `#/member/${route.accountId}`;
    case "projects":
      return "#/projects";
    case "project":
      return `#/project/${route.projectId}`;
    case "session":
      return `#/session/${route.sessionId}`;
    case "pricing":
      return "#/pricing";
    case "timeline":
      return "#/timeline";
  }
}

/** 路径 + 区间 / 费用口径查询串。侧栏跳转要带上当前查询，避免丢掉日期。 */
export function buildHash(route: Route, query?: ViewQuery): string {
  const base = routeHash(route);
  if (!query) return base;
  return `${base}?${viewQueryString(query)}`;
}

/** 管理员落在团队总览；成员只有自己的页面。 */
export function homeRoute(role: Role, accountId: number): Route {
  return role === "admin" ? { page: "overview" } : { page: "member", accountId };
}

/**
 * 成员不能进总览、成员管理与价目，也不能看别人；越界一律拉回自己的页面（服务端仍会 403）。
 * 项目页、会话页与工作时间线成员也能进：项目页与时间线只统计自己的用量，会话由服务端按归属放行。
 */
export function allowedRoute(route: Route | null, role: Role, accountId: number): Route {
  const home = homeRoute(role, accountId);
  if (!route) return home;
  if (role === "admin") return route;
  switch (route.page) {
    case "member":
      return route.accountId === accountId ? route : home;
    case "projects":
    case "project":
    case "session":
    case "timeline":
      return route;
    default:
      return home;
  }
}
