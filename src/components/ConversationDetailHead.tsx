import { Icon } from "../icons";
import {
  capabilityLabel,
  conversationSourceLabel,
  conversationDetailSummary,
  conversationFileUnavailableLabel,
  conversationStatusLabel,
} from "../lib/conversationDisplay";
import { formatClock, projectLabel } from "../lib/format";
import { CollapsibleSection } from "./CollapsibleSection";
import { SessionResumeCommand } from "./SessionResumeCommand";
import { SourceLabel } from "./SourceIcon";
import { Spinner } from "./Spinner";
import type { ConversationDetailHeadProps } from "./type";
import { Button } from "./ui/Button";

const META_SECTION_ID = "conversation-detail";

export function ConversationDetailHead({
  session,
  fileAvailable,
  breadcrumb,
  parentAvailable,
  exportFormat,
  exportStatus,
  exportError,
  exportDisabled,
  onBack,
  onExport,
}: ConversationDetailHeadProps) {
  const exporting = exportFormat !== null;
  const cannotExport = exporting || exportDisabled;

  return (
    <section className="panel conversation-detail-head">
      <div className="conversation-detail-actions">
        <div className="conversation-detail-navigation">
          <Button onClick={onBack} size="sm">
            <Icon name="chevron" size={13} />
            {parentAvailable ? "返回父会话" : "返回目录"}
          </Button>
          {breadcrumb ? <span className="conversation-breadcrumb">{breadcrumb}</span> : null}
          <div className="conversation-detail-statuses">
            <span className={`conversation-status status-${session.support_status}`}>
              {conversationStatusLabel(session.support_status)}
            </span>
            {fileAvailable ? null : (
              <span className="conversation-file-unavailable">
                <Icon name="alertTriangle" size={12} />
                {conversationFileUnavailableLabel(session.source)}
              </span>
            )}
          </div>
        </div>
        <div className="conversation-export-wrap">
          <div className="conversation-export-actions" aria-label="导出会话">
            <Icon name="download" size={14} />
            <Button variant="text" onClick={() => onExport("markdown")} disabled={cannotExport}>
              {exportFormat === "markdown" ? <Spinner size={12} /> : null}
              Markdown
            </Button>
            <Button variant="text" onClick={() => onExport("json")} disabled={cannotExport}>
              {exportFormat === "json" ? <Spinner size={12} /> : null}
              JSON
            </Button>
          </div>
          <span
            className={
              exportError ? "conversation-export-status is-error" : "conversation-export-status"
            }
            role={exportError ? "alert" : "status"}
            aria-live={exportError ? "assertive" : "polite"}
          >
            {exportStatus}
          </span>
        </div>
      </div>
      <CollapsibleSection
        sectionId={META_SECTION_ID}
        title={session.title}
        defaultOpen={false}
        className="conversation-detail-meta-section"
        extra={
          <SourceLabel
            source={session.source}
            fallback={conversationSourceLabel(session.source)}
            size={14}
          />
        }
        collapsedSummary={conversationDetailSummary(session)}
      >
        <dl className="conversation-meta">
          <div>
            <dt>会话 ID</dt>
            <dd className="mono" title={session.session_id}>
              {session.session_id}
            </dd>
          </div>
          <div>
            <dt>项目</dt>
            <dd title={session.project}>{projectLabel(session.project)}</dd>
          </div>
          <div>
            <dt>模型</dt>
            <dd>{session.model || "未标注"}</dd>
          </div>
          <div>
            <dt>开始时间</dt>
            <dd>{formatClock(session.started_at)}</dd>
          </div>
          <div>
            <dt>结束时间</dt>
            <dd>{formatClock(session.ended_at)}</dd>
          </div>
          <div>
            <dt>可用能力</dt>
            <dd>
              {session.capabilities.length > 0
                ? session.capabilities.map(capabilityLabel).join("、")
                : "仅元数据"}
            </dd>
          </div>
          <div className="conversation-source-file">
            <dt>原始文件</dt>
            <dd className="mono conversation-source-files">
              {session.source_files.map((sourceFile) => (
                <span key={sourceFile}>{sourceFile}</span>
              ))}
            </dd>
          </div>
        </dl>
        <SessionResumeCommand source={session.source} sessionId={session.session_id} />
      </CollapsibleSection>
    </section>
  );
}
