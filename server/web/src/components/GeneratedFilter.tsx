export type GeneratedFilter = "all" | "only" | "hide";

const OPTIONS: { id: GeneratedFilter; label: string }[] = [
  { id: "all", label: "全部会话" },
  { id: "hide", label: "排除码表生成" },
  { id: "only", label: "只看码表生成" },
];

/** 传给 `Api.sessions` 的 `generatedByWorkNotes`：不过滤时为 `undefined`。 */
export function generatedParam(filter: GeneratedFilter): boolean | undefined {
  return filter === "all" ? undefined : filter === "only";
}

/** 会话列表的「码表生成」标记过滤。 */
export function GeneratedFilterSelect({
  value,
  onChange,
}: {
  value: GeneratedFilter;
  onChange: (value: GeneratedFilter) => void;
}) {
  return (
    <select
      aria-label="码表生成过滤"
      value={value}
      onChange={(e) => onChange(e.target.value as GeneratedFilter)}
      className="rounded border border-slate-300 bg-white px-2 py-1 text-sm"
    >
      {OPTIONS.map((o) => (
        <option key={o.id} value={o.id}>
          {o.label}
        </option>
      ))}
    </select>
  );
}
