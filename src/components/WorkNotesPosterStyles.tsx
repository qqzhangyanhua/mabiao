import {
  WORK_NOTES_POSTER_STYLES,
  resolveWorkNotesPosterStyle,
  type WorkNotesPosterStyleId,
} from "../workNotes/posterStyleRegistry";

export function WorkNotesPosterStyles({
  selectedStyleId,
  disabled,
  onSelect,
}: {
  selectedStyleId: WorkNotesPosterStyleId;
  disabled: boolean;
  onSelect: (styleId: WorkNotesPosterStyleId) => void;
}) {
  const selectedLabel = resolveWorkNotesPosterStyle(selectedStyleId).label;
  return (
    <div className="work-notes-styles">
      <div className="work-notes-styles-head">
        <span className="work-notes-styles-kicker">风格</span>
        <span className="work-notes-styles-current">{selectedLabel}</span>
      </div>
      <div className="work-notes-style-grid" role="radiogroup" aria-label="纪要海报风格">
        {WORK_NOTES_POSTER_STYLES.map((style) => {
          const active = style.id === selectedStyleId;
          return (
            <button
              key={style.id}
              type="button"
              role="radio"
              aria-checked={active}
              aria-label={style.label}
              title={style.label}
              className={active ? "work-notes-style is-active" : "work-notes-style"}
              disabled={disabled}
              onClick={() => onSelect(style.id)}
            >
              <span
                className="work-notes-style-swatch"
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
