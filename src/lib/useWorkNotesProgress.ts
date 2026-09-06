import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import type { WorkNotesProgressDto } from "../types";

const POLL_MS = 400;

export function useWorkNotesProgress(revision: number): WorkNotesProgressDto | null {
  const [progress, setProgress] = useState<WorkNotesProgressDto | null>(null);

  useEffect(() => {
    let cancelled = false;
    let timer: number | undefined;

    async function loadProgress() {
      try {
        const next = await invoke<WorkNotesProgressDto>("get_work_notes_progress");
        if (cancelled) {
          return;
        }
        setProgress(next);
        if (next.status === "running") {
          timer = window.setTimeout(() => {
            void loadProgress();
          }, POLL_MS);
        }
      } catch {
        if (!cancelled) {
          setProgress(null);
        }
      }
    }

    void loadProgress();
    return () => {
      cancelled = true;
      if (timer !== undefined) {
        window.clearTimeout(timer);
      }
    };
  }, [revision]);

  return progress;
}
