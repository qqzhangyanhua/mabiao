import { listen } from "@tauri-apps/api/event";
import {
  useEffect,
  useRef,
  useState,
  type Dispatch,
  type MutableRefObject,
  type SetStateAction,
} from "react";
import { catalogStatus, isCatalogSyncing } from "../../lib/ingestStatus";
import type { ConversationCatalogProgressDto } from "../../types";

export const CATALOG_PROGRESS_EVENT = "conversation-catalog-progress";

type CatalogProgressConfig = {
  setSessionsRevision: Dispatch<SetStateAction<number>>;
  setStatus: Dispatch<SetStateAction<string>>;
  usageStatusRef: MutableRefObject<string>;
};

export function useCatalogProgress({
  setSessionsRevision,
  setStatus,
  usageStatusRef,
}: CatalogProgressConfig) {
  const [progress, setProgress] = useState<ConversationCatalogProgressDto | null>(null);
  const configRef = useRef({ setSessionsRevision, setStatus, usageStatusRef });
  useEffect(() => {
    configRef.current = { setSessionsRevision, setStatus, usageStatusRef };
  }, [setSessionsRevision, setStatus, usageStatusRef]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    listen<ConversationCatalogProgressDto>(CATALOG_PROGRESS_EVENT, (event) => {
      const next = event.payload;
      const cfg = configRef.current;
      setProgress(next);
      cfg.setStatus(catalogStatus(next, cfg.usageStatusRef.current));
      if (next.finished) {
        cfg.setSessionsRevision((n) => n + 1);
      }
    })
      .then((fn) => {
        if (disposed) {
          fn();
        } else {
          unlisten = fn;
        }
      })
      .catch(() => {
        // 非 Tauri 环境没有事件通道，目录同步状态保持空闲
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  return { catalogProgress: progress, catalogSyncing: isCatalogSyncing(progress) };
}
