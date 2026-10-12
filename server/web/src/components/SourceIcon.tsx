import { SourceGlyph, sourceMarkPlate } from "../../../../src/components/sourceGlyph";
import { sourceTone } from "../lib/chart";

export function SourceIcon({ source, size = 16 }: { source: string; size?: number }) {
  const tone = sourceTone(source);
  const plate = sourceMarkPlate(source);
  return (
    <span
      className="inline-grid shrink-0 place-items-center overflow-hidden rounded-[5px] [&_img]:size-full [&_img]:object-contain"
      style={{
        width: size,
        height: size,
        color: tone,
        background: plate ? "transparent" : `color-mix(in srgb, ${tone} 16%, transparent)`,
      }}
      aria-hidden
    >
      <SourceGlyph source={source} size={size} />
    </span>
  );
}
