export interface TrendPoint {
  label: string;
  value: number;
}

/** 纯 CSS 柱状图，天数再多也只是柱子变细；每根柱子带 title 当悬停提示。 */
export function TrendChart({
  title,
  points,
  format,
}: {
  title: string;
  points: TrendPoint[];
  format: (value: number) => string;
}) {
  const max = Math.max(0, ...points.map((p) => p.value));
  return (
    <section className="rounded border border-slate-200 bg-white p-4">
      <h3 className="mb-3 text-sm font-medium text-slate-700">{title}</h3>
      {points.length === 0 ? (
        <p className="py-6 text-center text-sm text-slate-500">这段时间没有数据</p>
      ) : (
        <>
          <div role="img" aria-label={title} className="flex h-32 items-end gap-px">
            {points.map((p) => (
              <div
                key={p.label}
                title={`${p.label}：${format(p.value)}`}
                className="min-w-px flex-1 rounded-t-sm bg-blue-500 hover:bg-blue-700"
                style={{
                  height: `${max > 0 ? Math.max((p.value / max) * 100, p.value > 0 ? 2 : 0) : 0}%`,
                }}
              />
            ))}
          </div>
          <div className="mt-1 flex justify-between text-xs text-slate-500">
            <span>{points[0]?.label}</span>
            <span>峰值 {format(max)}</span>
            <span>{points[points.length - 1]?.label}</span>
          </div>
        </>
      )}
    </section>
  );
}
