import { invoke } from "@tauri-apps/api/core";
import { useEffect, useMemo, useState } from "react";
import { CURSOR_ACCOUNT_AUTO_REFRESH_MINUTES } from "../hooks/usage/constants";
import type { ResolvedTheme } from "../hooks/useTheme";
import { areaTrendOption, donutOption, modelSlices } from "../lib/chartTheme";
import { formatClock, formatCompact, formatTokens, humanStatus } from "../lib/format";
import { cursorAccountDailyTable, cursorAccountModelTable } from "../lib/exportRows";
import type { CursorAccountUsageDto } from "../types";
import { CursorAccountEventTable } from "./CursorAccountEventTable";
import { DonutChart } from "./DonutChart";
import { EmptyState } from "./EmptyState";
import { ExportButton } from "./ExportButton";
import { ExportableChart } from "./ExportableChart";
import { KpiCard, LegendRow } from "./Kpi";
import { Button } from "./ui/Button";

function emptyUsage(): CursorAccountUsageDto {
  return {
    as_of: null,
    event_count: 0,
    input_tokens: 0,
    output_tokens: 0,
    cache_read_tokens: 0,
    cache_creation_tokens: 0,
    total_tokens: 0,
    daily: [],
    by_model: [],
    headless_tokens: 0,
    interactive_tokens: 0,
    headless_share: null,
  };
}

export function CursorAccountUsagePanel({
  theme,
  autoRefresh,
  revision,
  autoRefreshError = null,
  onRefresh,
}: {
  theme: ResolvedTheme;
  autoRefresh: boolean;
  revision: number;
  autoRefreshError?: string | null;
  onRefresh: () => Promise<void>;
}) {
  const [usage, setUsage] = useState<CursorAccountUsageDto | null>(null);
  const [hasToken, setHasToken] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    Promise.all([
      invoke<CursorAccountUsageDto>("get_cursor_account_usage"),
      invoke<boolean>("has_cursor_session_token"),
    ])
      .then(([next, configured]) => {
        if (!alive) {
          return;
        }
        setUsage(next);
        setHasToken(configured);
      })
      .catch((err: unknown) => {
        if (alive) {
          setError(humanStatus(err));
        }
      });
    return () => {
      alive = false;
    };
  }, [revision]);

  async function handleRefresh() {
    setBusy(true);
    setError(null);
    try {
      await onRefresh();
      setHasToken(true);
    } catch (err: unknown) {
      setError(humanStatus(err));
    } finally {
      setBusy(false);
    }
  }

  const panelError = error ?? autoRefreshError;
  const data = usage ?? emptyUsage();
  const asOf = formatClock(data.as_of);
  const showEmpty = data.event_count === 0 && data.total_tokens === 0;
  const trendOption = useMemo(() => areaTrendOption(data.daily, theme), [data.daily, theme]);
  const modelOption = useMemo(
    () => donutOption(modelSlices(data.by_model), theme),
    [data.by_model, theme],
  );
  const headlessOption = useMemo(
    () =>
      donutOption(
        [
          { name: "后台", value: data.headless_tokens, color: "#8b6cff" },
          { name: "交互", value: data.interactive_tokens, color: "#22d3ee" },
        ],
        theme,
      ),
    [data.headless_tokens, data.interactive_tokens, theme],
  );
  const headlessPct =
    data.headless_share == null ? "—" : `${(data.headless_share * 100).toFixed(1)}%`;

  return (
    <div className="stack">
      <section className="panel partition">
        <div className="panel-head">
          <div>
            <h2>Cursor 账号用量</h2>
            <p className="note">
              云端账号 / 含全部设备 / 全时段 / 仅 token 无费用 /{" "}
              {autoRefresh
                ? `每 ${CURSOR_ACCOUNT_AUTO_REFRESH_MINUTES} 分钟自动刷新`
                : "需手动刷新"}{" "}
              · 最后刷新于 {asOf} / 已缓存 {formatTokens(data.event_count)} 条
            </p>
          </div>
          <Button variant="accent" disabled={busy} onClick={() => void handleRefresh()}>
            {busy ? "刷新中…" : "刷新"}
          </Button>
        </div>
        {panelError ? (
          <p className="panel-note snapshot-error" role="alert">
            {panelError}
          </p>
        ) : null}
      </section>

      {showEmpty ? (
        <div className="panel partition">
          {hasToken ? (
            <EmptyState
              icon="cursor"
              title="尚未拉取 Cursor 账号用量"
              hint="已读到本机 Cursor 登录态。点刷新从云端拉取，或在设置里打开独立自动刷新；离线时会继续展示上次成功结果。该数据是账号级用量，不会并入本机 token 总量。"
            />
          ) : (
            <EmptyState
              icon="cursor"
              title="未找到 Cursor 登录态"
              hint="请确认本机装了 Cursor 客户端并已登录，登录态会被自动读取，无需手动配置。该数据是账号级用量，不会并入本机 token 总量。"
            />
          )}
        </div>
      ) : (
        <>
          <section className="kpi-row">
            <KpiCard
              icon="trend"
              tone="purple"
              label="总量"
              value={formatTokens(data.total_tokens)}
            />
            <KpiCard
              icon="sessions"
              tone="cyan"
              label="输入"
              value={formatTokens(data.input_tokens)}
            />
            <KpiCard
              icon="model"
              tone="orange"
              label="输出"
              value={formatTokens(data.output_tokens)}
            />
            <KpiCard
              icon="daily"
              tone="blue"
              label="缓存读 / 写"
              value={`${formatTokens(data.cache_read_tokens)} / ${formatTokens(data.cache_creation_tokens)}`}
            />
          </section>
          <section className="panel partition">
            <div className="panel-head">
              <h2>按天趋势</h2>
              <ExportButton
                filename="Cursor账号按天"
                headers={cursorAccountDailyTable(data).headers}
                rows={cursorAccountDailyTable(data).rows}
              />
            </div>
            <ExportableChart
              option={trendOption}
              filename="cursor-account-daily"
              style={{ height: 280 }}
            />
          </section>
          <div className="split-2">
            <section className="panel partition">
              <div className="panel-head">
                <h2>按模型</h2>
                <ExportButton
                  filename="Cursor账号按模型"
                  headers={cursorAccountModelTable(data).headers}
                  rows={cursorAccountModelTable(data).rows}
                />
              </div>
              <div className="donut-wrap">
                <DonutChart option={modelOption} centerValue={formatCompact(data.total_tokens)} />
                <div className="legend-col">
                  {modelSlices(data.by_model).map((item) => (
                    <LegendRow
                      key={item.name}
                      color={item.color}
                      label={item.name}
                      value={formatTokens(item.value)}
                      extra={
                        data.total_tokens > 0
                          ? `${((item.value / data.total_tokens) * 100).toFixed(1)}%`
                          : undefined
                      }
                    />
                  ))}
                </div>
              </div>
            </section>
            <section className="panel partition">
              <div className="panel-head">
                <h2>后台 / 交互</h2>
              </div>
              <div className="donut-wrap">
                <DonutChart option={headlessOption} centerValue={headlessPct} />
                <div className="legend-col">
                  <LegendRow
                    color="#8b6cff"
                    label="后台"
                    value={formatTokens(data.headless_tokens)}
                    extra={headlessPct}
                  />
                  <LegendRow
                    color="#22d3ee"
                    label="交互"
                    value={formatTokens(data.interactive_tokens)}
                    extra={
                      data.total_tokens > 0
                        ? `${((data.interactive_tokens / data.total_tokens) * 100).toFixed(1)}%`
                        : undefined
                    }
                  />
                </div>
              </div>
            </section>
          </div>
          <CursorAccountEventTable
            revision={data.as_of ?? data.event_count}
            eventCount={data.event_count}
            onError={(err) => setError(humanStatus(err))}
          />
        </>
      )}
    </div>
  );
}
