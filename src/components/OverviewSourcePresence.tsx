import { SourceLabel } from "./SourceIcon";
import { Button } from "./ui/Button";
import { EmptyState } from "./EmptyState";
import {
  sourcePresenceSummary,
  type SourcePresenceDto,
  type SourcePresenceRow,
} from "../lib/sourcePresence";

export function OverviewSourcePresence({
  data,
  onOpenScanPaths,
}: {
  data: SourcePresenceDto;
  onOpenScanPaths: () => void;
}) {
  const { found, missing } = sourcePresenceSummary(data.rows);
  return (
    <article className="panel overview-presence-panel">
      <EmptyState
        icon="overview"
        title="本机还没有消耗记录"
        hint="下面是各来源的默认扫描目录在不在。只读探测，不会改任何文件。路径不对时去设置页填写绝对路径。"
        action={
          <Button variant="accent" onClick={onOpenScanPaths}>
            打开扫描路径
          </Button>
        }
      />
      <div className="overview-presence-cols">
        <PresenceGroup
          title={`已找到默认目录（${found.length}）`}
          empty="还没有来源的默认目录存在。若 CLI 装在非默认位置，请到扫描路径里填写。"
          rows={found}
          present
        />
        <PresenceGroup
          title={`未找到默认目录（${missing.length}）`}
          empty="所有来源的默认目录都在。"
          rows={missing}
          present={false}
        />
      </div>
    </article>
  );
}

function PresenceGroup({
  title,
  empty,
  rows,
  present,
}: {
  title: string;
  empty: string;
  rows: SourcePresenceRow[];
  present: boolean;
}) {
  return (
    <section className="overview-presence-group">
      <h3>{title}</h3>
      {rows.length === 0 ? (
        <p className="muted">{empty}</p>
      ) : (
        <ul>
          {rows.map((row) => (
            <li key={row.source}>
              <SourceLabel source={row.source} fallback={row.application} size={14} />
              <span className="overview-presence-paths">
                {row.roots.map((root) => (
                  <code key={root.path} className={root.exists === present ? undefined : "muted"}>
                    {root.path}
                  </code>
                ))}
              </span>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
