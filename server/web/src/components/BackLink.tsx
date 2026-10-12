import { ArrowLeft } from "lucide-react";
import { createContext, useContext, useState, type ReactNode } from "react";
import { routeLabel, sameRoute, type Route } from "../lib/route";
import { useHref } from "../view";

const PreviousRouteContext = createContext<Route | null>(null);

/**
 * 记下「上一个不同的页面」。区间、费用口径只改查询串，不算换页；
 * 所以用路径（页面 + id）做键，查询串变化不会冲掉上一页。
 */
export function BackTrail({ route, children }: { route: Route; children: ReactNode }) {
  const [trail, setTrail] = useState<{ current: Route; previous: Route | null }>({
    current: route,
    previous: null,
  });
  if (!sameRoute(trail.current, route)) {
    setTrail({ current: route, previous: trail.current });
  }
  return (
    <PreviousRouteContext.Provider value={trail.previous}>{children}</PreviousRouteContext.Provider>
  );
}

/**
 * 详情页顶部的返回链接。优先回到刚才来的页面；直接打开详情页（没有上一页）时回 `fallback`。
 * 是真正的 <a>：键盘、中键新标签都可用，且带上当前区间与费用口径。
 */
export function BackLink({ current, fallback }: { current: Route; fallback: Route }) {
  const previous = useContext(PreviousRouteContext);
  const href = useHref();
  const target = previous && !sameRoute(previous, current) ? previous : fallback;
  return (
    <a
      href={href(target)}
      className="inline-flex w-fit items-center gap-1.5 rounded-full border border-border bg-card/60 py-1 pr-3.5 pl-2.5 text-sm text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
    >
      <ArrowLeft size={15} aria-hidden="true" />
      返回{routeLabel(target)}
    </a>
  );
}
