import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";
import { Icon } from "../icons";
import { ConversationMarkdown } from "../lib/conversationMarkdown";
import {
  actorLabel,
  attachmentRequestKey,
  attachmentSignature,
  attachmentStatusText,
  capabilityStatusLabel,
  EVENT_LABELS,
  formatAttachmentBytes,
  hasEventDetails,
  prettyDetails,
} from "../lib/conversationEventDisplay";
import { formatClock } from "../lib/format";
import { HighlightedSnippet } from "../lib/highlightMatch";
import { dataUrlToBlob } from "../lib/objectUrl";
import { useKeyedAsyncLoad } from "../lib/useKeyedAsyncLoad";
import type {
  ConversationAttachment,
  ConversationAttachmentContentDto,
  ConversationEvent,
  ConversationEventContentDto,
} from "../types";
import { ConversationImageDialog } from "./ConversationImageDialog";
import { Spinner } from "./Spinner";
import { Button } from "./ui/Button";

type ObjectUrlEntry = { signature: string; url: string };

export function ConversationEventItem({
  event,
  source,
  sessionId,
  highlighted = false,
  highlightQuery = null,
  highlightSnippet = null,
  onEventContentLoaded,
}: {
  event: ConversationEvent;
  source: string;
  sessionId: string;
  highlighted?: boolean;
  highlightQuery?: string | null;
  highlightSnippet?: string | null;
  onEventContentLoaded: (content: ConversationEventContentDto) => void;
}) {
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
  const { states: imageLoads, errors: imageErrors, run: runImageLoad } = useKeyedAsyncLoad<string>();
  const [thumbnailUrls, setThumbnailUrls] = useState<Record<string, ObjectUrlEntry>>({});
  const requestedThumbnails = useRef(new Map<string, string>());
  const [openImage, setOpenImage] = useState<{ name: string; url: string } | null>(null);
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

  async function loadFullEvent() {
    await runEventLoad(
      event.event_id,
      () =>
        invoke<ConversationEventContentDto>("get_conversation_event_content", {
          source,
          sessionId,
          eventId: event.event_id,
        }),
      onEventContentLoaded,
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
    for (const attachment of event.attachments) {
      if (
        attachment.kind === "image" &&
        (attachment.status === "available" || attachment.status === "embedded")
      ) {
        void loadThumbnail(attachment);
      }
    }
  }, [event.attachments, loadThumbnail]);

  async function loadImage(attachment: ConversationAttachment) {
    const requestKey = attachmentRequestKey(attachment);
    await runImageLoad(
      requestKey,
      () =>
        invoke<ConversationAttachmentContentDto>("get_conversation_attachment", {
          source,
          sessionId,
          attachmentId: attachment.id,
        }),
      (result) => {
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

  const label = EVENT_LABELS[event.kind];
  const showDetails =
    event.kind === "unadapted" ||
    ((event.kind === "plan" || event.kind === "tool_call" || event.kind === "tool_result") &&
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
  const eventClass = [
    "conversation-event",
    `event-${event.kind.replaceAll("_", "-")}`,
    highlighted ? "conversation-event-hit" : "",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <>
      <article className={eventClass}>
        <header className="conversation-event-meta">
          <strong>{label}</strong>
          {event.occurred_at ? (
            <time dateTime={event.occurred_at}>{formatClock(event.occurred_at)}</time>
          ) : (
            <span className="conversation-event-missing-time">时间缺失</span>
          )}
        </header>
        <div className="conversation-event-content">
          {highlighted && highlightSnippet ? (
            <p className="conversation-event-match-snippet">
              命中：
              <HighlightedSnippet text={highlightSnippet} query={highlightQuery ?? ""} />
            </p>
          ) : null}
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
              {event.name ? (
                <code>
                  {highlighted && highlightQuery ? (
                    <HighlightedSnippet text={event.name} query={highlightQuery} />
                  ) : (
                    event.name
                  )}
                </code>
              ) : null}
            </div>
          ) : null}
          {event.text ? (
            usesMarkdown ? (
              <ConversationMarkdown markdown={event.text} />
            ) : (
              <pre className="conversation-event-text conversation-event-command">
                {highlighted && highlightQuery ? (
                  <HighlightedSnippet text={event.text} query={highlightQuery} />
                ) : (
                  event.text
                )}
              </pre>
            )
          ) : null}
          {isDeferred ? (
            <div className="conversation-deferred" aria-live="polite">
              <span>仅显示前部内容</span>
              <Button
                variant="text"
                onClick={() => void loadFullEvent()}
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
              <summary>{event.kind === "unadapted" ? "查看原始事件" : "查看详细数据"}</summary>
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
                        <span>{formatAttachmentBytes(attachment.size_bytes)}</span>
                        <span className={`attachment-status status-${attachment.status}`}>
                          {attachmentStatusText(attachment)}
                        </span>
                      </div>
                      {thumbnailState === "error" ? (
                        <div className="conversation-attachment-action">
                          <span className="conversation-inline-error" role="alert">
                            {thumbnailErrors[requestKey]}
                          </span>
                          <Button variant="text" onClick={() => void loadThumbnail(attachment, true)}>
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
                          aria-label={thumbnailState === "error" ? undefined : "正在生成缩略图"}
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
      {openImage ? (
        <ConversationImageDialog name={openImage.name} url={openImage.url} onClose={closeImage} />
      ) : null}
    </>
  );
}
