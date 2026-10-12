import {
  ChartColumn,
  Clock,
  Download,
  Folder,
  Moon,
  Settings,
  Sun,
  UserRound,
  Users,
  type LucideIcon,
} from "lucide-react";
import type { ReactNode } from "react";
import { useAuth } from "../auth";
import { useExportBar } from "../export-action";
import { COST_MODE_LABEL, type CostMode } from "../lib/cost";
import { cn } from "../lib/cn";
import type { Route } from "../lib/route";
import type { RangePreset } from "../lib/range";
import { useTheme } from "../theme";
import { useHref, useView } from "../view";
import { BackTrail } from "./BackLink";
import { OverflowMenu } from "./OverflowMenu";
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
  icon: LucideIcon;
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
        {
          label: "团队总览",
          icon: Users,
          route: { page: "overview" },
          active: route.page === "overview",
        },
        {
          label: "工作时间线",
          icon: Clock,
          route: { page: "timeline" },
          active: route.page === "timeline",
        },
      ],
    });
  }
  groups.push({
    title: role === "admin" ? "浏览" : "分析",
    items: [
      ...(role === "admin"
        ? [
            {
              label: "成员",
              icon: UserRound,
              route: { page: "members" as const },
              active: route.page === "members",
            },
          ]
        : [
            {
              label: "工作时间线",
              icon: Clock,
              route: { page: "timeline" as const },
              active: route.page === "timeline",
            },
          ]),
      {
        label: "项目",
        icon: Folder,
        route: { page: "projects" },
        active: route.page === "projects" || route.page === "project",
      },
      {
        label: "我的数据",
        icon: ChartColumn,
        route: { page: "member", accountId },
        active: route.page === "member" && route.accountId === accountId,
      },
    ],
  });
  if (role === "admin") {
    groups.push({
      title: "管理",
      items: [
        {
          label: "价目",
          icon: Settings,
          route: { page: "pricing" },
          active: route.page === "pricing",
        },
      ],
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
    <aside className="flex w-full shrink-0 flex-col border-b border-sidebar-border bg-sidebar text-sidebar-foreground backdrop-blur-xl backdrop-saturate-150 md:sticky md:top-0 md:h-screen md:w-60 md:border-r md:border-b-0">
      <div className="flex items-center gap-3 px-5 py-5">
        <span className="bg-brand grid size-9 shrink-0 place-items-center rounded-xl text-sm font-semibold text-white shadow-[0_6px_18px_-6px_var(--primary)] ring-1 ring-white/25">
          码
        </span>
        <div>
          <div className="text-[15px] font-semibold text-white">码表</div>
          <div className="text-[11px] text-sidebar-muted">团队</div>
        </div>
      </div>
      <nav className="flex gap-3 overflow-x-auto px-3 pb-3 md:flex-1 md:flex-col md:overflow-y-auto md:overflow-x-hidden">
        {groups.map((group) => (
          <div key={group.title} className="flex shrink-0 flex-col gap-1">
            <div className="hidden px-3 pt-4 pb-1 text-xs text-sidebar-muted md:block">
              {group.title}
            </div>
            <div className="flex gap-1 md:flex-col">
              {group.items.map((item) => {
                const Icon = item.icon;
                return (
                  <a
                    key={item.label}
                    href={href(item.route)}
                    aria-current={item.active ? "page" : undefined}
                    className={cn(
                      "flex shrink-0 items-center gap-2.5 rounded-xl px-3 py-2 text-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-white/60",
                      item.active
                        ? "bg-[linear-gradient(90deg,rgb(99_102_241/75%),rgb(139_92_246/45%))] font-medium text-sidebar-accent-foreground shadow-[0_8px_20px_-10px_rgb(99_102_241)] ring-1 ring-white/15"
                        : "font-normal hover:bg-sidebar-accent hover:text-white",
                    )}
                  >
                    <Icon size={17} strokeWidth={1.8} aria-hidden="true" />
                    {item.label}
                  </a>
                );
              })}
            </div>
          </div>
        ))}
      </nav>
      <div className="flex items-center gap-2.5 border-t border-sidebar-border px-4 py-4">
        <span
          aria-hidden="true"
          className="grid size-9 shrink-0 place-items-center rounded-full bg-[linear-gradient(135deg,#94a3ff,#c4b5fd)] text-sm font-semibold text-[#1e1b4b] ring-2 ring-white/20"
        >
          {profile.account.slice(0, 1).toUpperCase()}
        </span>
        <div className="min-w-0 flex-1">
          <div className="truncate text-sm font-medium text-white">{profile.account}</div>
          <div className="text-xs text-sidebar-muted">
            {profile.role === "admin" ? "管理员" : "成员"}
          </div>
        </div>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          onClick={toggle}
          aria-label={theme === "dark" ? "切换到浅色" : "切换到深色"}
          className="text-sidebar-foreground hover:bg-sidebar-accent hover:text-white"
        >
          {theme === "dark" ? <Sun size={16} /> : <Moon size={16} />}
        </Button>
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={logout}
          className="border-white/15 bg-white/8 text-sidebar-foreground hover:bg-white/15 hover:text-white"
        >
          退出
        </Button>
      </div>
    </aside>
  );
}

function TopBar({ hideRange }: { hideRange: boolean }) {
  const view = useView();
  const { actions } = useExportBar();
  const [primary, ...secondary] = actions;
  return (
    <div className="flex flex-wrap items-center gap-3 px-4 pt-4 md:px-8 md:pt-6">
      {!hideRange && (
        <div className="glass flex flex-wrap items-center gap-2 rounded-2xl p-1.5">
          <Segmented
            label="时间范围"
            value={view.preset}
            onChange={view.setPreset}
            options={PRESETS}
            size="compact"
          />
          {view.preset === "custom" && (
            <div className="flex items-center gap-2 text-xs">
              <Input
                type="date"
                aria-label="开始日期"
                value={view.custom.fromDay}
                onChange={(e) => view.setCustom({ ...view.custom, fromDay: e.target.value })}
                className="h-7 w-auto text-xs"
              />
              <span className="text-muted-foreground">至</span>
              <Input
                type="date"
                aria-label="结束日期"
                value={view.custom.toDay}
                onChange={(e) => view.setCustom({ ...view.custom, toDay: e.target.value })}
                className="h-7 w-auto text-xs"
              />
            </div>
          )}
          <span aria-hidden="true" className="hidden h-5 w-px bg-border sm:block" />
          <Segmented
            label="费用口径"
            value={view.mode}
            onChange={view.setMode}
            options={COST_OPTIONS}
            size="compact"
          />
        </div>
      )}
      <div className="ml-auto flex items-center gap-2">
        {primary && (
          <Button
            type="button"
            variant="default"
            size="sm"
            disabled={primary.disabled}
            onClick={primary.onClick}
            className="h-9 rounded-xl px-4"
          >
            {primary.label.startsWith("导出") && <Download size={15} aria-hidden="true" />}
            {primary.label}
          </Button>
        )}
        {secondary.length > 0 && <OverflowMenu actions={secondary} />}
      </div>
    </div>
  );
}

export function Layout({ route, children }: { route: Route; children: ReactNode }) {
  const hideRange = route.page === "session" || route.page === "pricing";
  return (
    <BackTrail route={route}>
      <div className="flex min-h-screen flex-col md:flex-row">
        <Sidebar route={route} />
        <div className="flex min-w-0 flex-1 flex-col">
          <TopBar hideRange={hideRange} />
          <main className="min-w-0 flex-1 px-4 py-6 md:px-8">
            <div className="mx-auto flex max-w-6xl flex-col gap-5">{children}</div>
          </main>
        </div>
      </div>
    </BackTrail>
  );
}
