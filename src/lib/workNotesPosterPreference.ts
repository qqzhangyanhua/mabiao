import {
  DEFAULT_WORK_NOTES_POSTER_STYLE_ID,
  resolveWorkNotesPosterStyleId,
  type WorkNotesPosterStyleId,
} from "../workNotes/posterStyleRegistry";

/** 本机 webview 偏好：工作纪要海报风格。不进备份，也不写入报告分享偏好。 */
export const WORK_NOTES_POSTER_PREFERENCE_KEY = "mabiao:work-notes-poster-preference";

export type WorkNotesPosterPreference = {
  posterStyleId: WorkNotesPosterStyleId;
};

export function defaultWorkNotesPosterPreference(): WorkNotesPosterPreference {
  return { posterStyleId: DEFAULT_WORK_NOTES_POSTER_STYLE_ID };
}

export function parseWorkNotesPosterPreference(raw: string | null): WorkNotesPosterPreference {
  const defaults = defaultWorkNotesPosterPreference();
  if (raw == null || raw === "") {
    return defaults;
  }
  try {
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
      return defaults;
    }
    const record = parsed as Record<string, unknown>;
    return { posterStyleId: resolveWorkNotesPosterStyleId(record.posterStyleId) };
  } catch {
    return defaults;
  }
}

export function serializeWorkNotesPosterPreference(preference: WorkNotesPosterPreference): string {
  return JSON.stringify({ posterStyleId: preference.posterStyleId });
}

export function loadWorkNotesPosterPreference(): WorkNotesPosterPreference {
  try {
    return parseWorkNotesPosterPreference(localStorage.getItem(WORK_NOTES_POSTER_PREFERENCE_KEY));
  } catch {
    return defaultWorkNotesPosterPreference();
  }
}

export function saveWorkNotesPosterPreference(preference: WorkNotesPosterPreference): void {
  try {
    localStorage.setItem(
      WORK_NOTES_POSTER_PREFERENCE_KEY,
      serializeWorkNotesPosterPreference(preference),
    );
  } catch {
    /* quota / private mode */
  }
}
