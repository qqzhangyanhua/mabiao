import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";
import type {
  ConversationEvent,
  ConversationEventAnchor,
  ConversationEventContentDto,
  ConversationEventPage,
} from "../types";
import { conversationKey } from "./conversationCache";
import { humanStatus } from "./format";
import {
  advanceConversationEventWindow,
  aroundPageAnchor,
  CONVERSATION_EVENT_PAGE_SIZE,
  emptyConversationEventWindow,
  firstPageAnchor,
  latestPageAnchor,
  nextEarlierAnchor,
  nextLaterAnchor,
  type ConversationEventPageMode,
  type ConversationEventWindow,
} from "./conversationWindow";

export function useConversationEventPages({
  source,
  sessionId,
  revision,
  followLatest = false,
  initialSequence = null,
}: {
  source: string;
  sessionId: string;
  revision: string;
  followLatest?: boolean;
  initialSequence?: number | null;
}) {
  const sessionKey = conversationKey({ source, session_id: sessionId });
  const [loadedFor, setLoadedFor] = useState<string | null>(null);
  const [eventWindow, setEventWindow] = useState<ConversationEventWindow<ConversationEvent>>(
    emptyConversationEventWindow,
  );
  const [loadingEarlier, setLoadingEarlier] = useState(false);
  const [loadingLater, setLoadingLater] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const generation = useRef(0);
  const seenRevision = useRef(revision);
  const skippedRevision = useRef<string | null>(null);

  const replaceWindow = useCallback(
    (anchor: ConversationEventAnchor) => {
      const request = ++generation.current;
      const expectedSession = sessionKey;
      void invoke<ConversationEventPage>("get_conversation_events", {
        source,
        sessionId,
        anchor,
        limit: CONVERSATION_EVENT_PAGE_SIZE,
      })
        .then((page) => {
          if (generation.current !== request) {
            return;
          }
          setEventWindow((current) => advanceConversationEventWindow(current, page, "replace"));
          setLoadedFor(expectedSession);
          setError(null);
        })
        .catch((caught: unknown) => {
          if (generation.current !== request) {
            return;
          }
          setEventWindow(emptyConversationEventWindow());
          setError(humanStatus(caught));
          setLoadedFor(expectedSession);
        });
    },
    [sessionId, sessionKey, source],
  );

  const replaceWithLatest = useCallback(() => {
    replaceWindow(latestPageAnchor());
  }, [replaceWindow]);

  const loadInitial = useCallback(() => {
    replaceWindow(
      initialSequence == null ? latestPageAnchor() : aroundPageAnchor(initialSequence),
    );
  }, [initialSequence, replaceWindow]);

  useEffect(() => {
    seenRevision.current = revision;
    skippedRevision.current = null;
    loadInitial();
    // 换会话才重新锚定；revision 是否跟随由下面的 effect 判定。
    // eslint-disable-next-line react-hooks/exhaustive-deps -- session identity only
  }, [loadInitial, sessionKey]);

  useEffect(() => {
    if (seenRevision.current !== revision) {
      seenRevision.current = revision;
      if (followLatest) {
        skippedRevision.current = null;
        replaceWithLatest();
      } else {
        skippedRevision.current = revision;
      }
      return;
    }
    if (followLatest && skippedRevision.current !== null) {
      skippedRevision.current = null;
      replaceWithLatest();
    }
  }, [followLatest, replaceWithLatest, revision]);

  const visibleWindow =
    loadedFor === sessionKey ? eventWindow : emptyConversationEventWindow<ConversationEvent>();
  const loading = loadedFor !== sessionKey;
  const visibleError = loadedFor === sessionKey ? error : null;

  const requestPage = useCallback(
    async (anchor: ConversationEventAnchor, mode: ConversationEventPageMode) => {
      const request = generation.current;
      const page = await invoke<ConversationEventPage>("get_conversation_events", {
        source,
        sessionId,
        anchor,
        limit: CONVERSATION_EVENT_PAGE_SIZE,
      });
      if (generation.current !== request) {
        return false;
      }
      setEventWindow((current) => advanceConversationEventWindow(current, page, mode));
      setError(null);
      return true;
    },
    [sessionId, source],
  );

  const loadEarlier = useCallback(async () => {
    const anchor = nextEarlierAnchor(visibleWindow);
    if (!anchor || loadingEarlier || loadingLater) {
      return false;
    }
    setLoadingEarlier(true);
    try {
      return await requestPage(anchor, "prepend");
    } catch (caught) {
      setError(humanStatus(caught));
      return false;
    } finally {
      setLoadingEarlier(false);
    }
  }, [loadingEarlier, loadingLater, requestPage, visibleWindow]);

  const loadLater = useCallback(async () => {
    const anchor = nextLaterAnchor(visibleWindow);
    if (!anchor || loadingEarlier || loadingLater) {
      return false;
    }
    setLoadingLater(true);
    try {
      return await requestPage(anchor, "append");
    } catch (caught) {
      setError(humanStatus(caught));
      return false;
    } finally {
      setLoadingLater(false);
    }
  }, [loadingEarlier, loadingLater, requestPage, visibleWindow]);

  const jumpToFirst = useCallback(async () => {
    if (!visibleWindow.hasMoreBefore) {
      return false;
    }
    return requestPage(firstPageAnchor(), "replace");
  }, [requestPage, visibleWindow.hasMoreBefore]);

  const jumpToLast = useCallback(async () => {
    seenRevision.current = revision;
    skippedRevision.current = null;
    return requestPage(latestPageAnchor(), "replace");
  }, [requestPage, revision]);

  const applyEventContent = useCallback((content: ConversationEventContentDto) => {
    setEventWindow((current) => ({
      ...current,
      events: current.events.map((event) =>
        event.event_id === content.event_id
          ? {
              ...event,
              text: content.text,
              details: content.details,
              content_status: "complete",
            }
          : event,
      ),
    }));
  }, []);

  return {
    eventWindow: visibleWindow,
    loading,
    loadingEarlier,
    loadingLater,
    error: visibleError,
    loadEarlier,
    loadLater,
    jumpToFirst,
    jumpToLast,
    applyEventContent,
  };
}
