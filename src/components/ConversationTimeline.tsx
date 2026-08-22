import { invoke } from "@tauri-apps/api/core";
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type RefObject,
  type UIEvent,
} from "react";
import { Icon } from "../icons";
import { conversationKey } from "../lib/conversationCache";
import { ConversationMarkdown } from "../lib/conversationMarkdown";
import {
  conversationWindowSlice,
  initialConversationHiddenCount,
  revealEarlierConversationEvents,
} from "../lib/conversationWindow";
import { formatClock, humanStatus } from "../lib/format";
import { dataUrlToBlob } from "../lib/objectUrl";
import type {
  ConversationAgentLink,
  ConversationAttachment,
  ConversationAttachmentContentDto,
  ConversationDetailDto,
  ConversationEvent,
  ConversationEventActor,
  ConversationEventCapabilityStatus,
  ConversationEventContentDto,
  ConversationEventKind,
} from "../types";
import { EmptyState } from "./EmptyState";
import { Spinner } from "./Spinner";
import { Button } from "./ui/Button";

const AGENT_LINK_LABELS = {
  linked: "已关联",
  missing_source: "子会话源不可用",
  unresolved: "无法确定子会话",
  conflict: "关联冲突",
  cycle: "循环关联",
} as const;

const EVENT_LABELS: Record<ConversationEventKind, string> = {
  message: "消息",
  plan: "计划",
  tool_call: "工具调用",
  tool_result: "工具结果",
  model_change: "模型切换",
  error: "错误",
  system_status: "系统状态",
  unadapted: "尚未适配",
};

const ACTOR_LABELS: Record<ConversationEventActor, string> = {
  user: "用户",
  assistant: "助手",
  tool: "工具",
};

const CAPABILITY_STATUS_LABELS: Record<ConversationEventCapabilityStatus, string> = {
  complete: "完整",
  missing_timestamp: "时间缺失",
  unadapted: "尚未适配",
  unadapted_missing_timestamp: "尚未适配、时间缺失",
};

function actorLabel(actor: ConversationEventActor): string {
  return ACTOR_LABELS[actor];
}

function capabilityStatusLabel(status: ConversationEventCapabilityStatus): string {
  return CAPABILITY_STATUS_LABELS[status];
}

function hasEventDetails(details: unknown): boolean {
  if (details == null) {
    return false;
  }
  if (Array.isArray(details)) {
    return details.length > 0;
  }
  if (typeof details === "object") {
    return Object.keys(details).length > 0;
  }
  return true;
}

function prettyDetails(details: unknown): string {
  try {
    return JSON.stringify(details, null, 2) ?? String(details);
  } catch {
    return String(details);
  }
}

function formatBytes(bytes: number | null): string {
  if (bytes === null) {
    return "大小未知";
  }
  if (bytes < 1024) {
    return `${bytes} B`;
  }
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  let unit = units[0];
  for (let index = 1; value >= 1024 && index < units.length; index += 1) {
    value /= 1024;
    unit = units[index];
  }
  return `${value >= 10 ? value.toFixed(0) : value.toFixed(1)} ${unit}`;
}

function attachmentStatusText(attachment: ConversationAttachment): string {
  if (attachment.status === "missing") {
    return "原附件已不存在";
  }
  if (attachment.status === "unsupported") {
    return "无法在应用内加载";
  }
  return attachment.status === "embedded" ? "已嵌入" : "可用";
}

function attachmentSignature(attachment: ConversationAttachment): string {
  return `${attachment.kind}\u0000${attachment.status}\u0000${attachment.original_path}\u0000${attachment.size_bytes ?? ""}`;
}

function attachmentRequestKey(attachment: ConversationAttachment): string {
  return `${attachment.id}\u0000${attachmentSignature(attachment)}`;
}

type ObjectUrlEntry = { signature: string; url: string };
type AsyncLoadState = "loading" | "error";

function useKeyedAsyncLoad<Key extends string | number>() {
  const [states, setStates] = useState<Partial<Record<Key, AsyncLoadState>>>({});
  const [errors, setErrors] = useState<Partial<Record<Key, string>>>({});
  const mounted = useRef(true);
  const inFlight = useRef(new Set<Key>());

  useEffect(() => {
    const activeRequests = inFlight.current;
    mounted.current = true;
    return () => {
      mounted.current = false;
      activeRequests.clear();
    };
  }, []);

  const run = useCallback(
    async <Result,>(key: Key, task: () => Promise<Result>, onSuccess: (result: Result) => void) => {
      if (inFlight.current.has(key)) {
        return;
      }
      inFlight.current.add(key);
      setStates((current) => ({ ...current, [key]: "loading" }));
      setErrors((current) => {
        const next = { ...current };
        delete next[key];
        return next;
      });
      try {
        const result = await task();
        if (!mounted.current) {
          return;
        }
        onSuccess(result);
        setStates((current) => {
          const next = { ...current };
          delete next[key];
          return next;
        });
      } catch (error) {
        if (mounted.current) {
          setStates((current) => ({ ...current, [key]: "error" }));
          setErrors((current) => ({ ...current, [key]: humanStatus(error) }));
        }
      } finally {
        inFlight.current.delete(key);
      }
    },
    [],
  );

  return { states, errors, run };
}

function ImageDialog({
  name,
  url,
  onClose,
}: {
  name: string;
  url: string;
  onClose: () => void;
}) {
  const titleId = `conversation-image-${encodeURIComponent(name).replaceAll("%", "")}`;
  const dialogRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const previousFocus =
      document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const dialog = dialogRef.current;
    const focusable = () =>
      Array.from(
        dialog?.querySelectorAll<HTMLElement>(
          'button:not([disabled]), a[href], input:not([disabled]), [tabindex]:not([tabindex="-1"])',
        ) ?? [],
      );
    focusable()[0]?.focus();

    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        onClose();
        return;
      }
      if (event.key !== "Tab") {
        return;
      }
      const controls = focusable();
      if (controls.length === 0) {
        event.preventDefault();
        dialog?.focus();
        return;
      }
      const first = controls[0];
      const last = controls[controls.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    }
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
      previousFocus?.focus();
    };
  }, [onClose]);

  return (
    <div
      className="conversation-image-backdrop"
      onClick={(event) => {
        if (event.target === event.currentTarget) {
          onClose();
        }
      }}
    >
      <div
        ref={dialogRef}
        className="conversation-image-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
      >
        <header>
          <h3 id={titleId}>{name}</h3>
          <Button variant="icon" onClick={onClose} aria-label="关闭图片预览">
            <Icon name="close" size={15} />
          </Button>
        </header>
        <div className="conversation-image-stage">
          <img src={url} alt={name} />
        </div>
      </div>
    </div>
  );
}

export type ConversationTimelineProps = {
  events: ConversationEvent[];
  source: string;
  sessionId: string;
  agentLinks: ConversationAgentLink[];
  expandedRelationshipIds: string[];
  childDetails: Record<string, ConversationDetailDto>;
  childLoading: Record<string, boolean>;
  depth?: number;
  onToggleChild: (link: ConversationAgentLink) => void;
  onOpenChild: (link: ConversationAgentLink) => void;
  onEventContentLoaded: (
    source: string,
    sessionId: string,
    content: ConversationEventContentDto,
  ) => void;
  timelineRef?: RefObject<HTMLDivElement | null>;
  onScroll?: (event: UIEvent<HTMLDivElement>) => void;
  /** 展开更早事件前后由父组件记录/还原滚动锚点——滚动容器是它的 ref。 */
  onCaptureScrollAnchor?: () => void;
  onRestoreScrollAnchor?: () => void;
};

export function ConversationTimeline({
  events,
  source,
  sessionId,
  agentLinks,
  expandedRelationshipIds,
  childDetails,
  childLoading,
  depth = 0,
  onToggleChild,
  onOpenChild,
  onEventContentLoaded,
  timelineRef,
  onScroll,
  onCaptureScrollAnchor,
  onRestoreScrollAnchor,
}: ConversationTimelineProps) {
  const {
    states: eventLoads,
    errors: eventErrors,
    run: runEventLoad,
  } = useKeyedAsyncLoad<string>();
  const {
    states: thumbnailLoads,
    errors: thumbnailErrors,
    run: runThumbnailLoad,
  } = useKeyedAsyncLoad<string>();
  const {
    states: imageLoads,
    errors: imageErrors,
    run: runImageLoad,
  } = useKeyedAsyncLoad<string>();

  // 组件挂载时 `events` 已经就绪（父级只在详情加载完成后才渲染时间线），
  // 换会话会带着新的 key 重新挂载，所以窗口只需要在这里初始化一次。
  const [hiddenCount, setHiddenCount] = useState(() =>
    initialConversationHiddenCount(events.length),
  );
  const visibleEvents = useMemo(
    () => conversationWindowSlice(events, hiddenCount),
    [events, hiddenCount],
  );

  const [thumbnailUrls, setThumbnailUrls] = useState<Record<string, ObjectUrlEntry>>({});
  const requestedThumbnails = useRef(new Map<string, string>());
  const [openImage, setOpenImage] = useState<{ name: string; url: string } | null>(null);

  // 卸载时要释放的是最后一刻的 URL 集合，只能经由 ref 带到清理函数里。
  const liveObjectUrls = useRef<{
    thumbnails: Record<string, ObjectUrlEntry>;
    image: { name: string; url: string } | null;
  }>({ thumbnails: {}, image: null });
  useEffect(() => {
    liveObjectUrls.current = { thumbnails: thumbnailUrls, image: openImage };
  }, [thumbnailUrls, openImage]);
  useEffect(
    () => () => {
      for (const entry of Object.values(liveObjectUrls.current.thumbnails)) {
        URL.revokeObjectURL(entry.url);
      }
      const image = liveObjectUrls.current.image;
      if (image) {
        URL.revokeObjectURL(image.url);
      }
    },
    [],
  );

  const closeImage = useCallback(() => {
    setOpenImage((current) => {
      if (current) {
        URL.revokeObjectURL(current.url);
      }
      return null;
    });
  }, []);

  // 只有窗口内的附件会被请求，缩略图数量因此跟着渲染窗口一起收敛。
  const visibleAttachmentSignatures = useMemo(
    () =>
      new Map(
        visibleEvents.flatMap((event) =>
          event.attachments.map(
            (attachment) => [attachment.id, attachmentSignature(attachment)] as const,
          ),
        ),
      ),
    [visibleEvents],
  );
  const attachmentSignatures = useRef(visibleAttachmentSignatures);
  useEffect(() => {
    attachmentSignatures.current = visibleAttachmentSignatures;
  }, [visibleAttachmentSignatures]);

  async function loadFullEvent(eventId: string) {
    await runEventLoad(
      eventId,
      () =>
        invoke<ConversationEventContentDto>("get_conversation_event_content", {
          source,
          sessionId,
          eventId,
        }),
      (content) => onEventContentLoaded(source, sessionId, content),
    );
  }

  const loadThumbnail = useCallback(
    async (attachment: ConversationAttachment, retry = false) => {
      if (retry) {
        requestedThumbnails.current.delete(attachment.id);
      }
      const signature = attachmentSignature(attachment);
      if (requestedThumbnails.current.get(attachment.id) === signature) {
        return;
      }
      requestedThumbnails.current.set(attachment.id, signature);
      await runThumbnailLoad(
        attachmentRequestKey(attachment),
        () =>
          invoke<ConversationAttachmentContentDto>("get_conversation_attachment_thumbnail", {
            source,
            sessionId,
            attachmentId: attachment.id,
          }),
        (result) => {
          if (attachmentSignatures.current.get(attachment.id) !== signature) {
            return;
          }
          const blob = dataUrlToBlob(result.data_url);
          if (!blob) {
            return;
          }
          const url = URL.createObjectURL(blob);
          setThumbnailUrls((current) => {
            const previous = current[attachment.id];
            if (previous) {
              URL.revokeObjectURL(previous.url);
            }
            return { ...current, [attachment.id]: { signature, url } };
          });
        },
      );
    },
    [runThumbnailLoad, sessionId, source],
  );

  useEffect(() => {
    for (const event of visibleEvents) {
      for (const attachment of event.attachments) {
        if (
          attachment.kind === "image" &&
          (attachment.status === "available" || attachment.status === "embedded")
        ) {
          void loadThumbnail(attachment);
        }
      }
    }
  }, [visibleEvents, loadThumbnail]);

  // 原图只保留当前打开的一张：几 MB 一张，缓存全部看过的图是纯粹的内存浪费，
  // 重新打开只是再读一次本地文件。
  async function loadImage(attachment: ConversationAttachment) {
    const signature = attachmentSignature(attachment);
    await runImageLoad(
      attachmentRequestKey(attachment),
      () =>
        invoke<ConversationAttachmentContentDto>("get_conversation_attachment", {
          source,
          sessionId,
          attachmentId: attachment.id,
        }),
      (result) => {
        if (attachmentSignatures.current.get(attachment.id) !== signature) {
          return;
        }
        const blob = dataUrlToBlob(result.data_url);
        if (!blob) {
          return;
        }
        const url = URL.createObjectURL(blob);
        setOpenImage((current) => {
          if (current) {
            URL.revokeObjectURL(current.url);
          }
          return { name: attachment.name, url };
        });
      },
    );
  }

  // 展开更早的事件会把内容插在顶部，浏览器只保 scrollTop，视口会跟着往上跳。
  // 滚动容器归父组件所有，锚点的记录与还原也交给它。
  function revealEarlier() {
    onCaptureScrollAnchor?.();
    setHiddenCount((current) => revealEarlierConversationEvents(current));
  }
  useLayoutEffect(() => {
    onRestoreScrollAnchor?.();
  }, [hiddenCount, onRestoreScrollAnchor]);

  // 归属判断要看全量事件：窗口外事件的子代理链接跟着那条事件一起等到展开时再渲染，
  // 而不是被误判成「无归属」掉到列表末尾。
  const eventIds = new Set(events.map((event) => event.event_id));
  const linksForEvent = (eventId: string) =>
    agentLinks.filter((link) => link.launch_event_id === eventId);
  const trailingLinks = agentLinks.filter(
    (link) => link.launch_event_id === null || !eventIds.has(link.launch_event_id),
  );

  function renderAgentLinks(links: ConversationAgentLink[]) {
    return links.map((link) => {
      const linkedSession = link.status === "linked" ? link.session : null;
      const expanded = expandedRelationshipIds.includes(link.relationship_id);
      const nestedDetail = linkedSession ? childDetails[conversationKey(linkedSession)] : null;
      const nestedLoading = linkedSession ? childLoading[conversationKey(linkedSession)] : false;
      const controlsId = `agent-timeline-${link.relationship_id.replaceAll(/[^a-zA-Z0-9_-]/g, "-")}`;
      return (
        <section
          className={`conversation-agent-link depth-${Math.min(depth, 3)} status-${link.status}`}
          key={link.relationship_id}
        >
          <div className="conversation-agent-link-head">
            <Button
              variant="icon"
              size="sm"
              onClick={() => onToggleChild(link)}
              disabled={!linkedSession}
              aria-label={expanded ? "收起子代理时间线" : "展开子代理时间线"}
              aria-expanded={expanded}
              aria-controls={controlsId}
              title={expanded ? "收起子代理时间线" : "展开子代理时间线"}
            >
              <Icon name="chevron" size={13} />
            </Button>
            <div className="conversation-agent-link-title">
              <strong>{linkedSession?.title || link.session_id || "未解析的子代理"}</strong>
              <span>{AGENT_LINK_LABELS[link.status]}</span>
              {link.session_id ? <code>{link.session_id}</code> : null}
            </div>
            {linkedSession ? (
              <Button
                variant="text"
                size="sm"
                onClick={() => onOpenChild(link)}
                data-relationship-id={link.relationship_id}
              >
                打开详情
              </Button>
            ) : null}
          </div>
          {expanded && linkedSession ? (
            <div className="conversation-agent-link-body" id={controlsId}>
              {nestedLoading && !nestedDetail ? (
                <div className="conversation-agent-loading">
                  <Spinner size={14} />
                </div>
              ) : nestedDetail ? (
                <ConversationTimeline
                  key={conversationKey(nestedDetail.session)}
                  events={nestedDetail.events}
                  source={nestedDetail.session.source}
                  sessionId={nestedDetail.session.session_id}
                  agentLinks={nestedDetail.agent_relations.children}
                  expandedRelationshipIds={expandedRelationshipIds}
                  childDetails={childDetails}
                  childLoading={childLoading}
                  depth={depth + 1}
                  onToggleChild={onToggleChild}
                  onOpenChild={onOpenChild}
                  onEventContentLoaded={onEventContentLoaded}
                />
              ) : (
                <span className="conversation-inline-error">子会话内容不可用</span>
              )}
            </div>
          ) : null}
        </section>
      );
    });
  }

  if (events.length === 0 && agentLinks.length === 0) {
    return (
      <EmptyState icon="chat" title="这条会话暂无事件" hint="当前会话没有可展示的语义事件。" />
    );
  }

  return (
    <>
      <div
        className="conversation-timeline"
        aria-label="完整事件列表"
        ref={timelineRef}
        onScroll={onScroll}
      >
        <div className="conversation-timeline-stack">
          {hiddenCount > 0 ? (
            <div className="conversation-timeline-earlier">
              <span className="muted">上方还有 {hiddenCount} 条更早事件未渲染</span>
              <Button size="sm" onClick={revealEarlier}>
                加载更早
              </Button>
            </div>
          ) : null}
          {visibleEvents.map((event) => {
            const label = EVENT_LABELS[event.kind];
            const showDetails =
              event.kind === "unadapted" ||
              ((event.kind === "plan" ||
                event.kind === "tool_call" ||
                event.kind === "tool_result") &&
                hasEventDetails(event.details));
            const showCapabilityStatus =
              event.capability_status !== "complete" &&
              event.kind !== "unadapted" &&
              event.occurred_at !== null;
            const usesMarkdown =
              event.kind === "message" ||
              event.kind === "plan" ||
              event.kind === "error" ||
              event.kind === "tool_result";
            const isDeferred = event.content_status === "deferred";
            return (
              <div className="conversation-event-group" key={event.event_id}>
                <article className={`conversation-event event-${event.kind.replaceAll("_", "-")}`}>
                  <header className="conversation-event-meta">
                    <strong>{label}</strong>
                    {event.occurred_at ? (
                      <time dateTime={event.occurred_at}>{formatClock(event.occurred_at)}</time>
                    ) : (
                      <span className="conversation-event-missing-time">时间缺失</span>
                    )}
                  </header>
                  <div className="conversation-event-content">
                    {event.kind === "unadapted" ? (
                      <span className="conversation-unadapted-state">尚未适配</span>
                    ) : showCapabilityStatus ? (
                      <span className="conversation-capability-status">
                        {capabilityStatusLabel(event.capability_status)}
                      </span>
                    ) : null}
                    {event.actor || event.name ? (
                      <div className="conversation-event-identity">
                        {event.actor ? <span>{actorLabel(event.actor)}</span> : null}
                        {event.name ? <code>{event.name}</code> : null}
                      </div>
                    ) : null}
                    {event.text ? (
                      usesMarkdown ? (
                        <ConversationMarkdown markdown={event.text} />
                      ) : (
                        <pre className="conversation-event-text conversation-event-command">
                          {event.text}
                        </pre>
                      )
                    ) : null}
                    {isDeferred ? (
                      <div className="conversation-deferred" aria-live="polite">
                        <span>仅显示前部内容</span>
                        <Button
                          variant="text"
                          onClick={() => void loadFullEvent(event.event_id)}
                          disabled={eventLoads[event.event_id] === "loading"}
                        >
                          {eventLoads[event.event_id] === "loading" ? <Spinner size={12} /> : null}
                          加载全文
                        </Button>
                        {eventLoads[event.event_id] === "error" ? (
                          <span className="conversation-inline-error" role="alert">
                            {eventErrors[event.event_id]}
                          </span>
                        ) : null}
                      </div>
                    ) : null}
                    {showDetails ? (
                      <details className="conversation-event-details">
                        <summary>
                          {event.kind === "unadapted" ? "查看原始事件" : "查看详细数据"}
                        </summary>
                        <pre>{prettyDetails(event.details)}</pre>
                      </details>
                    ) : null}
                    {event.attachments.length > 0 ? (
                      <div className="conversation-attachments" aria-label="附件">
                        {event.attachments.map((attachment) => {
                          const signature = attachmentSignature(attachment);
                          const requestKey = attachmentRequestKey(attachment);
                          const cachedThumbnail = thumbnailUrls[attachment.id];
                          const thumbnailUrl =
                            cachedThumbnail?.signature === signature ? cachedThumbnail.url : null;
                          const thumbnailState = thumbnailLoads[requestKey];
                          const imageState = imageLoads[requestKey];
                          const canLoadImage =
                            attachment.kind === "image" &&
                            (attachment.status === "available" || attachment.status === "embedded");
                          return (
                            <div className="conversation-attachment" key={attachment.id}>
                              <div className="conversation-attachment-main">
                                <strong>{attachment.name}</strong>
                                <code>{attachment.original_path || "—"}</code>
                                <div className="conversation-attachment-meta">
                                  <span>{attachment.media_type || "未知类型"}</span>
                                  <span>{formatBytes(attachment.size_bytes)}</span>
                                  <span className={`attachment-status status-${attachment.status}`}>
                                    {attachmentStatusText(attachment)}
                                  </span>
                                </div>
                                {thumbnailState === "error" ? (
                                  <div className="conversation-attachment-action">
                                    <span className="conversation-inline-error" role="alert">
                                      {thumbnailErrors[requestKey]}
                                    </span>
                                    <Button
                                      variant="text"
                                      onClick={() => void loadThumbnail(attachment, true)}
                                    >
                                      重试缩略图
                                    </Button>
                                  </div>
                                ) : null}
                                {imageState === "error" ? (
                                  <span className="conversation-inline-error" role="alert">
                                    {imageErrors[requestKey]}
                                  </span>
                                ) : null}
                              </div>
                              {canLoadImage ? (
                                thumbnailUrl ? (
                                  <button
                                    type="button"
                                    className="conversation-image-thumb"
                                    onClick={() => void loadImage(attachment)}
                                    disabled={imageState === "loading"}
                                    aria-label={`查看原图：${attachment.name}`}
                                  >
                                    <img src={thumbnailUrl} alt="" />
                                    {imageState === "loading" ? (
                                      <span className="conversation-image-loading" aria-hidden>
                                        <Spinner size={14} />
                                      </span>
                                    ) : null}
                                  </button>
                                ) : (
                                  <div
                                    className="conversation-image-placeholder"
                                    aria-label={
                                      thumbnailState === "error" ? undefined : "正在生成缩略图"
                                    }
                                    aria-hidden={thumbnailState === "error" || undefined}
                                  >
                                    {thumbnailState === "loading" ? (
                                      <Spinner size={14} />
                                    ) : (
                                      <Icon name="alertTriangle" size={14} />
                                    )}
                                  </div>
                                )
                              ) : null}
                            </div>
                          );
                        })}
                      </div>
                    ) : null}
                    {!event.text &&
                    !showDetails &&
                    !event.actor &&
                    !event.name &&
                    event.attachments.length === 0 ? (
                      <span className="muted">无附加内容</span>
                    ) : null}
                  </div>
                </article>
                {renderAgentLinks(linksForEvent(event.event_id))}
              </div>
            );
          })}
          {renderAgentLinks(trailingLinks)}
        </div>
      </div>
      {openImage ? (
        <ImageDialog name={openImage.name} url={openImage.url} onClose={closeImage} />
      ) : null}
    </>
  );
}
