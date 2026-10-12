import { TOKEN_PARTS, splitSum, type TokenSplit } from "../lib/tokens";
import { formatPercent, formatTokens } from "../lib/format";

export function TokenComposition({ split }: { split: TokenSplit }) {
  const total = splitSum(split);
  if (total <= 0) return null;
  return (
    <div>
      <div
        className="flex h-2 overflow-hidden rounded-full bg-muted shadow-[inset_0_1px_2px_rgb(15_23_42/8%)]"
        role="img"
        aria-label="Token 口径占比"
      >
        {TOKEN_PARTS.map((part) => {
          const value = split[part.key];
          if (value <= 0) return null;
          return (
            <div
              key={part.key}
              title={`${part.label} ${formatTokens(value)}（${formatPercent(value / total)}）`}
              style={{ width: `${(value / total) * 100}%`, background: part.colorVar }}
            />
          );
        })}
      </div>
      <ul className="mt-2 flex flex-wrap gap-x-3.5 gap-y-1 text-xs text-muted-foreground">
        {TOKEN_PARTS.filter((part) => split[part.key] > 0).map((part) => (
          <li key={part.key} className="flex items-center gap-1.5">
            <span className="size-2 rounded-full" style={{ background: part.colorVar }} />
            {part.label} {formatTokens(split[part.key])}
          </li>
        ))}
      </ul>
    </div>
  );
}
