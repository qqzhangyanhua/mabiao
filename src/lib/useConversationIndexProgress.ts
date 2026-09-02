import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import type { ConversationIndexProgressDto } from "../types";

const POLL_MS = 2000;

export function useConversationIndexProgress(
  revision: number,
): ConversationIndexProgressDto | null {
  const [progress, setProgress] = useState<ConversationIndexProgressDto | null>(null);

  useEffect(() => {
    let cancelled = false;
    let timer: number | undefined;

    async function loadProgress() {
      try {
        const next = await invoke<ConversationIndexProgressDto>("get_conversation_index_progress");
        if (cancelled) {
          return;
        }
        setProgress(next);
        if (next.total > 0 && next.indexed < next.total) {
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
