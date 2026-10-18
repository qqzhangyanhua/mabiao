import { useState, type ReactNode } from "react";
import { Icon } from "../icons";
import { chatStepCount, type ChatPart, type ChatToolStep, type ChatTurn } from "../lib/conversationChat";
import { actorLabel, compactionBoundaryLabel } from "../lib/conversationEventDisplay";
import { conversationSourceLabel } from "../lib/conversationDisplay";
import { formatClock } from "../lib/format";
import type { ConversationAgentLink, ConversationEventContentDto } from "../types";
import { ConversationEventItem } from "./ConversationEventItem";
import { SourceIcon } from "./SourceIcon";
import { UserAvatar } from "./UserAvatar";

function clockLabel(iso: string | null): string {
  const full = formatClock(iso);
  const time = full.split(" ")[1];
  return time || full;
}

function stepNames(steps: ChatToolStep[]): string {
  const names = steps.map((step) => step.call.name || step.result?.name).filter(Boolean);
  return names.slice(0, 3).join("、");
}

function ToolRun({
  steps,
  source,
  sessionId,
  highlightEventId,
  highlightQuery,
  highlightSnippet,
  onEventContentLoaded,
}: {
  steps: ChatToolStep[];
  source: string;
  sessionId: string;
  highlightEventId: string | null;
  highlightQuery: string | null;
  highlightSnippet: string | null;
  onEventContentLoaded: (content: ConversationEventContentDto) => void;
}) {
  const names = stepNames(steps);
  return (
    <details className="conversation-chat-op">
      <summary>
        <span className="conversation-chat-check" aria-hidden>
          <Icon name="check" size={12} />
        </span>
        <span>完成了 {steps.length} 步操作</span>
        {names ? <span className="muted">{names}</span> : null}
        <Icon name="chevron" size={12} className="conversation-chat-chevron" />
      </summary>
      <div className="conversation-chat-op-body">
        {steps.map((step) => (
          <div key={step.call.event_id} className="conversation-chat-op-step">
            <strong>{step.call.name || "操作"}</strong>
            <ConversationEventItem
              event={step.call}
              source={source}
              sessionId={sessionId}
              chrome="body"
              highlighted={step.call.event_id === highlightEventId}
              highlightQuery={step.call.event_id === highlightEventId ? highlightQuery : null}
              highlightSnippet={step.call.event_id === highlightEventId ? highlightSnippet : null}
              onEventContentLoaded={onEventContentLoaded}
            />
            {step.result ? (
              <ConversationEventItem
                event={step.result}
                source={source}
                sessionId={sessionId}
                chrome="body"
                highlighted={step.result.event_id === highlightEventId}
                highlightQuery={step.result.event_id === highlightEventId ? highlightQuery : null}
                highlightSnippet={step.result.event_id === highlightEventId ? highlightSnippet : null}
                onEventContentLoaded={onEventContentLoaded}
              />
            ) : null}
          </div>
        ))}
      </div>
    </details>
  );
}

function PartBody({
  part,
  source,
  sessionId,
  highlightEventId,
  highlightQuery,
  highlightSnippet,
  onEventContentLoaded,
}: {
  part: ChatPart;
  source: string;
  sessionId: string;
  highlightEventId: string | null;
  highlightQuery: string | null;
  highlightSnippet: string | null;
  onEventContentLoaded: (content: ConversationEventContentDto) => void;
}) {
  if (part.type === "tools") {
    return (
      <ToolRun
        steps={part.steps}
        source={source}
        sessionId={sessionId}
        highlightEventId={highlightEventId}
        highlightQuery={highlightQuery}
        highlightSnippet={highlightSnippet}
        onEventContentLoaded={onEventContentLoaded}
      />
    );
  }
  if (part.type === "boundary") {
    return (
      <div className="conversation-compaction-boundary" role="separator">
        <span>{compactionBoundaryLabel(part.event)}</span>
      </div>
    );
  }
  const event = part.event;
  return (
    <div className={part.type === "notice" ? "conversation-chat-notice" : undefined}>
      {part.type === "notice" ? (
        <span className={`conversation-chat-notice-label kind-${event.kind}`}>
          {event.kind === "error"
            ? "错误"
            : event.kind === "plan"
              ? "计划"
              : event.kind === "model_change"
                ? "切换模型"
                : "状态"}
        </span>
      ) : null}
      <ConversationEventItem
        event={event}
        source={source}
        sessionId={sessionId}
        chrome="body"
        highlighted={event.event_id === highlightEventId}
        highlightQuery={event.event_id === highlightEventId ? highlightQuery : null}
        highlightSnippet={event.event_id === highlightEventId ? highlightSnippet : null}
        onEventContentLoaded={onEventContentLoaded}
      />
    </div>
  );
}

export function ConversationChatTurn({
  turn,
  source,
  sessionId,
  highlightEventId,
  highlightQuery,
  highlightSnippet,
  onEventContentLoaded,
  linksFor,
  renderLinks,
}: {
  turn: ChatTurn;
  source: string;
  sessionId: string;
  highlightEventId: string | null;
  highlightQuery: string | null;
  highlightSnippet: string | null;
  onEventContentLoaded: (content: ConversationEventContentDto) => void;
  linksFor: (eventId: string) => ConversationAgentLink[];
  renderLinks: (links: ConversationAgentLink[]) => ReactNode;
}) {
  const steps = chatStepCount(turn);
  const [stepsOpen, setStepsOpen] = useState(true);
  const speaker =
    turn.role === "user"
      ? actorLabel("user")
      : turn.role === "system"
        ? "压缩边界"
        : conversationSourceLabel(source);
  const highlighted = turn.eventIds.includes(highlightEventId ?? "");
  const boundary = turn.role === "system" ? turn.parts.find((part) => part.type === "boundary") : null;

  if (boundary?.type === "boundary") {
    return (
      <div
        className={["conversation-chat-turn", "is-system", highlighted ? "is-hit" : ""]
          .filter(Boolean)
          .join(" ")}
      >
        <div className="conversation-compaction-boundary" role="separator" data-event-id={boundary.event.event_id}>
          <time dateTime={turn.occurredAt ?? undefined}>{clockLabel(turn.occurredAt)}</time>
          <span>{compactionBoundaryLabel(boundary.event)}</span>
        </div>
        {turn.eventIds.flatMap((eventId) => renderLinks(linksFor(eventId)))}
      </div>
    );
  }

  return (
    <article
      className={["conversation-chat-turn", `is-${turn.role}`, highlighted ? "is-hit" : ""]
        .filter(Boolean)
        .join(" ")}
    >
      <div className="conversation-chat-mark">
        {turn.role === "assistant" ? (
          <SourceIcon source={source} size={32} />
        ) : (
          <UserAvatar seed={sessionId} />
        )}
      </div>
      <div className="conversation-chat-main">
        <header className="conversation-chat-speaker">
          <strong>{speaker}</strong>
          <time dateTime={turn.occurredAt ?? undefined} title={formatClock(turn.occurredAt)}>
            {clockLabel(turn.occurredAt)}
          </time>
        </header>
        {steps > 0 ? (
          <button
            type="button"
            className="conversation-chat-fold"
            aria-expanded={stepsOpen}
            onClick={() => setStepsOpen((open) => !open)}
          >
            <Icon name="chevron" size={12} className="conversation-chat-chevron" />
            {stepsOpen ? `收起 ${steps} 个步骤` : `展开 ${steps} 个步骤`}
          </button>
        ) : null}
        <div className="conversation-chat-parts">
          {turn.parts.map((part, index) => {
            const eventIds = partEventIds(part);
            const links = eventIds.flatMap((eventId) => linksFor(eventId));
            if (part.type === "tools" && !stepsOpen) {
              return links.length > 0 ? <div key={eventIds[0] ?? index}>{renderLinks(links)}</div> : null;
            }
            return (
              <div
                key={eventIds[0] ?? index}
                className={part.type === "prose" ? "conversation-chat-bubble" : undefined}
              >
                <PartBody
                  part={part}
                  source={source}
                  sessionId={sessionId}
                  highlightEventId={highlightEventId}
                  highlightQuery={highlightQuery}
                  highlightSnippet={highlightSnippet}
                  onEventContentLoaded={onEventContentLoaded}
                />
                {renderLinks(links)}
              </div>
            );
          })}
        </div>
      </div>
    </article>
  );
}

function partEventIds(part: ChatPart): string[] {
  if (part.type === "tools") {
    return part.steps.flatMap((step) =>
      step.result ? [step.call.event_id, step.result.event_id] : [step.call.event_id],
    );
  }
  return [part.event.event_id];
}
