import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  conversationFocusToRestore,
  hashForConversation,
  replaceLocationHash,
} from "../hooks/viewCache";
import { conversationKey } from "../lib/conversationCache";
import {
  currentConversationFrame,
  initialConversationNavigationState,
  transitionConversationNavigation,
} from "../lib/conversationNavigation";
import { consumeEscape, consumeRefreshShortcut } from "../lib/escapeShortcut";
import { useConversationCatalog } from "../lib/useConversationCatalog";
import { useConversationDetailLoader } from "../lib/useConversationDetailLoader";
import { useConversationTimelineFollow } from "../lib/useConversationTimelineFollow";
import type {
  ConversationAgentLink,
  ConversationDetailDto,
  ConversationFocus,
  ConversationSessionRow,
  Filter,
} from "../types";
import { ConversationCatalog } from "./ConversationCatalog";
import { ConversationDetailView } from "./ConversationDetailView";

export function Conversations({
  filter,
  revision,
  focus,
  onFocusConsumed,
  onError,
}: {
  filter: Filter;
  revision: number;
  focus?: ConversationFocus | null;
  onFocusConsumed?: () => void;
  onError?: (error: unknown) => void;
}) {
  const {
    searchInput,
    setSearchInput,
    search,
    setSearch,
    toolNames,
    setToolNames,
    toolFailed,
    setToolFailed,
    toolNameOptions,
    page,
    setPage,
    pageData,
    loading: catalogLoading,
    error: catalogError,
    indexProgress,
  } = useConversationCatalog({ filter, revision, onError });
  const [navigation, setNavigation] = useState(initialConversationNavigationState);
  const [matchFocus, setMatchFocus] = useState<{
    eventId: string;
    sequence: number;
    snippet: string | null;
    query: string;
  } | null>(null);
  const currentFrame = currentConversationFrame(navigation);
  const selected = currentFrame?.session ?? null;
  const selectedKey = selected ? conversationKey(selected) : null;
  const detailTab = currentFrame?.tab ?? "events";
  const follow = useConversationTimelineFollow();
  const loader = useConversationDetailLoader({
    selected,
    selectedKey,
    onError,
    follow,
  });
  const { fetchDetail, clearExport, reset: resetDetail, exportConversation } = loader;
  const { prepareOpen, prepareClose, prepareBack, prepareEnterChild, rememberEventsScroll } =
    follow;
  const usageIdentity =
    selected && loader.detail
      ? `${selected.source}:${selected.session_id}:${loader.detail.revision}:${revision}`
      : "";

  useEffect(() => {
    if (!navigation.focus_relationship_id) return;
    const relationshipId = navigation.focus_relationship_id;
    const frame = window.requestAnimationFrame(() => {
      const target = [...document.querySelectorAll<HTMLElement>("[data-relationship-id]")].find(
        (element) => element.dataset.relationshipId === relationshipId,
      );
      target?.focus();
      setNavigation((current) =>
        transitionConversationNavigation(current, { type: "focus_restored" }),
      );
    });
    return () => window.cancelAnimationFrame(frame);
  }, [navigation.focus_relationship_id]);

  const selectedSource = selected?.source ?? null;
  const selectedSessionId = selected?.session_id ?? null;

  useEffect(() => {
    if (!selectedSource || !selectedSessionId) {
      return;
    }
    replaceLocationHash(hashForConversation(selectedSource, selectedSessionId));
  }, [selectedSource, selectedSessionId]);

  const loadDetail = useCallback(
    (session: ConversationSessionRow) => {
      const eventId = session.match_event_id;
      const sequence = session.match_sequence;
      const bodyHit = session.match_field === "body" && Boolean(eventId) && sequence != null;
      setMatchFocus(
        bodyHit && eventId && sequence != null
          ? {
              eventId,
              sequence,
              snippet: session.match_snippet ?? null,
              query: search,
            }
          : null,
      );
      setNavigation((current) =>
        transitionConversationNavigation(current, { type: "open_root", session }),
      );
      clearExport();
      prepareOpen(Boolean(bodyHit));
      fetchDetail(session);
    },
    [search, clearExport, fetchDetail, prepareOpen],
  );

  useEffect(() => {
    const restore = conversationFocusToRestore(focus ?? null, window.location.hash);
    if (!restore) {
      return;
    }
    const source = restore.source;
    const sessionId = restore.session_id;
    if (focus) {
      onFocusConsumed?.();
    }
    invoke<ConversationDetailDto>("get_conversation_detail", {
      source,
      sessionId,
    })
      .then((detail) => {
        loadDetail(detail.session);
      })
      .catch((error) => {
        setSearchInput(sessionId);
        setSearch(sessionId);
        onError?.(error);
      });
  }, [focus, loadDetail, onError, onFocusConsumed, setSearch, setSearchInput]);

  function closeDetail() {
    replaceLocationHash("conversations");
    setNavigation((current) => transitionConversationNavigation(current, { type: "close" }));
    resetDetail();
    prepareClose();
    setMatchFocus(null);
  }

  function backToParent() {
    const scrollTop = navigation.frames.at(-2)?.scroll_top ?? 0;
    setNavigation((current) => transitionConversationNavigation(current, { type: "back" }));
    prepareBack(scrollTop);
  }

  const onDetailEscapeRef = useRef(closeDetail);
  useEffect(() => {
    onDetailEscapeRef.current = navigation.frames.length > 1 ? backToParent : closeDetail;
  });

  useEffect(() => {
    if (!selectedKey) {
      return;
    }
    function onKeyDown(event: KeyboardEvent) {
      if (event.metaKey || event.ctrlKey || event.altKey || event.defaultPrevented) {
        return;
      }
      if (consumeRefreshShortcut(event)) {
        return;
      }
      if (!consumeEscape(event)) {
        return;
      }
      onDetailEscapeRef.current();
    }
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [selectedKey]);

  function setDetailTab(tab: typeof detailTab) {
    setNavigation((current) => transitionConversationNavigation(current, { type: "set_tab", tab }));
  }

  function toggleChild(link: ConversationAgentLink) {
    setNavigation((current) =>
      transitionConversationNavigation(current, {
        type: "toggle_child",
        relationship_id: link.relationship_id,
      }),
    );
  }

  function openChild(link: ConversationAgentLink) {
    if (!link.session) return;
    const parentScrollTop = follow.parentScrollTop();
    setNavigation((current) =>
      transitionConversationNavigation(current, {
        type: "enter_child",
        session: link.session!,
        relationship_id: link.relationship_id,
        parent_scroll_top: parentScrollTop,
      }),
    );
    prepareEnterChild();
    setMatchFocus(null);
    fetchDetail(link.session);
  }

  if (selected) {
    const session = loader.detail?.session ?? selected;
    return (
      <ConversationDetailView
        session={session}
        detail={loader.detail}
        detailTab={detailTab}
        detailLoading={loader.detailLoading}
        detailError={loader.detailError}
        detailFileAvailable={loader.detailFileAvailable}
        pollError={loader.pollError}
        breadcrumb={
          navigation.frames.length > 1
            ? navigation.frames.map((frame) => frame.session.title).join(" / ")
            : null
        }
        parentAvailable={navigation.frames.length > 1}
        expandedRelationshipIds={currentFrame?.expanded_relationship_ids ?? []}
        matchFocus={matchFocus}
        usageIdentity={usageIdentity}
        exportFormat={loader.exportFormat}
        exportStatus={loader.exportStatus}
        exportError={loader.exportError}
        follow={follow}
        onBack={navigation.frames.length > 1 ? backToParent : closeDetail}
        onExport={(format) => void exportConversation(format)}
        onTabChange={(tab) => {
          rememberEventsScroll(detailTab, tab);
          setDetailTab(tab);
        }}
        onRetry={() => fetchDetail(selected)}
        onToggleChild={toggleChild}
        onOpenChild={openChild}
        onError={onError}
      />
    );
  }

  return (
    <ConversationCatalog
      searchInput={searchInput}
      onSearchInput={setSearchInput}
      search={search}
      page={page}
      onPage={setPage}
      pageData={pageData}
      loading={catalogLoading}
      error={catalogError}
      indexProgress={indexProgress}
      toolNames={toolNames}
      toolNameOptions={toolNameOptions}
      toolFailed={toolFailed}
      onToolNames={setToolNames}
      onToolFailed={setToolFailed}
      onOpen={loadDetail}
    />
  );
}
