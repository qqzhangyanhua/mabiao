import {
  REPORT_POSTER_STYLES,
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
  return (
    <div className="report-dialog-styles" role="radiogroup" aria-label="周报风格">
      {REPORT_POSTER_STYLES.map((style) => {
        const active = style.id === selectedStyleId;
        return (
          <button
            key={style.id}
            type="button"
            role="radio"
            aria-checked={active}
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
            <span className="report-dialog-style-label">{style.label}</span>
          </button>
        );
      })}
    </div>
  );
}
