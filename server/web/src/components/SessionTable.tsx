import type { SessionListItem } from "../api/types";
import { formatDateTime } from "../lib/format";
import { useHref } from "../view";
import { SourceBadge } from "./SourceBadge";
import { Badge } from "./ui/badge";
import { numCellClass, tableClass, thClass, theadRowClass, trClass } from "./ui/table";

export function SessionTable({ sessions }: { sessions: SessionListItem[] }) {
  const href = useHref();
  if (sessions.length === 0) {
    return <p className="py-6 text-center text-sm text-muted-foreground">这段时间没有推送过会话</p>;
  }
  return (
    <div className="overflow-x-auto">
      <table className={tableClass}>
        <caption className="sr-only">会话目录</caption>
        <thead>
          <tr className={theadRowClass}>
            <th scope="col" className={`py-1 ${thClass}`}>
              标题
            </th>
            <th scope="col" className={`py-1 ${thClass}`}>
              来源
            </th>
            <th scope="col" className={`py-1 ${thClass}`}>
              项目
            </th>
            <th scope="col" className={`py-1 ${thClass}`}>
              模型
            </th>
            <th scope="col" className={`py-1 ${thClass}`}>
              设备
            </th>
            <th scope="col" className={`py-1 ${thClass}`}>
              结束时间
            </th>
            <th scope="col" className={`py-1 ${numCellClass} ${thClass}`}>
              事件
            </th>
          </tr>
        </thead>
        <tbody>
          {sessions.map((s) => (
            <tr key={s.id} className={trClass}>
              <td className="max-w-72 truncate py-1.5 pr-2" title={s.title}>
                <a
                  href={href({ page: "session", sessionId: s.id })}
                  className="text-primary hover:underline"
                >
                  {s.title || "（无标题）"}
                </a>
                {s.generated_by_work_notes && (
                  <Badge variant="secondary" className="ml-2">
                    码表生成
                  </Badge>
                )}
              </td>
              <td className="py-1.5 pr-2">
                <SourceBadge source={s.source} />
              </td>
              <td className="max-w-40 truncate py-1.5 pr-2" title={s.project}>
                {s.project_name ?? s.project}
              </td>
              <td className="max-w-40 truncate py-1.5 pr-2" title={s.model}>
                {s.model}
              </td>
              <td className="py-1.5 pr-2">{s.device_name}</td>
              <td className="whitespace-nowrap py-1.5 pr-2 tabular-nums">{formatDateTime(s.ended_at)}</td>
              <td className={`py-1.5 ${numCellClass}`}>{s.event_count}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
