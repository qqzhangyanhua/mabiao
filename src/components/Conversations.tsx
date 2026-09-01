import { invoke } from "@tauri-apps/api/core";
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type UIEvent,
} from "react";
import { Icon } from "../icons";
import {
  conversationFocusToRestore,
  hashForConversation,
  replaceLocationHash,
} from "../hooks/viewCache";
import { conversationKey } from "../lib/conversationCache";
import {
  conversationJumpBehavior,
  conversationJumpScrollTop,
  conversationTimelineScrollTarget,
  createConversationRequestGate,
  isConversationResponseCurrent,
  isNearConversationBottom,
  isNearConversationTop,
  nextConversationFollowState,
  nextConversationRevisionPollState,
  type ConversationJumpEdge,
} from "../lib/conversationFollow";
import {
  currentConversationFrame,
  type ConversationDetailTab,
  initialConversationNavigationState,
  shouldRequestConversationDetail,
  transitionConversationNavigation,
} from "../lib/conversationNavigation";
import { humanStatus } from "../lib/format";
import type {
  ConversationAgentLink,
  ConversationDetailDto,
  ConversationDetailStateDto,
  ConversationFocus,
  ConversationPage,
  ConversationSessionRow,
  Filter,
} from "../types";
import { ConversationCatalogRow } from "./ConversationCatalogRow";
import { ConversationDetailHead } from "./ConversationDetailHead";
import { ConversationJumpBar } from "./ConversationJumpBar";
import {
  ConversationTimeline,
  type ConversationTimelineHandle,
} from "./ConversationTimeline";
import { ConversationUsageTable } from "./ConversationUsageTable";
import { CursorSessionDetail } from "./CursorSessionDetail";
import { EmptyState } from "./EmptyState";
import { LoadingOverlay } from "./LoadingOverlay";
import { Pagination } from "./Pagination";
import { Spinner } from "./Spinner";
import type { ConversationExportFormat } from "./type";
import { Button } from "./ui/Button";
import { SearchField } from "./ui/Field";
import { Segmented } from "./ui/Segmented";
import { SESSION_ENTRY_COPY } from "../lib/sessionEntryCopy";

const PAGE_SIZE = 20;

type ConversationDetailRequestIntent = {
  session: ConversationSessionRow;
  key: string;
  generation: number;
  followUpdates: boolean;
};
const DETAIL_TABS: { value: ConversationDetailTab; label: string }[] = [
  { value: "events", label: "完整事件" },
  { value: "usage", label: "用量明细" },
];
const BEHAVIOR_TAB: { value: ConversationDetailTab; label: string } = {
  value: "behavior",
  label: "行为统计",
};

const AGENT_CAPABILITY_MESSAGES = {
  partial: "部分子代理关系可确定，其余会话保持分离。",
  unavailable: "无法确定子代理关系，相关会话保持独立。",
} as const;

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
  const [searchInput, setSearchInput] = useState("");
  const [search, setSearch] = useState("");
  const [page, setPage] = useState(1);
  const [pageData, setPageData] = useState<ConversationPage>({ rows: [], total: 0 });
  const [catalogLoading, setCatalogLoading] = useState(false);
  const [catalogError, setCatalogError] = useState<string | null>(null);
  const [navigation, setNavigation] = useState(initialConversationNavigationState);
  const [details, setDetails] = useState<Record<string, ConversationDetailDto>>({});
  const [detailLoadingByKey, setDetailLoadingByKey] = useState<Record<string, boolean>>({});
  const [detailErrorsByKey, setDetailErrorsByKey] = useState<Record<string, string>>({});
  const [fileAvailableByKey, setFileAvailableByKey] = useState<Record<string, boolean>>({});
  const [pollErrorsByKey, setPollErrorsByKey] = useState<Record<string, string>>({});
  const [unseenCount, setUnseenCount] = useState(0);
  const [atTop, setAtTop] = useState(true);
  const [atBottom, setAtBottom] = useState(true);
  const currentFrame = currentConversationFrame(navigation);
  const selected = currentFrame?.session ?? null;
  const selectedKey = selected ? conversationKey(selected) : null;
  const detail = selectedKey ? (details[selectedKey] ?? null) : null;
  const detailTab: ConversationDetailTab = currentFrame?.tab ?? "events";
  const detailLoading = selectedKey ? Boolean(detailLoadingByKey[selectedKey]) : false;
  const detailError = selectedKey ? (detailErrorsByKey[selectedKey] ?? null) : null;
  const detailFileAvailable = selectedKey
    ? (fileAvailableByKey[selectedKey] ?? selected?.file_available ?? true)
    : true;
  const pollError = selectedKey ? (pollErrorsByKey[selectedKey] ?? null) : null;
  const usageIdentity = selected && detail
    ? `${selected.source}:${selected.session_id}:${detail.revision}:${revision}`
    : "";
  const [usageIdentitySeen, setUsageIdentitySeen] = useState("");
  const [usageTotal, setUsageTotal] = useState<number | null>(null);
  if (usageIdentity !== usageIdentitySeen) {
    setUsageIdentitySeen(usageIdentity);
    setUsageTotal(null);
  }
  const [exportFormat, setExportFormat] = useState<ConversationExportFormat | null>(null);
  const [exportStatus, setExportStatus] = useState<string | null>(null);
  const [exportError, setExportError] = useState(false);
  const catalogGeneration = useRef(0);
  const detailGenerations = useRef(new Map<string, number>());
  const detailRequestGates = useRef(
    new Map<
      string,
      ReturnType<typeof createConversationRequestGate<ConversationDetailRequestIntent>>
    >(),
  );
  const mountedRef = useRef(true);
  const selectedKeyRef = useRef<string | null>(selectedKey);
  selectedKeyRef.current = selectedKey;
  const detailsRef = useRef<Record<string, ConversationDetailDto>>({});
  const observedDetailRevisions = useRef(new Map<string, string>());
  const timelineRef = useRef<HTMLDivElement>(null);
  const timelineApiRef = useRef<ConversationTimelineHandle | null>(null);
  const windowEdgesRef = useRef({ hasMoreBefore: false, hasMoreAfter: false });
  const wasAtBottomRef = useRef(true);
  const pendingScrollRef = useRef(false);
  const savedTimelineScrollTopRef = useRef(0);
  const unseenCountRef = useRef(0);
  const jumpingRef = useRef(false);
  const jumpTokenRef = useRef(0);
  const jumpTimerRef = useRef(0);

  // 用户主动去看更早的内容，就别再让新事件把视口拽回底部。
  const captureTimelineAnchor = useCallback(() => {
    wasAtBottomRef.current = false;
    pendingScrollRef.current = false;
  }, []);

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
          const follow = nextConversationFollowState({
            previousCount: detailsRef.current[key]?.event_count ?? 0,
            nextCount: result.event_count,
            wasAtBottom: wasAtBottomRef.current,
            unseenCount: unseenCountRef.current,
          });
          pendingScrollRef.current = follow.shouldScroll;
          unseenCountRef.current = follow.unseenCount;
          setUnseenCount(follow.unseenCount);
        } else {
          pendingScrollRef.current = true;
          wasAtBottomRef.current = true;
          unseenCountRef.current = 0;
          setUnseenCount(0);
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
      jumpTokenRef.current += 1;
      jumpingRef.current = false;
      window.clearTimeout(jumpTimerRef.current);
      for (const gate of requestGates.values()) {
        gate.clearPending();
      }
    };
  }, []);

  useEffect(() => {
    const timer = window.setTimeout(() => setSearch(searchInput.trim()), 300);
    return () => window.clearTimeout(timer);
  }, [searchInput]);

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

  useEffect(() => {
    setPage(1);
  }, [filter, search]);

  useEffect(() => {
    const generation = ++catalogGeneration.current;
    setCatalogLoading(true);
    setCatalogError(null);
    invoke<ConversationPage>("get_conversation_sessions_page", {
      query: {
        search: search || null,
        page,
        page_size: PAGE_SIZE,
        sources: filter.sources,
        projects: filter.projects,
        models: filter.models,
        providers: filter.providers,
        from: filter.from,
        to: filter.to,
      },
    })
      .then((result) => {
        if (generation === catalogGeneration.current) {
          setPageData(result);
        }
      })
      .catch((error) => {
        if (generation === catalogGeneration.current) {
          setCatalogError(humanStatus(error));
          onError?.(error);
        }
      })
      .finally(() => {
        if (generation === catalogGeneration.current) {
          setCatalogLoading(false);
        }
      });
  }, [filter, revision, search, page, onError]);

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

  useEffect(() => {
    if (!selectedSource || !selectedSessionId) {
      return;
    }
    replaceLocationHash(hashForConversation(selectedSource, selectedSessionId));
  }, [selectedSource, selectedSessionId]);

  const syncTimelineEdge = useCallback((timeline: HTMLElement) => {
    const nextAtTop =
      isNearConversationTop(timeline) && !windowEdgesRef.current.hasMoreBefore;
    const nextAtBottom =
      isNearConversationBottom(timeline) && !windowEdgesRef.current.hasMoreAfter;
    setAtTop(nextAtTop);
    setAtBottom(nextAtBottom);
    if (!jumpingRef.current) {
      wasAtBottomRef.current = nextAtBottom;
      savedTimelineScrollTopRef.current = timeline.scrollTop;
    }
    if (nextAtBottom) {
      unseenCountRef.current = 0;
      setUnseenCount(0);
    }
  }, []);

  const handleWindowChange = useCallback(
    (edges: { hasMoreBefore: boolean; hasMoreAfter: boolean }) => {
      windowEdgesRef.current = edges;
      if (timelineRef.current) {
        syncTimelineEdge(timelineRef.current);
      }
    },
    [syncTimelineEdge],
  );

  useLayoutEffect(() => {
    if (!detail || detailTab !== "events") {
      return;
    }
    const timeline = timelineRef.current;
    if (!timeline) {
      setAtTop(true);
      setAtBottom(true);
      return;
    }

    const pinToFollowedEdge = () => {
      timeline.scrollTop = conversationTimelineScrollTarget({
        wasAtBottom: pendingScrollRef.current || wasAtBottomRef.current,
        savedScrollTop: savedTimelineScrollTopRef.current,
        scrollHeight: timeline.scrollHeight,
      });
      pendingScrollRef.current = false;
      syncTimelineEdge(timeline);
    };

    pinToFollowedEdge();
    const stack = timeline.firstElementChild;
    if (!(stack instanceof HTMLElement)) {
      return;
    }
    const observer = new ResizeObserver(() => {
      if (jumpingRef.current) {
        return;
      }
      if (wasAtBottomRef.current) {
        timeline.scrollTop = timeline.scrollHeight;
      }
      syncTimelineEdge(timeline);
    });
    observer.observe(stack);
    return () => observer.disconnect();
  }, [detail, detailTab, syncTimelineEdge]);

  function handleTimelineScroll(event: UIEvent<HTMLDivElement>) {
    syncTimelineEdge(event.currentTarget);
  }

  async function jumpTimeline(edge: ConversationJumpEdge) {
    const token = ++jumpTokenRef.current;
    window.clearTimeout(jumpTimerRef.current);

    const hadUnseen = unseenCountRef.current > 0;
    if (edge === "top") {
      pendingScrollRef.current = false;
      wasAtBottomRef.current = false;
      setAtBottom(false);
    } else {
      wasAtBottomRef.current = true;
      setAtBottom(true);
      unseenCountRef.current = 0;
      setUnseenCount(0);
    }

    const needsReload =
      edge === "top"
        ? windowEdgesRef.current.hasMoreBefore
        : windowEdgesRef.current.hasMoreAfter || hadUnseen;
    if (needsReload) {
      jumpingRef.current = false;
      if (edge === "top") {
        await timelineApiRef.current?.jumpToStart();
      } else {
        await timelineApiRef.current?.jumpToEnd();
      }
      if (token !== jumpTokenRef.current) {
        return;
      }
      if (timelineRef.current) {
        syncTimelineEdge(timelineRef.current);
      } else {
        setAtTop(edge === "top");
        setAtBottom(edge === "bottom");
      }
      return;
    }

    const timeline = timelineRef.current;
    if (!timeline) {
      jumpingRef.current = false;
      pendingScrollRef.current = edge === "bottom";
      setAtTop(edge === "top");
      setAtBottom(edge === "bottom");
      return;
    }

    const top = conversationJumpScrollTop(edge, timeline.scrollHeight);
    const maxTop = Math.max(0, timeline.scrollHeight - timeline.clientHeight);
    const targetTop = edge === "top" ? 0 : maxTop;
    const behavior = conversationJumpBehavior(
      window.matchMedia("(prefers-reduced-motion: reduce)").matches,
    );

    if (behavior === "auto" || Math.abs(timeline.scrollTop - targetTop) <= 40) {
      jumpingRef.current = false;
      timeline.scrollTop = top;
      syncTimelineEdge(timeline);
      return;
    }

    jumpingRef.current = true;
    timeline.scrollTo({ top, behavior: "smooth" });

    const settle = () => {
      if (token !== jumpTokenRef.current) {
        return;
      }
      jumpingRef.current = false;
      if (edge === "bottom") {
        timeline.scrollTop = timeline.scrollHeight;
      }
      syncTimelineEdge(timeline);
    };

    const onScrollEnd = () => {
      timeline.removeEventListener("scrollend", onScrollEnd);
      settle();
    };
    timeline.addEventListener("scrollend", onScrollEnd);
    jumpTimerRef.current = window.setTimeout(() => {
      timeline.removeEventListener("scrollend", onScrollEnd);
      settle();
    }, 1200);
  }

  function handleDetailTabChange(nextTab: ConversationDetailTab) {
    if (detailTab === "events" && nextTab !== "events") {
      const timeline = timelineRef.current;
      if (timeline) {
        savedTimelineScrollTopRef.current = timeline.scrollTop;
        wasAtBottomRef.current =
          isNearConversationBottom(timeline) && !windowEdgesRef.current.hasMoreAfter;
      }
    }
    setDetailTab(nextTab);
  }

  const loadDetail = useCallback(
    (session: ConversationSessionRow) => {
      setNavigation((current) =>
        transitionConversationNavigation(current, { type: "open_root", session }),
      );
      setExportFormat(null);
      setExportStatus(null);
      setExportError(false);
      savedTimelineScrollTopRef.current = 0;
      wasAtBottomRef.current = true;
      pendingScrollRef.current = true;
      jumpTokenRef.current += 1;
      jumpingRef.current = false;
      window.clearTimeout(jumpTimerRef.current);
      unseenCountRef.current = 0;
      setUnseenCount(0);
      windowEdgesRef.current = { hasMoreBefore: false, hasMoreAfter: false };
      fetchDetail(session);
    },
    [fetchDetail],
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
  }, [focus, loadDetail, onError, onFocusConsumed]);

  function closeDetail() {
    replaceLocationHash("conversations");
    setNavigation((current) => transitionConversationNavigation(current, { type: "close" }));
    detailGenerations.current.clear();
    observedDetailRevisions.current.clear();
    for (const gate of detailRequestGates.current.values()) {
      gate.clearPending();
    }
    detailRequestGates.current.clear();
    detailsRef.current = {};
    setDetails({});
    windowEdgesRef.current = { hasMoreBefore: false, hasMoreAfter: false };
    setDetailLoadingByKey({});
    setDetailErrorsByKey({});
    setFileAvailableByKey({});
    setPollErrorsByKey({});
    savedTimelineScrollTopRef.current = 0;
    unseenCountRef.current = 0;
    setUnseenCount(0);
    pendingScrollRef.current = false;
    jumpTokenRef.current += 1;
    jumpingRef.current = false;
    window.clearTimeout(jumpTimerRef.current);
    setExportFormat(null);
    setExportStatus(null);
    setExportError(false);
  }

  function backToParent() {
    const scrollTop = navigation.frames.at(-2)?.scroll_top ?? 0;
    setNavigation((current) => transitionConversationNavigation(current, { type: "back" }));
    savedTimelineScrollTopRef.current = scrollTop;
    wasAtBottomRef.current = false;
    pendingScrollRef.current = false;
    jumpTokenRef.current += 1;
    jumpingRef.current = false;
    window.clearTimeout(jumpTimerRef.current);
    unseenCountRef.current = 0;
    setUnseenCount(0);
  }

  function setDetailTab(tab: ConversationDetailTab) {
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
    const parentScrollTop = timelineRef.current?.scrollTop ?? 0;
    setNavigation((current) =>
      transitionConversationNavigation(current, {
        type: "enter_child",
        session: link.session!,
        relationship_id: link.relationship_id,
        parent_scroll_top: parentScrollTop,
      }),
    );
    savedTimelineScrollTopRef.current = 0;
    wasAtBottomRef.current = true;
    pendingScrollRef.current = true;
    jumpTokenRef.current += 1;
    jumpingRef.current = false;
    window.clearTimeout(jumpTimerRef.current);
    unseenCountRef.current = 0;
    setUnseenCount(0);
    windowEdgesRef.current = { hasMoreBefore: false, hasMoreAfter: false };
    fetchDetail(link.session);
  }

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

  if (selected) {
    const session = detail?.session ?? selected;
    return (
      <div className="conversation-detail-view">
        <ConversationDetailHead
          session={session}
          fileAvailable={detailFileAvailable}
          breadcrumb={
            navigation.frames.length > 1
              ? navigation.frames.map((frame) => frame.session.title).join(" / ")
              : null
          }
          parentAvailable={navigation.frames.length > 1}
          exportFormat={exportFormat}
          exportStatus={exportStatus}
          exportError={exportError}
          exportDisabled={!detailFileAvailable || !detail}
          onBack={navigation.frames.length > 1 ? backToParent : closeDetail}
          onExport={(format) => void exportConversation(format)}
        />

        <section className="conversation-detail-body" aria-busy={detailLoading}>
          <div className="conversation-detail-tabs">
            <Segmented
              value={detailTab}
              options={detail?.cursor_behavior ? [...DETAIL_TABS, BEHAVIOR_TAB] : DETAIL_TABS}
              disabled={detailLoading || Boolean(detailError)}
              ariaLabel="对话详情视图"
              onChange={handleDetailTabChange}
            />
            {detail ? (
              <span className="muted">
                {detailTab === "events"
                  ? `${detail.event_count} 条事件`
                  : detailTab === "behavior"
                    ? "Cursor 行为聚合"
                    : usageTotal === null
                      ? "用量明细"
                      : `${usageTotal} 条记录`}
              </span>
            ) : null}
          </div>
          {!detailFileAvailable ? (
            <div className="conversation-detail-notice" role="status">
              <Icon name="alertTriangle" size={16} />
              <div>
                <strong>
                  {session.source === "cursor_agent"
                    ? "缺少 Cursor transcript，对话正文不可读取"
                    : "原文件已删除，详情不可继续读取"}
                </strong>
                <span>
                  {session.source === "cursor_agent"
                    ? "仍可查看确定性关联的用量、行为统计与会话状态。"
                    : detail
                      ? "当前显示的是已加载快照；文件恢复后将自动更新。"
                      : "仍可查看目录元数据；文件恢复后将自动读取详情。"}
                </span>
              </div>
            </div>
          ) : null}
          {pollError ? (
            <div className="conversation-detail-notice" role="status">
              <Icon name="alertTriangle" size={16} />
              <div>
                <strong>暂时无法检查最新内容</strong>
                <span>{pollError}；后台将继续重试。</span>
              </div>
            </div>
          ) : null}
          {detailLoading ? (
            <EmptyState icon="chat" title="正在读取原始会话…" />
          ) : detailError ? (
            <div className="conversation-load-error" role="alert">
              <EmptyState
                icon="alertTriangle"
                tone="warn"
                title="无法读取对话详情"
                hint={detailError}
              />
              <Button onClick={() => fetchDetail(selected)}>重新读取</Button>
            </div>
          ) : detail ? (
            detailTab === "usage" ? (
              <ConversationUsageTable
                key={usageIdentity}
                source={session.source}
                sessionId={session.session_id}
                refreshKey={usageIdentity}
                onTotalChange={setUsageTotal}
                onError={onError}
              />
            ) : detailTab === "behavior" && detail.cursor_behavior ? (
              <CursorSessionDetail detail={detail.cursor_behavior} embedded />
            ) : (
              <div className="conversation-events-view">
                {detail.agent_relations.capability_status !== "complete" ? (
                  <div
                    className={`conversation-agent-capability status-${detail.agent_relations.capability_status}`}
                    role="status"
                  >
                    <Icon name="alertTriangle" size={14} />
                    <span>
                      {AGENT_CAPABILITY_MESSAGES[detail.agent_relations.capability_status]}
                    </span>
                  </div>
                ) : null}
                <ConversationTimeline
                  key={`${session.source}:${session.session_id}`}
                  source={session.source}
                  sessionId={session.session_id}
                  revision={detail.revision}
                  eventCount={detail.event_count}
                  agentLinks={detail.agent_relations.children}
                  expandedRelationshipIds={currentFrame?.expanded_relationship_ids ?? []}
                  followLatest={atBottom}
                  onToggleChild={toggleChild}
                  onOpenChild={openChild}
                  timelineRef={timelineRef}
                  timelineApiRef={timelineApiRef}
                  onScroll={handleTimelineScroll}
                  onWindowChange={handleWindowChange}
                  onCaptureScrollAnchor={captureTimelineAnchor}
                />
                <ConversationJumpBar
                  atTop={atTop}
                  atBottom={atBottom}
                  unseenCount={unseenCount}
                  onJumpTop={() => void jumpTimeline("top")}
                  onJumpBottom={() => void jumpTimeline("bottom")}
                />
              </div>
            )
          ) : null}
        </section>
      </div>
    );
  }

  const { rows, total } = pageData;
  const pageCount = Math.max(1, Math.ceil(total / PAGE_SIZE));
  const maxTotal = Math.max(1, ...rows.map((row) => row.total_tokens));

  return (
    <section className="panel conversation-catalog">
      <div className="panel-head conversation-catalog-head">
        <div>
          <h2>本地会话目录</h2>
          <p className="panel-note">{SESSION_ENTRY_COPY.conversationCatalogNote}</p>
        </div>
        <SearchField
          value={searchInput}
          onChange={setSearchInput}
          placeholder="搜索标题、来源、项目、模型、ID 或时间"
          ariaLabel="搜索对话记录"
        />
        <span className="muted conversation-total">
          共 {total} 条
          {catalogLoading ? (
            <span className="inline-loading">
              <Spinner size={12} />
              加载中…
            </span>
          ) : null}
        </span>
      </div>

      {catalogError && rows.length === 0 ? (
        <div role="alert">
          <EmptyState
            icon="alertTriangle"
            tone="warn"
            title="无法加载对话目录"
            hint={catalogError}
          />
        </div>
      ) : (
        <LoadingOverlay
          active={catalogLoading && rows.length > 0}
          className="table-scroll conversation-table-scroll"
        >
          <table className="conversation-table">
            <thead>
              <tr>
                <th>标题</th>
                <th>来源</th>
                <th>项目</th>
                <th>模型</th>
                <th>token</th>
                <th>费用</th>
                <th>起止</th>
                <th>能力</th>
                <th>状态</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => (
                <ConversationCatalogRow
                  key={`${row.source}-${row.session_id}`}
                  row={row}
                  maxTotal={maxTotal}
                  onOpen={loadDetail}
                />
              ))}
              {rows.length === 0 ? (
                <tr>
                  <td colSpan={9} className="analytics-empty">
                    {catalogLoading ? (
                      <EmptyState icon="chat" title="正在加载对话目录…" />
                    ) : (
                      <EmptyState
                        icon="chat"
                        title="当前条件下暂无对话记录"
                        hint="请确认本机已有会话文件，并执行一次刷新。Cursor 与其它来源共用此目录。"
                      />
                    )}
                  </td>
                </tr>
              ) : null}
            </tbody>
          </table>
        </LoadingOverlay>
      )}
      <Pagination page={page} pageCount={pageCount} totalCount={total} onPageChange={setPage} />
    </section>
  );
}
