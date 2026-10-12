import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type Dispatch,
  type SetStateAction,
} from "react";
import { composeStatus, isCatalogManagedStatus, isCatalogSyncing } from "../../lib/ingestStatus";
import type { ConversationCatalogProgressDto, IngestIssue } from "../../types";

const CATALOG_PROGRESS_EVENT = "conversation-catalog-progress";

type CatalogProgressConfig = {
  setSessionsRevision: Dispatch<SetStateAction<number>>;
  setStatus: Dispatch<SetStateAction<string>>;
  setConversationIssues: Dispatch<SetStateAction<IngestIssue[]>>;
};

export function useCatalogProgress({
  setSessionsRevision,
  setStatus,
  setConversationIssues,
}: CatalogProgressConfig) {
  const [progress, setProgress] = useState<ConversationCatalogProgressDto | null>(null);
  // 目录事件可能早于 `ingest` 命令返回到达，所以最新的目录状态和用量文案都放在 ref 里，
  // 谁后到谁按「用量文案 + 最新目录状态」重新拼，而不是各写各的。
  const latestRef = useRef<ConversationCatalogProgressDto | null>(null);
  const usageStatusRef = useRef("");
  const configRef = useRef({ setSessionsRevision, setStatus, setConversationIssues });
  useEffect(() => {
    configRef.current = { setSessionsRevision, setStatus, setConversationIssues };
  }, [setSessionsRevision, setStatus, setConversationIssues]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    const loadIssues = () => {
      invoke<IngestIssue[]>("get_conversation_catalog_issues")
        .then((issues) => {
          if (!disposed) {
            configRef.current.setConversationIssues(issues);
          }
        })
        .catch(() => {
          // 非 Tauri 环境没有命令通道，诊断保持空
        });
    };
    loadIssues();
    listen<ConversationCatalogProgressDto>(CATALOG_PROGRESS_EVENT, (event) => {
      const next = event.payload;
      const cfg = configRef.current;
      latestRef.current = next;
      setProgress(next);
      const usage = usageStatusRef.current;
      cfg.setStatus((current) =>
        isCatalogManagedStatus(current, usage) ? composeStatus(usage, next) : current,
      );
      if (next.finished) {
        cfg.setSessionsRevision((n) => n + 1);
        loadIssues();
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

  /** 新一轮用量阶段开始：上一轮已结束的目录结果（含问题数）不再属于这次的状态文案。 */
  const beginUsageStage = useCallback(() => {
    if (latestRef.current?.finished) {
      latestRef.current = null;
    }
  }, []);

  /** 用量阶段完成：记下用量文案，并叠上此刻已知的目录状态后显示。 */
  const publishUsageStatus = useCallback(
    (text: string) => {
      usageStatusRef.current = text;
      setStatus(composeStatus(text, latestRef.current));
    },
    [setStatus],
  );

  return {
    catalogSyncing: isCatalogSyncing(progress),
    beginUsageStage,
    publishUsageStatus,
  };
}
