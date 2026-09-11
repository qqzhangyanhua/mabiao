import type { WorkNotesSessionChoice, WorkNotesSessionRef } from "../types";

export function workNotesSessionKey(session: {
  source: string;
  session_id: string;
}): string {
  return `${session.source}\0${session.session_id}`;
}

export function selectedSessionRefs(
  sessions: WorkNotesSessionChoice[],
  keys: ReadonlySet<string>,
): WorkNotesSessionRef[] {
  return sessions
    .filter((session) => keys.has(workNotesSessionKey(session)))
    .map((session) => ({
      source: session.source,
      session_id: session.session_id,
    }));
}

export function allSessionKeys(sessions: WorkNotesSessionChoice[]): Set<string> {
  return new Set(sessions.map(workNotesSessionKey));
}
