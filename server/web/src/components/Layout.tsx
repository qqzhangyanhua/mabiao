import { Moon, Sun } from "lucide-react";
import type { ReactNode } from "react";
import { useAuth } from "../auth";
import { cn } from "../lib/cn";
import { routeHash, type Route } from "../lib/route";
import { useTheme } from "../theme";
import { Button } from "./ui/button";

interface NavItem {
  label: string;
  route: Route;
  active: boolean;
}

export function Layout({ route, children }: { route: Route; children: ReactNode }) {
  const { profile, logout } = useAuth();
  const { theme, toggle } = useTheme();
  const items: NavItem[] = [];
  if (profile.role === "admin") {
    items.push(
      { label: "团队总览", route: { page: "overview" }, active: route.page === "overview" },
      { label: "成员", route: { page: "members" }, active: route.page === "members" },
    );
  }
  items.push({
    label: "项目",
    route: { page: "projects" },
    active: route.page === "projects" || route.page === "project",
  });
  items.push({
    label: "我的数据",
    route: { page: "member", accountId: profile.id },
    active: route.page === "member" && route.accountId === profile.id,
  });

  return (
    <div className="flex min-h-screen flex-col md:flex-row">
      <aside className="flex w-full shrink-0 flex-col border-b border-sidebar-border bg-sidebar md:sticky md:top-0 md:h-screen md:w-60 md:border-r md:border-b-0">
        <div className="flex items-center gap-2.5 px-4 pt-4">
          <span className="grid size-8 shrink-0 place-items-center rounded-lg bg-primary text-sm font-semibold text-primary-foreground">
            码
          </span>
          <div>
            <div className="text-sm font-semibold tracking-wide text-foreground">码表</div>
            <div className="text-xs text-muted-foreground">团队</div>
          </div>
        </div>
        <nav className="flex gap-1 overflow-x-auto px-3 py-3 md:flex-1 md:flex-col md:overflow-visible">
          {items.map((item) => (
            <a
              key={item.label}
              href={routeHash(item.route)}
              aria-current={item.active ? "page" : undefined}
              className={cn(
                "shrink-0 rounded-lg px-3 py-2 text-sm",
                item.active
                  ? "bg-sidebar-accent font-medium text-sidebar-accent-foreground"
                  : "text-sidebar-foreground hover:bg-accent hover:text-foreground",
              )}
            >
              {item.label}
            </a>
          ))}
        </nav>
        <div className="flex items-center gap-2 border-t border-sidebar-border px-3 py-3">
          <div className="min-w-0 flex-1">
            <div className="truncate text-sm text-foreground">{profile.account}</div>
            <div className="text-xs text-muted-foreground">
              {profile.role === "admin" ? "管理员" : "成员"}
            </div>
          </div>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            onClick={toggle}
            aria-label={theme === "dark" ? "切换到浅色" : "切换到深色"}
          >
            {theme === "dark" ? <Sun size={16} /> : <Moon size={16} />}
          </Button>
          <Button type="button" variant="outline" size="sm" onClick={logout}>
            退出
          </Button>
        </div>
      </aside>
      <main className="min-w-0 flex-1 px-4 py-6 md:px-8">
        <div className="mx-auto flex max-w-6xl flex-col gap-6">{children}</div>
      </main>
    </div>
  );
}
