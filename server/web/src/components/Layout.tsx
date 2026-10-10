import type { ReactNode } from "react";
import { useAuth } from "../auth";
import { routeHash, type Route } from "../lib/route";

interface NavItem {
  label: string;
  route: Route;
  active: boolean;
}

export function Layout({ route, children }: { route: Route; children: ReactNode }) {
  const { profile, logout } = useAuth();
  const items: NavItem[] = [];
  if (profile.role === "admin") {
    items.push(
      { label: "团队总览", route: { page: "overview" }, active: route.page === "overview" },
      { label: "成员", route: { page: "members" }, active: route.page === "members" },
    );
  }
  items.push({
    label: "我的数据",
    route: { page: "member", accountId: profile.id },
    active: route.page === "member" && route.accountId === profile.id,
  });

  return (
    <div className="min-h-screen">
      <header className="border-b border-slate-200 bg-white">
        <div className="mx-auto flex max-w-6xl items-center gap-6 px-4 py-3">
          <span className="font-semibold">码表 · 团队</span>
          <nav className="flex gap-4 text-sm">
            {items.map((item) => (
              <a
                key={item.label}
                href={routeHash(item.route)}
                aria-current={item.active ? "page" : undefined}
                className={
                  item.active ? "font-medium text-blue-700" : "text-slate-600 hover:text-slate-900"
                }
              >
                {item.label}
              </a>
            ))}
          </nav>
          <div className="ml-auto flex items-center gap-3 text-sm text-slate-600">
            <span>
              {profile.account}（{profile.role === "admin" ? "管理员" : "成员"}）
            </span>
            <button
              type="button"
              onClick={logout}
              className="rounded border border-slate-300 px-2 py-1 hover:bg-slate-100"
            >
              退出
            </button>
          </div>
        </div>
      </header>
      <main className="mx-auto max-w-6xl space-y-6 px-4 py-6">{children}</main>
    </div>
  );
}
