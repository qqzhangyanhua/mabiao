import { Moon, Sun } from "lucide-react";
import type { ReactNode } from "react";
import { useAuth } from "../auth";
import { useExportBar } from "../export-action";
import { COST_MODE_LABEL, type CostMode } from "../lib/cost";
import { cn } from "../lib/cn";
import type { Route } from "../lib/route";
import type { RangePreset } from "../lib/range";
import { useTheme } from "../theme";
import { useHref, useView } from "../view";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { Segmented } from "./ui/segmented";

const PRESETS: { id: RangePreset; label: string }[] = [
  { id: "7d", label: "近 7 天" },
  { id: "30d", label: "近 30 天" },
  { id: "month", label: "本月" },
  { id: "custom", label: "自定义" },
];

const COST_OPTIONS = (Object.keys(COST_MODE_LABEL) as CostMode[]).map((id) => ({
  id,
  label: COST_MODE_LABEL[id],
}));

interface NavItem {
  label: string;
  route: Route;
  active: boolean;
}

interface NavGroup {
  title: string;
  items: NavItem[];
}

function navGroups(route: Route, accountId: number, role: "admin" | "member"): NavGroup[] {
  const groups: NavGroup[] = [];
  if (role === "admin") {
    groups.push({
      title: "分析",
      items: [
        { label: "团队总览", route: { page: "overview" }, active: route.page === "overview" },
        { label: "工作时间线", route: { page: "timeline" }, active: route.page === "timeline" },
      ],
    });
  }
  groups.push({
    title: role === "admin" ? "浏览" : "分析",
    items: [
      ...(role === "admin"
        ? [{ label: "成员", route: { page: "members" as const }, active: route.page === "members" }]
        : [
            {
              label: "工作时间线",
              route: { page: "timeline" as const },
              active: route.page === "timeline",
            },
          ]),
      {
        label: "项目",
        route: { page: "projects" },
        active: route.page === "projects" || route.page === "project",
      },
      {
        label: "我的数据",
        route: { page: "member", accountId },
        active: route.page === "member" && route.accountId === accountId,
      },
    ],
  });
  if (role === "admin") {
    groups.push({
      title: "管理",
      items: [{ label: "价目", route: { page: "pricing" }, active: route.page === "pricing" }],
    });
  }
  return groups;
}

function Sidebar({ route }: { route: Route }) {
  const { profile, logout } = useAuth();
  const { theme, toggle } = useTheme();
  const href = useHref();
  const groups = navGroups(route, profile.id, profile.role);

  return (
    <aside className="flex w-full shrink-0 flex-col border-b border-sidebar-border bg-sidebar md:sticky md:top-0 md:h-screen md:w-60 md:border-r md:border-b-0">
      <div className="flex items-center gap-2.5 px-4 py-4">
        <span className="grid size-8 shrink-0 place-items-center rounded-lg bg-primary text-sm font-semibold text-primary-foreground">
          码
        </span>
        <div>
          <div className="text-sm font-semibold tracking-wide text-foreground">码表</div>
          <div className="text-xs text-muted-foreground">团队</div>
        </div>
      </div>
      <nav className="flex gap-3 overflow-x-auto px-3 pb-3 md:flex-1 md:flex-col md:overflow-y-auto md:overflow-x-hidden">
        {groups.map((group) => (
          <div key={group.title} className="flex shrink-0 flex-col gap-1">
            <div className="hidden px-3 pt-3 pb-1 text-[11px] font-medium tracking-wide text-muted-foreground uppercase md:block">
              {group.title}
            </div>
            <div className="flex gap-1 md:flex-col">
              {group.items.map((item) => (
                <a
                  key={item.label}
                  href={href(item.route)}
                  aria-current={item.active ? "page" : undefined}
                  className={cn(
                    "shrink-0 rounded-lg px-3 py-2 text-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                    item.active
                      ? "bg-sidebar-accent font-medium text-sidebar-accent-foreground"
                      : "text-sidebar-foreground hover:bg-accent hover:text-foreground",
                  )}
                >
                  {item.label}
                </a>
              ))}
            </div>
          </div>
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
  );
}

function TopBar({ hideRange }: { hideRange: boolean }) {
  const view = useView();
  const { actions } = useExportBar();
  return (
    <div className="flex flex-wrap items-center gap-3 border-b border-border bg-card px-4 py-3 md:px-8">
      {!hideRange && (
        <>
          <Segmented label="时间范围" value={view.preset} onChange={view.setPreset} options={PRESETS} />
          {view.preset === "custom" && (
            <div className="flex items-center gap-2 text-sm">
              <Input
                type="date"
                aria-label="开始日期"
                value={view.custom.fromDay}
                onChange={(e) => view.setCustom({ ...view.custom, fromDay: e.target.value })}
                className="w-auto"
              />
              <span className="text-muted-foreground">至</span>
              <Input
                type="date"
                aria-label="结束日期"
                value={view.custom.toDay}
                onChange={(e) => view.setCustom({ ...view.custom, toDay: e.target.value })}
                className="w-auto"
              />
            </div>
          )}
          <Segmented label="费用口径" value={view.mode} onChange={view.setMode} options={COST_OPTIONS} />
        </>
      )}
      <div className="ml-auto flex flex-wrap gap-2">
        {actions.map((action) => (
          <Button
            key={action.label}
            type="button"
            variant="outline"
            size="sm"
            disabled={action.disabled}
            onClick={action.onClick}
          >
            {action.label}
          </Button>
        ))}
      </div>
    </div>
  );
}

export function Layout({ route, children }: { route: Route; children: ReactNode }) {
  const hideRange = route.page === "session" || route.page === "pricing";
  return (
    <div className="flex min-h-screen flex-col md:flex-row">
      <Sidebar route={route} />
      <div className="flex min-w-0 flex-1 flex-col">
        <TopBar hideRange={hideRange} />
        <main className="min-w-0 flex-1 px-4 py-6 md:px-8">
          <div className="mx-auto flex max-w-6xl flex-col gap-6">{children}</div>
        </main>
      </div>
    </div>
  );
}
