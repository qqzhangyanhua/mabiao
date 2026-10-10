import type { Role } from "../api/types";

export type Route =
  { page: "overview" } | { page: "members" } | { page: "member"; accountId: number };

export function parseRoute(hash: string): Route | null {
  const path = hash.replace(/^#\/?/, "").replace(/\/$/, "");
  if (path === "overview") return { page: "overview" };
  if (path === "members") return { page: "members" };
  const member = /^member\/(\d+)$/.exec(path);
  if (member?.[1]) {
    const accountId = Number(member[1]);
    if (Number.isSafeInteger(accountId)) return { page: "member", accountId };
  }
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
  }
}

/** 管理员落在团队总览；成员只有自己的页面。 */
export function homeRoute(role: Role, accountId: number): Route {
  return role === "admin" ? { page: "overview" } : { page: "member", accountId };
}

/** 成员不能进总览与成员管理，也不能看别人；越界一律拉回自己的页面（服务端仍会 403）。 */
export function allowedRoute(route: Route | null, role: Role, accountId: number): Route {
  const home = homeRoute(role, accountId);
  if (!route) return home;
  if (role === "admin") return route;
  return route.page === "member" && route.accountId === accountId ? route : home;
}
