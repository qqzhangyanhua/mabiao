import { sourceTone } from "../icons";
import { sourceLabel } from "../lib/format";
import { resolveSourceIconId } from "../lib/sourceIcon";
import type { SourceIconProps, SourceLabelProps } from "./type";
import { SourceGlyph, sourceMarkPlate } from "./sourceGlyph";

export function SourceIcon({ source, size = 16 }: SourceIconProps) {
  const iconId = resolveSourceIconId(source);
  const tone = sourceTone[source] ?? sourceTone[iconId] ?? "tone-other";
  const plate = sourceMarkPlate(source);
  return (
    <span
      className={`source-ico ${tone}${plate ? " plate" : ""}`}
      title={sourceLabel(source)}
      aria-hidden
      style={{ width: size, height: size }}
    >
      <SourceGlyph source={source} size={size} />
    </span>
  );
}

export function SourceLabel({ source, fallback, size = 16 }: SourceLabelProps) {
  const text = fallback ?? sourceLabel(source);
  return (
    <span className="source-label">
      <SourceIcon source={source} size={size} />
      <span className="source-label-text">{text || "（未知）"}</span>
    </span>
  );
}
