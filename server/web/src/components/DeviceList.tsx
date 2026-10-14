import { useMemo } from "react";
import type { DeviceView } from "../api/types";
import type { AsyncState } from "../hooks";
import { formatDateTime } from "../lib/format";
import { cn } from "../lib/cn";
import { ErrorNote, Loading } from "./Feedback";
import { panelClass } from "./ui/card";
import { tableClass, thClass, theadRowClass, trClass } from "./ui/table";

export function DeviceList({ devices }: { devices: AsyncState<DeviceView[]> }) {
  const rows = useMemo(
    () =>
      [...(devices.data ?? [])].sort(
        (left, right) => Date.parse(right.last_seen_at) - Date.parse(left.last_seen_at),
      ),
    [devices.data],
  );
  return (
    <section className={cn(panelClass, "p-4")}>
      <h2 className="mb-3 text-sm font-medium">设备</h2>
      {devices.error ? (
        <ErrorNote error={devices.error} onRetry={devices.reload} />
      ) : devices.loading || !devices.data ? (
        <Loading />
      ) : rows.length === 0 ? (
        <p className="py-4 text-center text-sm text-muted-foreground">还没有推送过的设备</p>
      ) : (
        <div className="overflow-x-auto">
          <table className={tableClass}>
            <caption className="sr-only">设备</caption>
            <thead>
              <tr className={theadRowClass}>
                <th scope="col" className={`px-3 py-2 ${thClass}`}>设备名</th>
                <th scope="col" className={`px-3 py-2 ${thClass}`}>首次出现</th>
                <th scope="col" className={`px-3 py-2 ${thClass}`}>最后出现</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((device) => (
                <tr key={device.device_id} className={trClass}>
                  <th scope="row" className="px-3 py-2 text-left font-normal">{device.device_name}</th>
                  <td className="px-3 py-2 tabular-nums">{formatDateTime(device.first_seen_at)}</td>
                  <td className="px-3 py-2 tabular-nums">{formatDateTime(device.last_seen_at)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}
