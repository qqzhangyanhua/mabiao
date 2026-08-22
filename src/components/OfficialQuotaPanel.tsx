import { invoke } from "@tauri-apps/api/core";
import { memo, useState } from "react";
import { Icon } from "../icons";
import { formatClock } from "../lib/format";
import { quotaPace } from "../lib/quotaPace";
import type { OfficialQuotaDto, OfficialQuotaFreshness, OfficialQuotaRow } from "../types";
import { EmptyState } from "./EmptyState";
import { SourceLabel } from "./SourceIcon";
import type { OfficialQuotaListProps } from "./type";
import { Button } from "./ui/Button";

const FRESHNESS_LABEL: Record<OfficialQuotaFreshness, string> = {
  official: "官方",
  stale: "已过期",
  unavailable: "暂无",
};

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
  const [busyProvider, setBusyProvider] = useState<string | null>(null);

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
        <h2>官方额度</h2>
        <span className="muted">
          账号级订阅限额，与上方本机估计窗不是同一口径
          {data ? ` · ${data.stale_after_minutes} 分钟后标为过期` : ""}
        </span>
      </div>
      {rows.length === 0 ? (
        <EmptyState
          compact
          icon="clock"
          title={data ? "所选账号均已隐藏" : "正在读取官方额度…"}
          hint={
            data ? "在「配置显示」里打开要看的账号" : "先显示上次缓存，再后台刷新"
          }
        />
      ) : (
        <OfficialQuotaList
          rows={rows}
          busyProvider={busyProvider}
          onRefresh={(provider) => void refreshProvider(provider)}
        />
      )}
    </article>
  );
});

export function OfficialQuotaList({ rows, busyProvider, onRefresh }: OfficialQuotaListProps) {
  return (
    <ul className="official-quota-list">
      {rows.map((row) => (
        <QuotaRow
          key={row.provider}
          row={row}
          busy={busyProvider === row.provider}
          disabled={busyProvider != null}
          onRefresh={onRefresh ? () => onRefresh(row.provider) : undefined}
        />
      ))}
    </ul>
  );
}

function QuotaRow({
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
  const tone = row.freshness === "official" ? "ok" : row.freshness === "stale" ? "warn" : "idle";
  const capturedAt = row.captured_at ? Date.parse(row.captured_at) : Number.NaN;
  return (
    <li className={`official-quota-row tone-${tone}`}>
      <div className="official-quota-head">
        <div className="official-quota-title">
          <strong>
            <SourceLabel source={row.provider} fallback={row.application} />
          </strong>
          {onRefresh ? (
            <Button
              variant="icon"
              className={busy ? "official-quota-refresh is-busy" : "official-quota-refresh"}
              disabled={disabled}
              onClick={onRefresh}
              title={busy ? `${row.application} 刷新中` : `刷新 ${row.application} 额度`}
              aria-label={busy ? `${row.application} 刷新中` : `刷新 ${row.application} 额度`}
            >
              <Icon name="refresh" size={13} />
            </Button>
          ) : null}
        </div>
        <em>{FRESHNESS_LABEL[row.freshness]}</em>
      </div>
      {row.windows.length === 0 ? (
        <span className="muted">{row.error ?? "尚未捕获官方额度"}</span>
      ) : (
        <div className="official-quota-windows">
          {row.windows.map((window) => {
            const percent = window.used_percent;
            // 只看百分比看不出好坏，配上「窗口已过多少」才知道是不是烧超了。
            // 基准取抓取时刻而不是当下：百分比本来就是那一刻的快照，两边要对齐。
            const pace = quotaPace(window.kind, percent, window.resets_at, capturedAt);
            return (
              <div className="official-quota-window" key={`${row.provider}-${window.kind}`}>
                <span>{window.label}</span>
                <strong>{percent == null ? "—" : `${percent.toFixed(0)}%`}</strong>
                <div className="billing-bar" aria-hidden="true">
                  <i style={{ width: `${Math.min(100, Math.max(0, percent ?? 0))}%` }} />
                </div>
                <span className="muted">
                  {window.resets_at ? `重置 ${formatClock(window.resets_at)}` : "重置时间未知"}
                  {pace ? <em className={`quota-pace tone-${pace.tone}`}>{pace.label}</em> : null}
                </span>
              </div>
            );
          })}
        </div>
      )}
      {row.error && row.windows.length > 0 ? <span className="muted">{row.error}</span> : null}
    </li>
  );
}
