import type { WorkNotesPosterRenderProps } from "./posterStyleRegistry";
import "./cinnabarSlipPoster.css";

function CinnabarSeal() {
  return (
    <svg className="cs-seal" viewBox="0 0 72 72" aria-hidden="true">
      <rect className="cs-seal-sq" x="6" y="6" width="60" height="60" />
      <text className="cs-seal-mark" x="36" y="46" textAnchor="middle">
        纪
      </text>
    </svg>
  );
}

/** `cinnabar-slip`：朱砂边、印章装饰、条目随内容撑高。 */
export function CinnabarSlipPoster({
  data,
  posterRef,
  posterId = "work-notes-poster",
}: WorkNotesPosterRenderProps) {
  return (
    <article ref={posterRef} id={posterId} className="cs-poster">
      <span className="cs-spine" aria-hidden="true" />
      <CinnabarSeal />
      <p className="cs-kicker">{data.kicker}</p>
      <p className="cs-range">{data.rangeLabel}</p>
      {data.headline ? <h1 className="cs-headline">{data.headline}</h1> : null}
      <ul className="cs-metrics">
        {data.metrics.map((metric) => (
          <li key={metric.id}>
            <span className="cs-metric-value">{metric.value}</span>
            <span className="cs-metric-label">{metric.label}</span>
          </li>
        ))}
      </ul>
      <ol className="cs-entries">
        {data.entries.map((entry, index) => (
          <li key={`${entry.title}-${index}`} className="cs-entry">
            <h2 className="cs-entry-title">{entry.title}</h2>
            <p className="cs-entry-detail">{entry.detail}</p>
            {entry.project ? <p className="cs-entry-project">{entry.project}</p> : null}
          </li>
        ))}
      </ol>
      {data.closing ? <p className="cs-closing">{data.closing}</p> : null}
      {data.skippedLabel ? <p className="cs-skipped">{data.skippedLabel}</p> : null}
    </article>
  );
}
