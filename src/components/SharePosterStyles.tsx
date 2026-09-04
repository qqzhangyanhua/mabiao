import {
  REPORT_POSTER_STYLES,
  resolveReportPosterStyle,
  type ReportPosterStyleId,
} from "../report/posterStyleRegistry";

export function SharePosterStyles({
  selectedStyleId,
  disabled,
  onSelect,
}: {
  selectedStyleId: ReportPosterStyleId;
  disabled: boolean;
  onSelect: (styleId: ReportPosterStyleId) => void;
}) {
  const selectedLabel = resolveReportPosterStyle(selectedStyleId).label;
  return (
    <div className="report-dialog-styles">
      <div className="report-dialog-styles-head">
        <span className="report-dialog-styles-kicker">风格</span>
        <span className="report-dialog-styles-current">{selectedLabel}</span>
      </div>
      <div className="report-dialog-style-grid" role="radiogroup" aria-label="周报风格">
        {REPORT_POSTER_STYLES.map((style) => {
          const active = style.id === selectedStyleId;
          return (
            <button
              key={style.id}
              type="button"
              role="radio"
              aria-checked={active}
              aria-label={style.label}
              title={style.label}
              className={active ? "report-dialog-style is-active" : "report-dialog-style"}
              disabled={disabled}
              onClick={() => onSelect(style.id)}
            >
              <span
                className="report-dialog-style-swatch"
                aria-hidden="true"
                style={{
                  background: style.swatch.background,
                  boxShadow: `inset 0 0 0 2px ${style.swatch.accent}`,
                }}
              />
            </button>
          );
        })}
      </div>
    </div>
  );
}
