import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";
import type {
  ConversationDetailDto,
  ConversationDetailStateDto,
  ConversationSessionRow,
} from "../types";
import type { ConversationExportFormat } from "../components/type";
import { conversationKey } from "./conversationCache";
import {
  createConversationRequestGate,
  isConversationResponseCurrent,
  nextConversationRevisionPollState,
} from "./conversationFollow";
import { shouldRequestConversationDetail } from "./conversationNavigation";
import { humanStatus } from "./format";
import type { ConversationTimelineFollow } from "./useConversationTimelineFollow";

type ConversationDetailRequestIntent = {
  session: ConversationSessionRow;
  key: string;
  generation: number;
  followUpdates: boolean;
};

export function useConversationDetailLoader({
  selected,
  selectedKey,
  onError,
  follow,
}: {
  selected: ConversationSessionRow | null;
  selectedKey: string | null;
  onError?: (error: unknown) => void;
  follow: ConversationTimelineFollow;
}) {
  const [details, setDetails] = useState<Record<string, ConversationDetailDto>>({});
  const [detailLoadingByKey, setDetailLoadingByKey] = useState<Record<string, boolean>>({});
  const [detailErrorsByKey, setDetailErrorsByKey] = useState<Record<string, string>>({});
  const [fileAvailableByKey, setFileAvailableByKey] = useState<Record<string, boolean>>({});
  const [pollErrorsByKey, setPollErrorsByKey] = useState<Record<string, string>>({});
  const [exportFormat, setExportFormat] = useState<ConversationExportFormat | null>(null);
  const [exportStatus, setExportStatus] = useState<string | null>(null);
  const [exportError, setExportError] = useState(false);
  const detailGenerations = useRef(new Map<string, number>());
  const detailRequestGates = useRef(
    new Map<string, ReturnType<typeof createConversationRequestGate<ConversationDetailRequestIntent>>>(),
  );
  const mountedRef = useRef(true);
  const selectedKeyRef = useRef<string | null>(selectedKey);
  const detailsRef = useRef<Record<string, ConversationDetailDto>>({});
  const observedDetailRevisions = useRef(new Map<string, string>());
  const followRef = useRef(follow);
  useEffect(() => {
    selectedKeyRef.current = selectedKey;
  }, [selectedKey]);
  useEffect(() => {
    followRef.current = follow;
  }, [follow]);

  const getDetailRequestGate = useCallback((key: string) => {
    let gate = detailRequestGates.current.get(key);
    if (!gate) {
      gate = createConversationRequestGate<ConversationDetailRequestIntent>();
      detailRequestGates.current.set(key, gate);
    }
    return gate;
  }, []);

  const isDetailResponseCurrent = useCallback(
    (key: string, generation: number) =>
      isConversationResponseCurrent({
        mounted: mountedRef.current,
        generation,
        currentGeneration: detailGenerations.current.get(key) ?? 0,
      }),
    [],
  );

  const replaceDetail = useCallback(
    (key: string, result: ConversationDetailDto, followUpdates: boolean) => {
      if (selectedKeyRef.current === key) {
        if (followUpdates) {
          followRef.current.applyFollowedReplace(
            detailsRef.current[key]?.event_count ?? 0,
            result.event_count,
          );
        } else {
          followRef.current.pinToLatest();
        }
      }
      detailsRef.current = { ...detailsRef.current, [key]: result };
      setDetails(detailsRef.current);
      observedDetailRevisions.current.set(key, result.revision);
      setFileAvailableByKey((current) => ({ ...current, [key]: result.session.file_available }));
      setDetailErrorsByKey((current) => {
        const next = { ...current };
        delete next[key];
        return next;
      });
      setPollErrorsByKey((current) => {
        const next = { ...current };
        delete next[key];
        return next;
      });
    },
    [],
  );

  const performDetailRequest = useCallback(
    async ({ session, key, generation, followUpdates }: ConversationDetailRequestIntent) => {
      try {
        const result = await invoke<ConversationDetailDto>("get_conversation_detail", {
          source: session.source,
          sessionId: session.session_id,
        });
        if (isDetailResponseCurrent(key, generation)) {
          replaceDetail(key, result, followUpdates);
        }
      } catch (error) {
        if (isDetailResponseCurrent(key, generation)) {
          setDetailErrorsByKey((current) => ({ ...current, [key]: humanStatus(error) }));
          onError?.(error);
        }
      } finally {
        if (isDetailResponseCurrent(key, generation)) {
          setDetailLoadingByKey((current) => ({ ...current, [key]: false }));
        }
      }
    },
    [isDetailResponseCurrent, onError, replaceDetail],
  );

  const drainDetailRequests = useCallback(
    async (initialIntent: ConversationDetailRequestIntent) => {
      let intent: ConversationDetailRequestIntent | null = initialIntent;
      while (intent) {
        await performDetailRequest(intent);
        intent = getDetailRequestGate(initialIntent.key).release();
      }
    },
    [getDetailRequestGate, performDetailRequest],
  );

  useEffect(() => {
    const requestGates = detailRequestGates.current;
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      followRef.current.cancelJumps();
      for (const gate of requestGates.values()) {
        gate.clearPending();
      }
    };
  }, []);

  const fetchDetail = useCallback(
    (session: ConversationSessionRow, followUpdates = false) => {
      const shouldRequest = shouldRequestConversationDetail(session);
      const key = conversationKey(session);
      const gate = getDetailRequestGate(key);
      const acquired = !shouldRequest || gate.acquire();
      const generation = (detailGenerations.current.get(key) ?? 0) + 1;
      detailGenerations.current.set(key, generation);
      setFileAvailableByKey((current) => ({
        ...current,
        [key]: session.file_available,
      }));
      setDetailErrorsByKey((current) => {
        const next = { ...current };
        delete next[key];
        return next;
      });
      setPollErrorsByKey((current) => {
        const next = { ...current };
        delete next[key];
        return next;
      });

      if (!shouldRequest) {
        gate.clearPending();
        setDetailLoadingByKey((current) => ({ ...current, [key]: false }));
        return;
      }
      setDetailLoadingByKey((current) => ({ ...current, [key]: true }));
      const intent = { session, key, generation, followUpdates };
      if (acquired) {
        void drainDetailRequests(intent);
      } else {
        gate.queueLatest(intent);
      }
    },
    [drainDetailRequests, getDetailRequestGate],
  );

  const selectedSource = selected?.source ?? null;
  const selectedSessionId = selected?.session_id ?? null;

  useEffect(() => {
    if (!selectedSource || !selectedSessionId || !selectedKey) {
      return;
    }

    let cancelled = false;
    const poll = async () => {
      const gate = getDetailRequestGate(selectedKey);
      if (!gate.acquire()) {
        return;
      }
      const generation = detailGenerations.current.get(selectedKey) ?? 0;
      try {
        const state = await invoke<ConversationDetailStateDto>("get_conversation_detail_state", {
          source: selectedSource,
          sessionId: selectedSessionId,
          knownRevision: observedDetailRevisions.current.get(selectedKey) ?? "",
        });
        if (cancelled || !isDetailResponseCurrent(selectedKey, generation)) {
          return;
        }

        const revisionPollState = nextConversationRevisionPollState({
          revision: state.revision,
          changed: state.changed,
          fileAvailable: state.file_available,
        });
        observedDetailRevisions.current.set(selectedKey, revisionPollState.knownRevision);
        setFileAvailableByKey((current) => ({
          ...current,
          [selectedKey]: state.file_available,
        }));
        setPollErrorsByKey((current) => {
          const next = { ...current };
          delete next[selectedKey];
          return next;
        });
        if (revisionPollState.shouldReload) {
          const result = await invoke<ConversationDetailDto>("get_conversation_detail", {
            source: selectedSource,
            sessionId: selectedSessionId,
          });
          if (!cancelled && isDetailResponseCurrent(selectedKey, generation)) {
            replaceDetail(selectedKey, result, true);
          }
        }
      } catch (error) {
        if (!cancelled && isDetailResponseCurrent(selectedKey, generation)) {
          setPollErrorsByKey((current) => ({ ...current, [selectedKey]: humanStatus(error) }));
        }
      } finally {
        const pendingIntent = gate.release();
        if (pendingIntent) {
          void drainDetailRequests(pendingIntent);
        }
      }
    };

    const timer = window.setInterval(poll, 2_000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [
    drainDetailRequests,
    getDetailRequestGate,
    isDetailResponseCurrent,
    replaceDetail,
    selectedKey,
    selectedSessionId,
    selectedSource,
  ]);

  const reset = useCallback(() => {
    detailGenerations.current.clear();
    observedDetailRevisions.current.clear();
    for (const gate of detailRequestGates.current.values()) {
      gate.clearPending();
    }
    detailRequestGates.current.clear();
    detailsRef.current = {};
    setDetails({});
    setDetailLoadingByKey({});
    setDetailErrorsByKey({});
    setFileAvailableByKey({});
    setPollErrorsByKey({});
    setExportFormat(null);
    setExportStatus(null);
    setExportError(false);
  }, []);

  const clearExport = useCallback(() => {
    setExportFormat(null);
    setExportStatus(null);
    setExportError(false);
  }, []);

  async function exportConversation(format: ConversationExportFormat) {
    if (!selected) {
      return;
    }
    setExportFormat(format);
    setExportStatus(null);
    setExportError(false);
    try {
      const saved = await invoke<boolean>("export_conversation", {
        source: selected.source,
        sessionId: selected.session_id,
        format,
      });
      setExportStatus(saved ? "已导出" : "已取消");
    } catch (error) {
      setExportError(true);
      setExportStatus(humanStatus(error));
    } finally {
      setExportFormat(null);
    }
  }

  const detail = selectedKey ? (details[selectedKey] ?? null) : null;
  const detailLoading = selectedKey ? Boolean(detailLoadingByKey[selectedKey]) : false;
  const detailError = selectedKey ? (detailErrorsByKey[selectedKey] ?? null) : null;
  const detailFileAvailable = selectedKey
    ? (fileAvailableByKey[selectedKey] ?? selected?.file_available ?? true)
    : true;
  const pollError = selectedKey ? (pollErrorsByKey[selectedKey] ?? null) : null;

  return {
    detail,
    detailLoading,
    detailError,
    detailFileAvailable,
    pollError,
    exportFormat,
    exportStatus,
    exportError,
    fetchDetail,
    reset,
    clearExport,
    exportConversation,
  };
}
