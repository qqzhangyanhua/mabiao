import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";
import type {
  ConversationEvent,
  ConversationEventContentDto,
  ConversationEventPage,
} from "../types";
import { humanStatus } from "./format";
import {
  applyConversationEventPage,
  CONVERSATION_EVENT_PAGE_SIZE,
  emptyConversationEventWindow,
  latestPageAnchor,
  nextEarlierAnchor,
  type ConversationEventWindow,
} from "./conversationWindow";

export function useConversationEventPages({
  source,
  sessionId,
  revision,
}: {
  source: string;
  sessionId: string;
  revision: string;
}) {
  const identity = `${source}\u{1f}${sessionId}\u{1f}${revision}`;
  const [loadedFor, setLoadedFor] = useState<string | null>(null);
  const [eventWindow, setEventWindow] = useState<ConversationEventWindow<ConversationEvent>>(
    emptyConversationEventWindow,
  );
  const [loadingEarlier, setLoadingEarlier] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const generation = useRef(0);

  useEffect(() => {
    const request = ++generation.current;
    void invoke<ConversationEventPage>("get_conversation_events", {
      source,
      sessionId,
      anchor: latestPageAnchor(),
      limit: CONVERSATION_EVENT_PAGE_SIZE,
    })
      .then((page) => {
        if (generation.current !== request) {
          return;
        }
        setEventWindow((current) => applyConversationEventPage(current, page, "replace"));
        setLoadedFor(identity);
        setError(null);
      })
      .catch((caught: unknown) => {
        if (generation.current !== request) {
          return;
        }
        setEventWindow(emptyConversationEventWindow());
        setError(humanStatus(caught));
        setLoadedFor(identity);
      });
  }, [identity, sessionId, source]);

  const visibleWindow =
    loadedFor === identity ? eventWindow : emptyConversationEventWindow<ConversationEvent>();
  const loading = loadedFor !== identity;
  const visibleError = loadedFor === identity ? error : null;

  const loadEarlier = useCallback(async () => {
    const anchor = nextEarlierAnchor(visibleWindow);
    if (!anchor || loadingEarlier) {
      return false;
    }
    const request = generation.current;
    setLoadingEarlier(true);
    try {
      const page = await invoke<ConversationEventPage>("get_conversation_events", {
        source,
        sessionId,
        anchor,
        limit: CONVERSATION_EVENT_PAGE_SIZE,
      });
      if (generation.current !== request) {
        return false;
      }
      setEventWindow((current) => applyConversationEventPage(current, page, "prepend"));
      return true;
    } catch (caught) {
      if (generation.current === request) {
        setError(humanStatus(caught));
      }
      return false;
    } finally {
      if (generation.current === request) {
        setLoadingEarlier(false);
      }
    }
  }, [loadingEarlier, sessionId, source, visibleWindow]);

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
    error: visibleError,
    loadEarlier,
    applyEventContent,
  };
}
