import { invoke } from "@tauri-apps/api/core";
import { memo, useEffect, useState, type PointerEvent } from "react";
import { useTrayQuotaArrange } from "../hooks/useTrayQuotaArrange";
import { Icon } from "../icons";
import { formatClock, formatWindowClock } from "../lib/format";
import {
  OFFICIAL_QUOTA_FRESHNESS_STATUS,
  officialQuotaAgeLabel,
  officialQuotaAmountLabel,
  officialQuotaEmptyCopy,
  officialQuotaFreshnessTitle,
  officialQuotaRefreshHint,
  officialQuotaUndetectedNote,
} from "../lib/officialQuotaDisplay";
import { trayQuotaRowSummary } from "../lib/trayQuotaLayout";
import type { OfficialQuotaDto, OfficialQuotaFreshness, OfficialQuotaRow } from "../types";
import { EmptyState } from "./EmptyState";
import { SourceLabel } from "./SourceIcon";
import type { OfficialQuotaListProps } from "./type";
import { Button } from "./ui/Button";

const DEFAULT_STALE_AFTER_MINUTES = 10;
const AGE_TICK_MS = 30_000;

export const OfficialQuotaPanel = memo(function OfficialQuotaPanel({
  data,
  onQuota,
  onError,
}: {
  data: OfficialQuotaDto | null;
  onQuota: (value: OfficialQuotaDto) => void;
  onError: (error: unknown) => void;
}) {
  const rows = data?.rows ?? [];
  const undetectedNote = officialQuotaUndetectedNote(data?.undetected ?? []);
  const staleAfterMinutes = data?.stale_after_minutes ?? DEFAULT_STALE_AFTER_MINUTES;
  const [busyProvider, setBusyProvider] = useState<string | null>(null);

  useEffect(() => {
    if (staleAfterMinutes <= 0) {
      return;
    }
    const id = window.setInterval(() => {
      void invoke<OfficialQuotaDto>("refresh_official_quota")
        .then(onQuota)
        .catch(() => undefined);
    }, staleAfterMinutes * 60_000);
    return () => window.clearInterval(id);
  }, [onQuota, staleAfterMinutes]);

  async function refreshProvider(provider: string) {
    setBusyProvider(provider);
    try {
      onQuota(await invoke<OfficialQuotaDto>("refresh_official_quota_provider", { provider }));
    } catch (error) {
      onError(error);
    } finally {
      setBusyProvider(null);
    }
  }

  return (
    <article className="panel official-quota-panel">
      <div className="panel-head">
        <div className="official-quota-heading">
          <h2>官方额度</h2>
          <span className="muted">账号级订阅限额，与上方本机估计窗不是同一口径</span>
          {data ? (
            <span className="muted official-quota-refresh-hint">
              {officialQuotaRefreshHint(staleAfterMinutes)}
            </span>
          ) : null}
        </div>
      </div>
      {rows.length === 0 ? (
        <EmptyState compact icon="clock" {...officialQuotaEmptyCopy(data)} />
      ) : (
        <>
          <OfficialQuotaList
            rows={rows}
            staleAfterMinutes={staleAfterMinutes}
            busyProvider={busyProvider}
            onRefresh={(provider) => void refreshProvider(provider)}
          />
          {undetectedNote ? <p className="panel-note">{undetectedNote}</p> : null}
        </>
      )}
    </article>
  );
});

export function OfficialQuotaList({
  rows,
  staleAfterMinutes = DEFAULT_STALE_AFTER_MINUTES,
  compactReset = false,
  arrangeable = false,
  busyProvider,
  onRefresh,
  onArrange,
}: OfficialQuotaListProps) {
  const nowMs = useTickingNow();
  const arrange = useTrayQuotaArrange(rows, arrangeable, onArrange);
  return (
    <ul
      className={
        arrangeable
          ? arrange.dragging
            ? "official-quota-list is-arrangeable is-reordering"
            : "official-quota-list is-arrangeable"
          : "official-quota-list"
      }
    >
      {arrange.visible.map((row) => (
        <QuotaRow
          key={row.provider}
          row={row}
          staleAfterMinutes={staleAfterMinutes}
          compactReset={compactReset}
          nowMs={nowMs}
          busy={busyProvider === row.provider}
          disabled={busyProvider != null}
          onRefresh={onRefresh ? () => onRefresh(row.provider) : undefined}
          arrangeable={arrangeable}
          open={!arrangeable || !arrange.isCollapsed(row.provider)}
          dragging={arrange.dragging === row.provider}
          dropTarget={arrange.dropTarget === row.provider}
          onToggle={() => arrange.toggle(row.provider)}
          onPointerDown={(event) => arrange.beginDrag(row.provider, event.clientY)}
        />
      ))}
    </ul>
  );
}

export function QuotaFreshnessMark({
  freshness,
  capturedAt,
  staleAfterMinutes,
  nowMs,
}: {
  freshness: OfficialQuotaFreshness;
  capturedAt: string | null;
  staleAfterMinutes: number;
  nowMs: number;
}) {
  const age = officialQuotaAgeLabel(capturedAt, nowMs);
  return (
    <em
      className="official-quota-freshness"
      title={officialQuotaFreshnessTitle(freshness, capturedAt, staleAfterMinutes)}
    >
      <span>{OFFICIAL_QUOTA_FRESHNESS_STATUS[freshness]}</span>
      {age ? <span className="official-quota-age">{age}</span> : null}
    </em>
  );
}

function QuotaRow({
  row,
  staleAfterMinutes,
  compactReset,
  nowMs,
  busy,
  disabled,
  onRefresh,
  arrangeable,
  open,
  dragging,
  dropTarget,
  onToggle,
  onPointerDown,
}: {
  row: OfficialQuotaRow;
  staleAfterMinutes: number;
  compactReset: boolean;
  nowMs: number;
  busy: boolean;
  disabled: boolean;
  onRefresh?: () => void;
  arrangeable: boolean;
  open: boolean;
  dragging: boolean;
  dropTarget: boolean;
  onToggle: () => void;
  onPointerDown: (event: PointerEvent<HTMLElement>) => void;
}) {
  const tone = row.freshness === "official" ? "ok" : row.freshness === "stale" ? "warn" : "idle";
  const rowClass = [
    "official-quota-row",
    `tone-${tone}`,
    open ? "is-open" : "is-collapsed",
    dragging ? "is-dragging" : "",
    dropTarget ? "is-drop-target" : "",
  ]
    .filter(Boolean)
    .join(" ");
  return (
    <li className={rowClass} data-quota-provider={row.provider}>
      <div
        className="official-quota-toolbar"
        onPointerDown={
          arrangeable
            ? (event) => {
                if (event.button !== 0) {
                  return;
                }
                onPointerDown(event);
              }
            : undefined
        }
      >
        {arrangeable ? (
          <span className="official-quota-grip" title="拖动排序" aria-hidden="true" />
        ) : null}
        {arrangeable ? (
          // 折叠开关本来整行是一个 <button>，但这一行现在还塞了刷新小图标——
          // 两个 <button> 不能嵌套（浏览器会把外层标签提前截断，布局跟着乱）。
          // 换成 role="button" 的 div 自己接管键盘可达性，刷新按钮再单独挡住点击冒泡。
          <div
            role="button"
            tabIndex={0}
            className="official-quota-head official-quota-head-toggle"
            aria-expanded={open}
            onClick={onToggle}
            onKeyDown={(event) => {
              if (event.key === "Enter" || event.key === " ") {
                event.preventDefault();
                onToggle();
              }
            }}
          >
            <QuotaRowTitle row={row} busy={busy} disabled={disabled} onRefresh={onRefresh} />
            {open ? (
              <QuotaFreshnessMark
                freshness={row.freshness}
                capturedAt={row.captured_at}
                staleAfterMinutes={staleAfterMinutes}
                nowMs={nowMs}
              />
            ) : (
              <em className="official-quota-summary">{trayQuotaRowSummary(row)}</em>
            )}
            <Icon name="chevron" size={12} className="official-quota-caret" />
          </div>
        ) : (
          <div className="official-quota-head">
            <QuotaRowTitle row={row} busy={busy} disabled={disabled} onRefresh={onRefresh} />
            <QuotaFreshnessMark
              freshness={row.freshness}
              capturedAt={row.captured_at}
              staleAfterMinutes={staleAfterMinutes}
              nowMs={nowMs}
            />
          </div>
        )}
      </div>
      {open ? <QuotaRowBody row={row} compactReset={compactReset} /> : null}
    </li>
  );
}

function QuotaRowTitle({
  row,
  busy,
  disabled,
  onRefresh,
}: {
  row: OfficialQuotaRow;
  busy: boolean;
  disabled: boolean;
  onRefresh?: () => void;
}) {
  return (
    <div className="official-quota-title">
      <strong>
        <SourceLabel source={row.provider} fallback={row.application} />
      </strong>
      {onRefresh ? (
        <Button
          variant="icon"
          className={busy ? "official-quota-refresh is-busy" : "official-quota-refresh"}
          disabled={disabled}
          onClick={(event) => {
            // 折叠开关那个 role="button" 的 div 就包在外面，点刷新不拦截会连带把它也触发。
            event.stopPropagation();
            onRefresh();
          }}
          title={busy ? `${row.application} 刷新中` : `刷新 ${row.application} 额度`}
          aria-label={busy ? `${row.application} 刷新中` : `刷新 ${row.application} 额度`}
        >
          <Icon name="refresh" size={13} />
        </Button>
      ) : null}
    </div>
  );
}

function QuotaRowBody({ row, compactReset }: { row: OfficialQuotaRow; compactReset: boolean }) {
  if (row.windows.length === 0) {
    return <span className="muted">{row.error ?? "尚未捕获官方额度"}</span>;
  }
  return (
    <>
      <div className="official-quota-windows">
        {row.windows.map((window) => {
          const percent = window.used_percent;
          // 金额与百分比可以并存：有上限时进度条旁边补一行钱，
          // 只有余额时那一行就是这个窗口的全部内容。
          const amount = officialQuotaAmountLabel(window);
          return (
            <div className="official-quota-window" key={`${row.provider}-${window.kind}`}>
              <span title={window.label}>{window.label}</span>
              <strong>{percent == null ? (amount ?? "—") : `${percent.toFixed(0)}%`}</strong>
              {/* 没有百分比就不画条：一根空条读起来是「用了 0%」，而事实是「不知道上限」。 */}
              {percent == null ? null : (
                <div className="billing-bar" aria-hidden="true">
                  <i style={{ width: `${Math.min(100, Math.max(0, percent))}%` }} />
                </div>
              )}
              <span
                className="muted"
                title={window.resets_at ? formatClock(window.resets_at) : undefined}
              >
                {window.resets_at
                  ? `重置 ${compactReset ? formatWindowClock(window.resets_at) : formatClock(window.resets_at)}`
                  : "重置时间未知"}
              </span>
              {percent != null && amount ? (
                <span className="muted official-quota-amount">{amount}</span>
              ) : null}
            </div>
          );
        })}
      </div>
      {row.error ? <span className="muted">{row.error}</span> : null}
    </>
  );
}

export function useTickingNow(intervalMs = AGE_TICK_MS): number {
  const [nowMs, setNowMs] = useState(() => Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setNowMs(Date.now()), intervalMs);
    return () => window.clearInterval(id);
  }, [intervalMs]);
  return nowMs;
}
