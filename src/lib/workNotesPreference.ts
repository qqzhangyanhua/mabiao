import type { DetectedEngine } from "../types";

/** 本机 webview 偏好：上次用的纪要引擎、模型，以及设置页手动探测结果。不进备份。 */
export const WORK_NOTES_PREFERENCE_STORAGE_KEY = "mabiao:work-notes-preference";

export type WorkNotesPreference = {
  engineId: string;
  models: Record<string, string>;
  detected: DetectedEngine[];
};

export function defaultWorkNotesPreference(): WorkNotesPreference {
  return { engineId: "codex", models: {}, detected: [] };
}

export function workNotesEngineLabel(id: string): string {
  if (id === "codex") {
    return "Codex";
  }
  if (id === "claude") {
    return "Claude";
  }
  if (id === "grok") {
    return "Grok";
  }
  if (id === "cursor-agent") {
    return "Cursor Agent";
  }
  return id;
}

export function installedWorkNoteEngines(detected: DetectedEngine[]): DetectedEngine[] {
  return detected.filter((engine) => engine.installed);
}

export function resolveWorkNotesEngine(
  engineId: string,
  installed: DetectedEngine[],
): string | null {
  if (installed.some((engine) => engine.id === engineId)) {
    return engineId;
  }
  return installed[0]?.id ?? null;
}

export function engineSelectLabel(engine: DetectedEngine): string {
  const name = workNotesEngineLabel(engine.id);
  return engine.writes_session_dir ? `${name}（会写会话）` : name;
}

function parseDetectedEngine(value: unknown): DetectedEngine | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return null;
  }
  const record = value as Record<string, unknown>;
  if (typeof record.id !== "string" || record.id.trim() === "") {
    return null;
  }
  if (typeof record.program !== "string" || record.program.trim() === "") {
    return null;
  }
  if (typeof record.writes_session_dir !== "boolean") {
    return null;
  }
  if (typeof record.installed !== "boolean") {
    return null;
  }
  const version =
    record.version == null ? null : typeof record.version === "string" ? record.version : null;
  return {
    id: record.id,
    program: record.program,
    writes_session_dir: record.writes_session_dir,
    installed: record.installed,
    version,
  };
}

function parseModels(value: unknown): Record<string, string> {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return {};
  }
  const models: Record<string, string> = {};
  for (const [key, model] of Object.entries(value as Record<string, unknown>)) {
    if (typeof model === "string") {
      models[key] = model;
    }
  }
  return models;
}

export function parseWorkNotesPreference(raw: string | null): WorkNotesPreference {
  const defaults = defaultWorkNotesPreference();
  if (raw == null || raw === "") {
    return defaults;
  }
  try {
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
      return defaults;
    }
    const record = parsed as Record<string, unknown>;
    const engineId = typeof record.engineId === "string" ? record.engineId : defaults.engineId;
    const detected = Array.isArray(record.detected)
      ? record.detected.flatMap((item) => {
          const engine = parseDetectedEngine(item);
          return engine ? [engine] : [];
        })
      : [];
    return {
      engineId: engineId.trim() === "" ? defaults.engineId : engineId,
      models: parseModels(record.models),
      detected,
    };
  } catch {
    return defaults;
  }
}

export function serializeWorkNotesPreference(preference: WorkNotesPreference): string {
  return JSON.stringify({
    engineId: preference.engineId,
    models: preference.models,
    detected: preference.detected,
  });
}

export function loadWorkNotesPreference(): WorkNotesPreference {
  try {
    return parseWorkNotesPreference(localStorage.getItem(WORK_NOTES_PREFERENCE_STORAGE_KEY));
  } catch {
    return defaultWorkNotesPreference();
  }
}

export function saveWorkNotesPreference(preference: WorkNotesPreference): void {
  try {
    localStorage.setItem(
      WORK_NOTES_PREFERENCE_STORAGE_KEY,
      serializeWorkNotesPreference(preference),
    );
  } catch {
    /* quota / private mode */
  }
}
