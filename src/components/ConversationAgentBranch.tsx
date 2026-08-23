import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { Icon } from "../icons";
import { conversationKey } from "../lib/conversationCache";
import { humanStatus } from "../lib/format";
import type {
  ConversationAgentLink,
  ConversationDetailDto,
  ConversationDetailStateDto,
} from "../types";
import { ConversationTimeline } from "./ConversationTimeline";
import { Spinner } from "./Spinner";
import { Button } from "./ui/Button";

const AGENT_LINK_LABELS = {
  linked: "已关联",
  missing_source: "子会话源不可用",
  unresolved: "无法确定子会话",
  conflict: "关联冲突",
  cycle: "循环关联",
} as const;

export function ConversationAgentBranch({
  link,
  expanded,
  expandedRelationshipIds,
  depth,
  onToggleChild,
  onOpenChild,
}: {
  link: ConversationAgentLink;
  expanded: boolean;
  expandedRelationshipIds: string[];
  depth: number;
  onToggleChild: (link: ConversationAgentLink) => void;
  onOpenChild: (link: ConversationAgentLink) => void;
}) {
  const linkedSession = link.status === "linked" ? link.session : null;
  const controlsId = `agent-timeline-${link.relationship_id.replaceAll(/[^a-zA-Z0-9_-]/g, "-")}`;

  return (
    <section
      className={`conversation-agent-link depth-${Math.min(depth, 3)} status-${link.status}`}
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
          <NestedConversationTimeline
            source={linkedSession.source}
            sessionId={linkedSession.session_id}
            expandedRelationshipIds={expandedRelationshipIds}
            depth={depth}
            onToggleChild={onToggleChild}
            onOpenChild={onOpenChild}
          />
        </div>
      ) : null}
    </section>
  );
}

function NestedConversationTimeline({
  source,
  sessionId,
  expandedRelationshipIds,
  depth,
  onToggleChild,
  onOpenChild,
}: {
  source: string;
  sessionId: string;
  expandedRelationshipIds: string[];
  depth: number;
  onToggleChild: (link: ConversationAgentLink) => void;
  onOpenChild: (link: ConversationAgentLink) => void;
}) {
  const [childDetail, setChildDetail] = useState<ConversationDetailDto | null>(null);
  const [childError, setChildError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void invoke<ConversationDetailDto>("get_conversation_detail", {
      source,
      sessionId,
    })
      .then((detail) => {
        if (!cancelled) {
          setChildDetail(detail);
        }
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          setChildError(humanStatus(error));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [sessionId, source]);

  useEffect(() => {
    if (!childDetail) {
      return;
    }
    let cancelled = false;
    const poll = () => {
      void invoke<ConversationDetailStateDto>("get_conversation_detail_state", {
        source,
        sessionId,
        knownRevision: childDetail.revision,
      })
        .then((state) => {
          if (cancelled || !state.changed || !state.file_available) {
            return;
          }
          return invoke<ConversationDetailDto>("get_conversation_detail", {
            source,
            sessionId,
          }).then((detail) => {
            if (!cancelled) {
              setChildDetail(detail);
            }
          });
        })
        .catch((error: unknown) => {
          if (!cancelled) {
            setChildError(humanStatus(error));
          }
        });
    };
    const timer = window.setInterval(poll, 2_000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [childDetail, sessionId, source]);

  if (childError && !childDetail) {
    return <span className="conversation-inline-error">{childError}</span>;
  }
  if (!childDetail) {
    return (
      <div className="conversation-agent-loading">
        <Spinner size={14} />
      </div>
    );
  }
  return (
    <ConversationTimeline
      key={`${conversationKey(childDetail.session)}:${childDetail.revision}`}
      source={childDetail.session.source}
      sessionId={childDetail.session.session_id}
      revision={childDetail.revision}
      eventCount={childDetail.event_count}
      agentLinks={childDetail.agent_relations.children}
      expandedRelationshipIds={expandedRelationshipIds}
      depth={depth + 1}
      onToggleChild={onToggleChild}
      onOpenChild={onOpenChild}
    />
  );
}
