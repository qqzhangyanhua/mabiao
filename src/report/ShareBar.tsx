import type { PosterSourceSlice } from "./posterTypes";

export function ShareBar({ sources }: { sources: PosterSourceSlice[] }) {
  return (
    <div>
      <div className="rp-share" role="img" aria-label="来源占比">
        {sources.map((source) => (
          <span
            key={source.label}
            className="rp-share-seg"
            style={{ width: `${source.pct}%`, background: source.color }}
          />
        ))}
      </div>
      <ul className="rp-share-legend">
        {sources.map((source) => (
          <li key={source.label}>
            <i className="rp-swatch" style={{ background: source.color }} />
            {source.label} {source.pct}%
          </li>
        ))}
      </ul>
    </div>
  );
}
