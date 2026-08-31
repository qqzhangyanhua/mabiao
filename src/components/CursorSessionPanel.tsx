import { useMemo, useState } from "react";
import {
  breakdownBarOption,
  cursorSessionDailyOption,
  donutOption,
  modelPalette,
} from "../lib/chartTheme";
import type { ResolvedTheme } from "../hooks/useTheme";
import {
  cursorSessionProjectTable,
  cursorSessionToolGroupTable,
  cursorSessionToolTable,
} from "../lib/exportRows";
import {
  formatClock,
  formatCompact,
  formatRatio,
  formatTokens,
  projectLabel,
  relativeTime,
} from "../lib/format";
import type { CursorSessionSummaryDto } from "../types";
import type { ConversationOpenRequest } from "./type";
import { CursorSessionTable } from "./CursorSessionTable";
import { DonutChart } from "./DonutChart";
import { EmptyState } from "./EmptyState";
import { ExportButton } from "./ExportButton";
import { ExportableChart } from "./ExportableChart";
import { KpiCard, LegendRow } from "./Kpi";
import { LoadingOverlay } from "./LoadingOverlay";
import { SESSION_ENTRY_COPY } from "../lib/sessionEntryCopy";

const TOOL_GROUP_LABELS: Record<string, string> = {
  read: "读取",
  write: "写入",
  shell: "命令",
  web: "网络",
  agent: "委派",
  other: "其他",
};

function emptySummary(): CursorSessionSummaryDto {
  return {
    as_of: null,
    session_count: 0,
    turn_count: 0,
    aborted_count: 0,
    user_prompt_count: 0,
    subagent_count: 0,
    error_rate: null,
    average_turns: null,
    average_tools_per_turn: null,
    write_read_ratio: null,
    active_project_count: 0,
    by_project: [],
    by_model: [],
    by_source: [],
    by_extension: [],
    top_tools: [],
    tool_groups: [],
    daily: [],
  };
}

export function CursorSessionPanel({
  summary,
  loading = false,
  theme,
  revision,
  onError,
  onOpenConversation,
}: {
  summary: CursorSessionSummaryDto | null;
  loading?: boolean;
  theme: ResolvedTheme;
  revision: number;
  onError?: (error: unknown) => void;
  onOpenConversation: (session: ConversationOpenRequest) => void;
}) {
  const data = summary ?? emptySummary();
  const [selectedProject, setSelectedProject] = useState<string | null>(null);

  const trendOption = useMemo(
    () => cursorSessionDailyOption(data.daily, theme),
    [data.daily, theme],
  );

  const projectOption = useMemo(() => {
    const top = data.by_project.slice(0, 8);
    const labels = top.map((row) => projectLabel(row.name)).reverse();
    const values = top.map((row) => row.session_count).reverse();
    return breakdownBarOption(labels, values, theme);
  }, [data.by_project, theme]);

  const modelOption = useMemo(() => {
    const slices = data.by_model.map((row, index) => ({
      name: row.name,
      value: row.session_count,
      color: modelPalette[index % modelPalette.length],
    }));
    return donutOption(slices, theme);
  }, [data.by_model, theme]);

  const toolOption = useMemo(() => {
    const top = data.top_tools.slice(0, 10);
    const labels = top.map((row) => row.name).reverse();
    const values = top.map((row) => row.call_count).reverse();
    return breakdownBarOption(labels, values, theme);
  }, [data.top_tools, theme]);

  const toolGroupOption = useMemo(() => {
    const labels = data.tool_groups.map((row) => TOOL_GROUP_LABELS[row.name] ?? row.name).reverse();
    const values = data.tool_groups.map((row) => row.call_count).reverse();
    return breakdownBarOption(labels, values, theme);
  }, [data.tool_groups, theme]);

  const modelTotal = data.by_model.reduce((sum, row) => sum + row.session_count, 0);
  const sourceTotal = data.by_source.reduce((sum, row) => sum + row.session_count, 0);

  if (!summary && loading) {
    return (
      <LoadingOverlay active className="panel partition">
        <EmptyState icon="cursor" title="正在加载会话…" />
      </LoadingOverlay>
    );
  }

  if (!summary || summary.session_count === 0) {
    return (
      <div className="panel partition">
        <EmptyState
          icon="cursor"
          title={SESSION_ENTRY_COPY.cursorSessionsEmptyTitle}
          hint={SESSION_ENTRY_COPY.cursorSessionsEmptyHint}
        />
      </div>
    );
  }

  return (
    <LoadingOverlay active={loading} className="stack">
      <p className="panel-note">{SESSION_ENTRY_COPY.cursorSessionsBanner}</p>
      <section className="kpi-row">
        <KpiCard
          icon="sessions"
          tone="purple"
          label="会话数"
          value={formatTokens(data.session_count)}
        />
        <KpiCard icon="trend" tone="cyan" label="轮次数" value={formatTokens(data.turn_count)} />
        <KpiCard
          icon="filter"
          tone="orange"
          label="失败率"
          value={data.error_rate == null ? "—" : `${(data.error_rate * 100).toFixed(1)}%`}
        />
        <KpiCard
          icon="alertTriangle"
          tone="orange"
          label="中止"
          value={formatTokens(data.aborted_count)}
        />
      </section>
      <section className="kpi-row">
        <KpiCard
          icon="chat"
          tone="blue"
          label="场均轮次"
          value={data.average_turns == null ? "—" : formatRatio(data.average_turns)}
        />
        <KpiCard
          icon="source"
          tone="cyan"
          label="工具/轮"
          value={
            data.average_tools_per_turn == null ? "—" : formatRatio(data.average_tools_per_turn)
          }
        />
        <KpiCard
          icon="inbox"
          tone="purple"
          label="提问数"
          value={formatTokens(data.user_prompt_count)}
        />
        <KpiCard
          icon="project"
          tone="blue"
          label="读写比"
          value={data.write_read_ratio == null ? "—" : formatRatio(data.write_read_ratio)}
        />
      </section>
      <p className="note">
        子代理 transcript 并入父会话，不单独计数。当前合计 {formatTokens(data.subagent_count)}{" "}
        个子代理、{formatTokens(data.active_project_count)} 个活跃项目。
      </p>

      <CursorSessionTable
        revision={revision}
        projectNames={data.by_project.map((row) => row.name)}
        selectedProject={selectedProject}
        onSelectProject={setSelectedProject}
        onSelectSession={(row) =>
          onOpenConversation({ id: row.session_id, source: "cursor_agent" })
        }
        onError={onError}
      />

      <section className="panel partition">
        <div className="panel-head">
          <h2>按天趋势</h2>
        </div>
        <p className="note">独立口径，不计入 token 总量；按会话最后活跃日分桶。</p>
        <ExportableChart
          option={trendOption}
          filename="cursor-session-daily"
          style={{ height: 280 }}
        />
      </section>

      <div className="split-2">
        <section className="panel partition">
          <div className="panel-head">
            <h2>按模型</h2>
          </div>
          {data.by_model.length > 0 ? (
            <div className="donut-wrap">
              <DonutChart option={modelOption} centerValue={formatCompact(modelTotal)} />
              <div className="legend-col">
                {data.by_model.slice(0, 8).map((row, index) => (
                  <LegendRow
                    key={row.name}
                    color={modelPalette[index % modelPalette.length]}
                    label={row.name}
                    value={`${formatTokens(row.session_count)} 会话`}
                  />
                ))}
                {data.by_source.length > 0 ? (
                  <p className="note">
                    来源{" "}
                    {data.by_source
                      .map((row) => `${row.name} ${formatTokens(row.session_count)}`)
                      .join(" · ")}
                    {sourceTotal > 0 ? `（${formatTokens(sourceTotal)}）` : ""}
                  </p>
                ) : null}
                {data.by_extension.length > 0 ? (
                  <p className="note">
                    扩展名{" "}
                    {data.by_extension
                      .map((row) => `${row.name} ${formatTokens(row.file_count)}`)
                      .join(" · ")}
                  </p>
                ) : null}
              </div>
            </div>
          ) : (
            <p className="note">暂无模型 enrich 数据（纯问答或未关联 ai_code_hashes）。</p>
          )}
        </section>

        <section className="panel partition">
          <div className="panel-head">
            <h2>工具调用</h2>
            <ExportButton
              filename="Cursor会话工具"
              headers={cursorSessionToolTable(data).headers}
              rows={cursorSessionToolTable(data).rows}
            />
          </div>
          {data.top_tools.length > 0 ? (
            <ExportableChart
              option={toolOption}
              filename="cursor-session-tools"
              style={{ height: Math.max(220, data.top_tools.slice(0, 10).length * 36) }}
            />
          ) : (
            <p className="note">暂无工具调用记录。</p>
          )}
          {data.tool_groups.length > 0 ? (
            <>
              <div className="panel-head">
                <h3>工具分类</h3>
                <ExportButton
                  filename="Cursor会话工具分类"
                  headers={cursorSessionToolGroupTable(data).headers}
                  rows={cursorSessionToolGroupTable(data).rows}
                />
              </div>
              <ExportableChart
                option={toolGroupOption}
                filename="cursor-session-tool-groups"
                style={{ height: Math.max(180, data.tool_groups.length * 36) }}
              />
            </>
          ) : null}
        </section>
      </div>

      <section className="panel partition">
        <div className="panel-head">
          <h2>按项目</h2>
          <span className="muted">点击项目可筛选上方会话明细</span>
          <ExportButton
            filename="Cursor会话项目"
            headers={cursorSessionProjectTable(data).headers}
            rows={cursorSessionProjectTable(data).rows}
          />
        </div>
        {data.by_project.length > 0 ? (
          <>
            <ExportableChart
              option={projectOption}
              filename="cursor-session-projects"
              style={{ height: Math.max(220, data.by_project.slice(0, 8).length * 36) }}
            />
            <div className="table-scroll cursor-session-table-scroll">
              <table>
                <thead>
                  <tr>
                    <th>项目</th>
                    <th>会话数</th>
                    <th>轮次</th>
                    <th>失败</th>
                    <th>文件</th>
                    <th>最近活跃</th>
                  </tr>
                </thead>
                <tbody>
                  {data.by_project.map((row) => (
                    <tr
                      key={row.name}
                      className={selectedProject === row.name ? "clickable selected" : "clickable"}
                      onClick={() =>
                        setSelectedProject((current) => (current === row.name ? null : row.name))
                      }
                    >
                      <td title={row.name}>
                        <div className="cell-stack">
                          <span>{projectLabel(row.name)}</span>
                          <span className="muted">{row.name}</span>
                        </div>
                      </td>
                      <td>{formatTokens(row.session_count)}</td>
                      <td>{formatTokens(row.turn_count)}</td>
                      <td>{formatTokens(row.error_count)}</td>
                      <td>{formatTokens(row.files_touched)}</td>
                      <td title={row.last_seen_at ? formatClock(row.last_seen_at) : undefined}>
                        {row.last_seen_at ? relativeTime(row.last_seen_at) : "—"}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </>
        ) : (
          <p className="note">暂无项目分布数据。</p>
        )}
      </section>
    </LoadingOverlay>
  );
}
