import { invoke } from "@tauri-apps/api/core";
import { useEffect, useMemo, useState } from "react";
import { useTheme } from "../hooks/useTheme";
import {
  groupQuotaHistory,
  quotaHistoryOption,
  quotaHistoryProviders,
} from "../lib/quotaHistoryChart";
import type { OfficialQuotaHistoryDto } from "../types";
import { ExportableChart } from "./ExportableChart";
import { EmptyState } from "./EmptyState";

export function OfficialQuotaHistoryChart({
  providers,
  revision,
}: {
  providers: string[];
  revision: string;
}) {
  const { theme } = useTheme();
  const [history, setHistory] = useState<OfficialQuotaHistoryDto | null>(null);
  const [selectedOverride, setSelectedOverride] = useState<string>("");

  useEffect(() => {
    void invoke<OfficialQuotaHistoryDto>("get_official_quota_history")
      .then(setHistory)
      .catch(() => undefined);
  }, [revision]);

  const available = useMemo(() => {
    const fromHistory = history ? quotaHistoryProviders(history.points) : [];
    return fromHistory.length > 0 ? fromHistory : providers;
  }, [history, providers]);
  const selected = available.includes(selectedOverride) ? selectedOverride : (available[0] ?? "");

  const series = useMemo(
    () => (history && selected ? groupQuotaHistory(history.points, selected) : []),
    [history, selected],
  );
  const canChart = series.some((item) => item.points.length >= 2);
  const retention = history?.retention_days ?? 45;

  return (
    <section className="official-quota-history">
      <div className="official-quota-history-head">
        <div>
          <h3>官方额度历史</h3>
          <p className="muted">
            连续官方额度快照，保留 {retention}{" "}
            天。与上方本机 5 小时 / 7 天估计窗不是同一口径。
          </p>
        </div>
        {available.length > 1 ? (
          <label className="official-quota-history-provider">
            <span>账号</span>
            <select
              value={selected}
              onChange={(event) => setSelectedOverride(event.target.value)}
            >
              {available.map((id) => (
                <option key={id} value={id}>
                  {id}
                </option>
              ))}
            </select>
          </label>
        ) : null}
      </div>
      {canChart ? (
        <ExportableChart
          option={quotaHistoryOption(series, theme)}
          filename={`official-quota-history-${selected || "all"}`}
          style={{ height: 220 }}
        />
      ) : (
        <EmptyState
          compact
          icon="clock"
          title="还没有足够的官方快照"
          hint="同一账号至少两次不同捕获时刻后才会画趋势。刷新官方额度即可累积。"
        />
      )}
    </section>
  );
}
