import { BRAND_MARKS, UNKNOWN_BRAND_MARK, type BrandMark } from "../generated/brandMarks";
import { resolveSourceIconId } from "../lib/sourceIcon";

function markOf(source: string): BrandMark {
  const iconId = resolveSourceIconId(source);
  return iconId === "unknown" ? UNKNOWN_BRAND_MARK : BRAND_MARKS[iconId];
}

export function sourceMarkPlate(source: string): boolean {
  const mark = markOf(source);
  return mark.kind === "image" && mark.plate === true;
}

export function SourceGlyph({ source, size }: { source: string; size: number }) {
  const mark = markOf(source);
  const markSize = mark.kind === "image" ? size : Math.max(10, Math.round(size * 0.72));
  if (mark.kind === "image") {
    return <img src={mark.src} alt="" width={markSize} height={markSize} draggable={false} />;
  }
  return (
    <svg
      viewBox={mark.viewBox}
      width={markSize}
      height={markSize}
      fill="currentColor"
      preserveAspectRatio="xMidYMid meet"
      aria-hidden
    >
      {mark.paths.map((path, index) => (
        <path
          key={index}
          d={path.d}
          fill={path.fill}
          fillRule={path.fillRule}
          clipRule={path.clipRule}
          opacity={path.opacity}
          stroke={path.stroke}
          strokeWidth={path.strokeWidth}
          strokeLinecap={path.strokeLinecap}
          strokeLinejoin={path.strokeLinejoin}
        />
      ))}
    </svg>
  );
}
