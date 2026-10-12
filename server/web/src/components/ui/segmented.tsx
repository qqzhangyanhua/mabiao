import { cn } from "../../lib/cn";

export function Segmented<T extends string>({
  label,
  value,
  onChange,
  options,
  size = "default",
}: {
  label: string;
  value: T;
  onChange: (value: T) => void;
  options: { id: T; label: string }[];
  size?: "default" | "compact";
}) {
  const compact = size === "compact";
  return (
    <div
      className={cn(
        "inline-flex flex-wrap rounded-xl bg-muted/80",
        compact ? "gap-0.5 p-0.5" : "gap-1 p-1",
      )}
      role="radiogroup"
      aria-label={label}
    >
      {options.map((option) => {
        const active = option.id === value;
        return (
          <button
            key={option.id}
            type="button"
            role="radio"
            aria-checked={active}
            onClick={() => onChange(option.id)}
            className={cn(
              "rounded-lg focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
              compact ? "px-2.5 py-1 text-xs" : "px-3.5 py-1 text-sm",
              active
                ? "bg-brand font-medium text-primary-foreground shadow-[0_4px_12px_-6px_var(--primary)]"
                : "font-normal text-muted-foreground hover:text-foreground",
            )}
          >
            {option.label}
          </button>
        );
      })}
    </div>
  );
}
