import { useLayoutEffect, useState } from "react";
import { Icon } from "../icons";
import type { ConversationDetailTab } from "../lib/conversationNavigation";
import type { ConversationTimelineFollow } from "../lib/useConversationTimelineFollow";
import type {
  ConversationAgentLink,
  ConversationDetailDto,
  ConversationSessionRow,
} from "../types";
import { ConversationDetailHead } from "./ConversationDetailHead";
import { ConversationJumpBar } from "./ConversationJumpBar";
import { ConversationTimeline } from "./ConversationTimeline";
import { ConversationUsageTable } from "./ConversationUsageTable";
import { CursorSessionDetail } from "./CursorSessionDetail";
import { EmptyState } from "./EmptyState";
import type { ConversationExportFormat } from "./type";
import { Button } from "./ui/Button";
import { Segmented } from "./ui/Segmented";

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

export function ConversationDetailView({
  session,
  detail,
  detailTab,
  detailLoading,
  detailError,
  detailFileAvailable,
  pollError,
  breadcrumb,
  parentAvailable,
  expandedRelationshipIds,
  matchFocus,
  usageIdentity,
  exportFormat,
  exportStatus,
  exportError,
  follow,
  onBack,
  onExport,
  onTabChange,
  onRetry,
  onToggleChild,
  onOpenChild,
  onError,
}: {
  session: ConversationSessionRow;
  detail: ConversationDetailDto | null;
  detailTab: ConversationDetailTab;
  detailLoading: boolean;
  detailError: string | null;
  detailFileAvailable: boolean;
  pollError: string | null;
  breadcrumb: string | null;
  parentAvailable: boolean;
  expandedRelationshipIds: string[];
  matchFocus: {
    eventId: string;
    sequence: number;
    snippet: string | null;
    query: string;
  } | null;
  usageIdentity: string;
  exportFormat: ConversationExportFormat | null;
  exportStatus: string | null;
  exportError: boolean;
  follow: ConversationTimelineFollow;
  onBack: () => void;
  onExport: (format: ConversationExportFormat) => void;
  onTabChange: (tab: ConversationDetailTab) => void;
  onRetry: () => void;
  onToggleChild: (link: ConversationAgentLink) => void;
  onOpenChild: (link: ConversationAgentLink) => void;
  onError?: (error: unknown) => void;
}) {
  const [usageIdentitySeen, setUsageIdentitySeen] = useState("");
  const [usageTotal, setUsageTotal] = useState<number | null>(null);
  const pinTimelineLayout = follow.pinTimelineLayout;
  if (usageIdentity !== usageIdentitySeen) {
    setUsageIdentitySeen(usageIdentity);
    setUsageTotal(null);
  }

  useLayoutEffect(() => {
    if (!detail || detailTab !== "events") {
      return;
    }
    return pinTimelineLayout();
  }, [detail, detailTab, pinTimelineLayout]);

  return (
    <div className="conversation-detail-view">
      <ConversationDetailHead
        session={session}
        fileAvailable={detailFileAvailable}
        breadcrumb={breadcrumb}
        parentAvailable={parentAvailable}
        exportFormat={exportFormat}
        exportStatus={exportStatus}
        exportError={exportError}
        exportDisabled={!detailFileAvailable || !detail}
        onBack={onBack}
        onExport={onExport}
      />

      <section className="conversation-detail-body" aria-busy={detailLoading}>
        <div className="conversation-detail-tabs">
          <Segmented
            value={detailTab}
            options={detail?.cursor_behavior ? [...DETAIL_TABS, BEHAVIOR_TAB] : DETAIL_TABS}
            disabled={detailLoading || Boolean(detailError)}
            ariaLabel="对话详情视图"
            onChange={onTabChange}
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
            <Button onClick={onRetry}>重新读取</Button>
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
                key={`${session.source}:${session.session_id}:${matchFocus?.eventId ?? ""}`}
                source={session.source}
                sessionId={session.session_id}
                revision={detail.revision}
                eventCount={detail.event_count}
                agentLinks={detail.agent_relations.children}
                expandedRelationshipIds={expandedRelationshipIds}
                followLatest={follow.atBottom}
                initialSequence={matchFocus?.sequence ?? null}
                highlightEventId={matchFocus?.eventId ?? null}
                highlightQuery={matchFocus?.query ?? null}
                highlightSnippet={matchFocus?.snippet ?? null}
                onToggleChild={onToggleChild}
                onOpenChild={onOpenChild}
                timelineRef={follow.timelineRef}
                timelineApiRef={follow.timelineApiRef}
                onScroll={follow.handleTimelineScroll}
                onWindowChange={follow.handleWindowChange}
                onCaptureScrollAnchor={follow.captureTimelineAnchor}
              />
              <ConversationJumpBar
                atTop={follow.atTop}
                atBottom={follow.atBottom}
                unseenCount={follow.unseenCount}
                onJumpTop={() => void follow.jumpTimeline("top")}
                onJumpBottom={() => void follow.jumpTimeline("bottom")}
              />
            </div>
          )
        ) : null}
      </section>
    </div>
  );
}
