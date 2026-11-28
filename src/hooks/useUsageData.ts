import { useCallback, useEffect, useState, useSyncExternalStore } from "react";
import { clearCursorSessionDetailCache } from "../lib/cursorSessionDetailCache";
import { conversationFocusFromSession } from "../lib/sessionEntryCopy";
import {
  hashForTab,
  REMOTE_SERVER_ANCHOR,
  SETTINGS_UNPRICED_ANCHOR,
} from "../lib/settingsTabs";
import {
  hashBelongsToView,
  hashForConversation,
  parseConversationFocus,
  replaceLocationHash,
  viewFromHash,
  worktimeHashForDay,
} from "../lib/viewHash";
import type { Filter, Grain, View } from "../types";
import { useAutoRefresh } from "./usage/useAutoRefresh";
import { useCatalogProgress } from "./usage/useCatalogProgress";
import { useCursorAccountAutoRefresh } from "./usage/useCursorAccountAutoRefresh";
import { tauriUsageBackend, type UsageBackend } from "./usage/usageBackend";
import { UsageViewController } from "./usage/usageViewController";

export { viewFromHash, views } from "../lib/viewHash";

export type UseUsageDataOptions = {
  backend?: UsageBackend;
  initialPreset?: string;
};

export function useUsageData(options: UseUsageDataOptions = {}) {
  const backend = options.backend ?? tauriUsageBackend;

  const [controller] = useState(
    () =>
      new UsageViewController({
        backend,
        initialView: viewFromHash(),
        initialPreset: options.initialPreset,
        initialConversationFocus: parseConversationFocus(window.location.hash),
        onClearSessionDetailCache: clearCursorSessionDetailCache,
      }),
  );

  const snapshot = useSyncExternalStore(controller.subscribe, controller.getSnapshot);

  const loadCatalogIssues = useCallback(() => backend.getConversationCatalogIssues(), [backend]);

  const { catalogSyncing, beginUsageStage, publishUsageStatus } = useCatalogProgress({
    bumpSessionsRevision: controller.bumpSessionsRevision,
    setStatus: controller.setStatus,
    setConversationIssues: controller.setConversationIssues,
    loadCatalogIssues,
  });

  useEffect(() => {
    controller.setCatalogStatusHandlers({ beginUsageStage, publishUsageStatus });
  }, [controller, beginUsageStage, publishUsageStatus]);

  const { autoRefresh, setAutoRefresh } = useAutoRefresh(
    controller.runIngest,
    controller.reportError,
  );
  const { autoRefresh: cursorAccountAutoRefresh, setAutoRefresh: setCursorAccountAutoRefresh } =
    useCursorAccountAutoRefresh(controller.refreshCursorAccount);

  useEffect(() => {
    void controller.connectAndStart();
  }, [controller]);

  const navigate = useCallback(
    (next: View) => {
      if (!hashBelongsToView(window.location.hash, next)) {
        replaceLocationHash(next);
      }
      void controller.navigate(next).catch(controller.reportError);
    },
    [controller],
  );

  const setGrain = useCallback(
    (next: Grain) => {
      void controller.setGrain(next).catch(controller.reportError);
    },
    [controller],
  );

  const applyPreset = useCallback(
    (next: string, explicitRange?: { from: string | null; to: string | null }) => {
      void controller.applyPreset(next, explicitRange).catch(controller.reportError);
    },
    [controller],
  );

  const drillRange = useCallback(
    (from: string, to: string) => {
      void controller.drillRange(from, to).catch(controller.reportError);
    },
    [controller],
  );

  const popRange = useCallback(() => {
    void controller.popRange().catch(controller.reportError);
  }, [controller]);

  const applyFilter = useCallback(
    (next: Filter) => {
      void controller.applyFilter(next).catch(controller.reportError);
    },
    [controller],
  );

  const openConversations = useCallback(
    (session?: { id: string; source: string }) => {
      const focus = conversationFocusFromSession(session);
      controller.setConversationFocus(focus);
      replaceLocationHash(
        focus ? hashForConversation(focus.source, focus.session_id) : "conversations",
      );
      void controller.navigate("conversations").catch(controller.reportError);
    },
    [controller],
  );

  const openWorktime = useCallback(
    (day: string) => {
      const hash = worktimeHashForDay(day);
      if (!hash) {
        return;
      }
      replaceLocationHash(hash);
      void controller.navigate("worktime").catch(controller.reportError);
    },
    [controller],
  );

  const openUnpricedDiagnosis = useCallback(() => {
    replaceLocationHash(SETTINGS_UNPRICED_ANCHOR);
    void controller.navigate("settings").catch(controller.reportError);
  }, [controller]);

  const openRemoteServerSettings = useCallback(() => {
    replaceLocationHash(REMOTE_SERVER_ANCHOR);
    void controller.navigate("settings").catch(controller.reportError);
  }, [controller]);

  const openScanPathSettings = useCallback(() => {
    replaceLocationHash(hashForTab("sources"));
    void controller.navigate("settings").catch(controller.reportError);
  }, [controller]);

  return {
    ...snapshot,
    setOfficialQuota: controller.setOfficialQuota,
    setPrices: controller.setPrices,
    setStatus: controller.setStatus,
    savePrices: controller.savePrices,
    invalidatePricing: controller.invalidatePricing,
    saveBudget: controller.saveBudget,
    reloadAfterRestore: controller.reloadAfterRestore,
    refreshCursorAccount: controller.refreshCursorAccount,
    clearCursorAccountUsage: controller.clearCursorAccountUsage,
    cursorAccountAutoRefresh,
    setCursorAccountAutoRefresh,
    setGrain,
    catalogSyncing,
    autoRefresh,
    setAutoRefresh,
    navigate,
    applyPreset,
    drillRange,
    popRange,
    applyFilter,
    openConversations,
    openRemoteServerSettings,
    openWorktime,
    openUnpricedDiagnosis,
    openScanPathSettings,
    clearConversationFocus: controller.clearConversationFocus,
    runIngest: controller.runIngest,
    runRebuild: controller.runRebuild,
    runPurgeArchived: controller.runPurgeArchived,
    reportError: controller.reportError,
  };
}
