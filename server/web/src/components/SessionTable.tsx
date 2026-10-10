import type { SessionListItem } from "../api/types";
import { formatDateTime } from "../lib/format";

export function SessionTable({ sessions }: { sessions: SessionListItem[] }) {
  if (sessions.length === 0) {
    return <p className="py-6 text-center text-sm text-slate-500">这段时间没有推送过会话</p>;
  }
  return (
    <div className="overflow-x-auto">
      <table className="w-full text-sm">
        <thead>
          <tr className="border-b border-slate-200 text-left text-xs text-slate-500">
            <th className="py-1 font-normal">标题</th>
            <th className="py-1 font-normal">来源</th>
            <th className="py-1 font-normal">项目</th>
            <th className="py-1 font-normal">模型</th>
            <th className="py-1 font-normal">设备</th>
            <th className="py-1 font-normal">结束时间</th>
            <th className="py-1 text-right font-normal">事件</th>
          </tr>
        </thead>
        <tbody>
          {sessions.map((s) => (
            <tr key={s.id} className="border-b border-slate-100 last:border-0">
              <td className="max-w-72 truncate py-1.5 pr-2" title={s.title}>
                {s.title || "（无标题）"}
                {s.generated_by_work_notes && (
                  <span className="ml-2 rounded bg-slate-100 px-1 text-xs text-slate-500">
                    码表生成
                  </span>
                )}
              </td>
              <td className="py-1.5 pr-2">{s.source}</td>
              <td className="max-w-40 truncate py-1.5 pr-2" title={s.project}>
                {s.project_name ?? s.project}
              </td>
              <td className="max-w-40 truncate py-1.5 pr-2" title={s.model}>
                {s.model}
              </td>
              <td className="py-1.5 pr-2">{s.device_name}</td>
              <td className="whitespace-nowrap py-1.5 pr-2">{formatDateTime(s.ended_at)}</td>
              <td className="py-1.5 text-right tabular-nums">{s.event_count}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
