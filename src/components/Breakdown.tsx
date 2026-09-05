import { Fragment, memo, useCallback, useMemo, useState, type KeyboardEvent } from "react";
import { Icon, type IconName } from "../icons";
import { breakdownBarOption } from "../lib/chartTheme";
import type { ResolvedTheme } from "../hooks/useTheme";
import { canExpandProjectSessions, rawProviderName } from "../lib/filterChips";
import { formatCost, formatTokens, projectLabel, providerChannel } from "../lib/format";
import { unpricedKpiLink } from "../lib/unpricedKpi";
import type { Filter, NamedAmount } from "../types";
import { BreakdownCallTable } from "./BreakdownCallTable";
import { BreakdownProjectSessions } from "./BreakdownProjectSessions";
import { EmptyState } from "./EmptyState";
import { ExportableChart } from "./ExportableChart";
import { ExportButton } from "./ExportButton";
import { KpiCard } from "./Kpi";
import { ModelLabel } from "./VendorIcon";

function rowClassName(selected: boolean, clickable: boolean): string | undefined {
  if (clickable) {
    return selected ? "clickable selected" : "clickable";
  }
  return selected ? "selected" : undefined;
}

function projectRowTitle(name: string, selected: boolean): string {
  if (!canExpandProjectSessions(name)) {
    return selected ? "收起说明" : "账号用量无项目路径，不能下钻会话";
  }
  return selected ? "收起该项目的会话" : "展开该项目下的会话";
}

function channelClass(channel: string): string {
  if (channel === "官方") return "official";
  if (channel === "中转") return "relay";
  return "unlabeled";
}

export const Breakdown = memo(function Breakdown({
  title,
  icon,
  rows,
  showProviderChannel,
  showVendorIcon,
  projectNames,
  showCallDetails,
  filter,
  revision,
  theme,
  onProviderClick,
  onOpenConversation,
  onOpenUnpricedDiagnosis,
  onError,
}: {
  title: string;
  icon: IconName;
  rows: NamedAmount[];
  showProviderChannel?: boolean;
  showVendorIcon?: boolean;
  projectNames?: boolean;
  showCallDetails?: boolean;
  filter?: Filter;
  revision?: string;
  theme: ResolvedTheme;
  onProviderClick?: (provider: string) => void;
  onOpenConversation?: (session: { id: string; source: string }) => void;
  onOpenUnpricedDiagnosis?: () => void;
  onError?: (error: unknown) => void;
}) {
  const [selectedProject, setSelectedProject] = useState<string | null>(null);
  const label = useCallback(
    (row: NamedAmount): string => {
      const name = projectNames ? projectLabel(row.name) : row.name;
      return showProviderChannel ? `${name}（${providerChannel(row.name)}）` : name;
    },
    [projectNames, showProviderChannel],
  );

  const option = useMemo(() => {
    const labels = rows.map(label).reverse();
    const values = rows.map((row) => row.total_tokens).reverse();
    return breakdownBarOption(labels, values, theme);
  }, [rows, label, theme]);

  const stats = useMemo(() => {
    const totalTokens = rows.reduce((sum, row) => sum + row.total_tokens, 0);
    const unpricedCount = rows.filter((row) => row.unpriced).length;
    const totalCost = rows.reduce((sum, row) => sum + (row.cost ?? 0), 0);
    const hasCost = rows.some((row) => row.cost != null);
    return { totalTokens, unpricedCount, totalCost, hasCost };
  }, [rows]);

  const top = rows.slice(0, 6);
  const maxTotal = Math.max(1, ...rows.map((row) => row.total_tokens));
  const unpricedLink =
    onOpenUnpricedDiagnosis != null ? unpricedKpiLink(stats.unpricedCount) : null;
  const selectedProjectName =
    projectNames && rows.some((row) => row.name === selectedProject) ? selectedProject : null;

  function toggleProject(name: string) {
    setSelectedProject((current) => (current === name ? null : name));
  }

  function onProjectRowKeyDown(event: KeyboardEvent<HTMLTableRowElement>, name: string) {
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      toggleProject(name);
    }
  }

  return (
    <div className="stack">
      <section className="kpi-row">
        <KpiCard icon={icon} tone="purple" label="覆盖条目" value={formatTokens(rows.length)} />
        <KpiCard
          icon="tokens"
          tone="cyan"
          label="合计 Token"
          value={formatTokens(stats.totalTokens)}
        />
        <KpiCard
          icon="cost"
          tone="orange"
          label="合计费用"
          value={stats.hasCost ? `$${stats.totalCost.toFixed(2)}` : "—"}
        />
        <KpiCard
          icon="filter"
          tone="blue"
          label="单价未配置"
          value={`${stats.unpricedCount} 项`}
          hint={unpricedLink?.hint}
          actionLabel={unpricedLink?.actionLabel}
          onClick={unpricedLink ? onOpenUnpricedDiagnosis : undefined}
        />
      </section>

      {top.length > 0 ? (
        <div className="panel">
          <div className="panel-head">
            <h2>Top {top.length}</h2>
            <span className="muted">按 Token 用量排序</span>
          </div>
          <ol className="rank-list">
            {top.map((row, index) => (
              <li key={row.name}>
                <span className="rank">{index + 1}</span>
                <span className="rank-name" title={row.name}>
                  {showVendorIcon ? (
                    <ModelLabel name={row.name} fallback={label(row)} />
                  ) : (
                    label(row)
                  )}
                </span>
                <span className="rank-bar">
                  <i style={{ width: `${(row.total_tokens / maxTotal) * 100}%` }} />
                </span>
                <span className="rank-val">{formatTokens(row.total_tokens)}</span>
              </li>
            ))}
          </ol>
        </div>
      ) : null}

      <div className="panel">
        <div className="panel-head">
          <div>
            <h2>{title}</h2>
            {projectNames ? (
              <p className="panel-note">Cursor 为账号用量，无项目路径，单独成一行。</p>
            ) : null}
          </div>
        </div>
        <ExportableChart option={option} style={{ height: 360 }} filename={`${title}图`} />
      </div>
      <div className="panel">
        <div className="panel-head">
          <div>
            <h2>明细列表</h2>
            {onProviderClick ? (
              <p className="panel-note">点击名称可只看该接口的明细调用。</p>
            ) : null}
            {projectNames ? (
              <p className="panel-note">
                点一行在表内展开该项目的对话记录。Cursor 账号用量不能下钻。
              </p>
            ) : null}
          </div>
          <ExportButton
            filename={title}
            headers={["名称", ...(showProviderChannel ? ["渠道"] : []), "占比", "Token", "费用"]}
            rows={rows.map((row) => [
              projectNames ? projectLabel(row.name) : row.name,
              ...(showProviderChannel ? [providerChannel(row.name)] : []),
              `${(row.share * 100).toFixed(1)}%`,
              row.total_tokens,
              row.cost ?? "",
            ])}
          />
        </div>
        <div className="table-scroll">
          <table>
            <thead>
              <tr>
                <th>名称</th>
                {showProviderChannel ? <th>渠道</th> : null}
                <th>占比</th>
                <th>Token</th>
                <th>费用</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => (
                <BreakdownListRow
                  key={row.name}
                  row={row}
                  projectNames={projectNames}
                  showProviderChannel={showProviderChannel}
                  showVendorIcon={showVendorIcon}
                  showCallDetails={showCallDetails}
                  filter={filter}
                  revision={revision}
                  selectedProjectName={selectedProjectName}
                  onToggleProject={toggleProject}
                  onProjectRowKeyDown={onProjectRowKeyDown}
                  onProviderClick={onProviderClick}
                  onOpenConversation={onOpenConversation}
                  onError={onError}
                />
              ))}
              {rows.length === 0 ? (
                <tr>
                  <td colSpan={showProviderChannel ? 5 : 4} className="analytics-empty">
                    <EmptyState icon={icon} title="当前筛选条件下暂无数据" />
                  </td>
                </tr>
              ) : null}
            </tbody>
          </table>
        </div>
      </div>
      {showCallDetails && filter ? (
        <BreakdownCallTable
          filter={filter}
          revision={revision ?? ""}
          onOpenConversation={onOpenConversation}
          onError={onError}
        />
      ) : null}
    </div>
  );
});

function BreakdownListRow({
  row,
  projectNames,
  showProviderChannel,
  showVendorIcon,
  showCallDetails,
  filter,
  revision,
  selectedProjectName,
  onToggleProject,
  onProjectRowKeyDown,
  onProviderClick,
  onOpenConversation,
  onError,
}: {
  row: NamedAmount;
  projectNames?: boolean;
  showProviderChannel?: boolean;
  showVendorIcon?: boolean;
  showCallDetails?: boolean;
  filter?: Filter;
  revision?: string;
  selectedProjectName: string | null;
  onToggleProject: (name: string) => void;
  onProjectRowKeyDown: (event: KeyboardEvent<HTMLTableRowElement>, name: string) => void;
  onProviderClick?: (provider: string) => void;
  onOpenConversation?: (session: { id: string; source: string }) => void;
  onError?: (error: unknown) => void;
}) {
  const displayName = projectNames ? projectLabel(row.name) : row.name;
  const projectClickable = Boolean(projectNames && filter);
  const selected = projectClickable
    ? selectedProjectName === row.name
    : Boolean(
        showCallDetails &&
          filter != null &&
          filter.providers.includes(rawProviderName(row.name)),
      );
  const nameCell = showVendorIcon ? (
    <ModelLabel name={row.name} fallback={displayName} />
  ) : (
    displayName
  );
  return (
    <Fragment>
      <tr
        className={rowClassName(selected, projectClickable)}
        tabIndex={projectClickable ? 0 : undefined}
        aria-expanded={projectClickable ? selected : undefined}
        aria-selected={projectClickable ? selected : undefined}
        title={projectClickable ? projectRowTitle(row.name, selected) : undefined}
        onClick={projectClickable ? () => onToggleProject(row.name) : undefined}
        onKeyDown={
          projectClickable ? (event) => onProjectRowKeyDown(event, row.name) : undefined
        }
      >
        <td title={row.name}>
          {onProviderClick ? (
            <button
              type="button"
              className="rank-link"
              onClick={() => onProviderClick(rawProviderName(row.name))}
            >
              {nameCell}
            </button>
          ) : projectClickable ? (
            <span className="breakdown-expand-toggle">
              <Icon name="chevron" size={12} className="breakdown-expand-caret" />
              {nameCell}
            </span>
          ) : (
            nameCell
          )}
        </td>
        {showProviderChannel ? (
          <td>
            <span className={`channel-badge ${channelClass(providerChannel(row.name))}`}>
              {providerChannel(row.name)}
            </span>
          </td>
        ) : null}
        <td>
          <span className="cell-bar">
            <i style={{ width: `${row.share * 100}%` }} />
          </span>
          <span className="cell-bar-label">{(row.share * 100).toFixed(1)}%</span>
        </td>
        <td>{formatTokens(row.total_tokens)}</td>
        <td>
          {formatCost(row.cost, row.unpriced)}
          {row.unpriced ? " · 单价未配置" : ""}
        </td>
      </tr>
      {selected && projectClickable && filter ? (
        <tr className="breakdown-expand-row">
          <td colSpan={showProviderChannel ? 5 : 4}>
            <BreakdownProjectSessions
              filter={filter}
              project={row.name}
              revision={revision ?? ""}
              onOpenConversation={onOpenConversation}
              onError={onError}
            />
          </td>
        </tr>
      ) : null}
    </Fragment>
  );
}
