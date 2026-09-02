import { sourceLabel, formatTokens } from "../lib/format";
import type { IngestIssue, IngestReport, SourceDiagnostic } from "../types";
import { SourceLabel } from "./SourceIcon";
import { Button } from "./ui/Button";

function IngestIssueList({ title, issues }: { title: string; issues: IngestIssue[] }) {
  if (issues.length === 0) {
    return null;
  }
  return (
    <div className="ingest-issues" role="status">
      <strong>{title}</strong>
      <ul>
        {issues.slice(0, 8).map((issue, index) => (
          <li key={`${issue.source}-${issue.path}-${index}`}>
            <SourceLabel source={issue.source} size={14} />
            <code title={issue.path}>{issue.path}</code>
            <em>{issue.message}</em>
          </li>
        ))}
      </ul>
    </div>
  );
}

export function SourceDiagnosticsPanel({
  diagnostics,
  ingestReport,
  rebuilding,
  purging,
  operationBusy,
  onRebuild,
  onPurgeArchived,
}: {
  diagnostics: SourceDiagnostic[];
  ingestReport: IngestReport | null;
  rebuilding: string | null;
  purging: string | null;
  operationBusy: boolean;
  onRebuild: (source: string | null) => void;
  onPurgeArchived: (source: string | null) => void;
}) {
  const totalArchived = diagnostics.reduce((sum, row) => sum + row.archived_record_count, 0);

  return (
    <section className="panel" id="settings-diagnostics">
      <div className="panel-head">
        <div>
          <h2>数据源健康</h2>
          <p className="panel-note">
            只展示扫描状态和用量元数据，不读取或保存会话正文。关闭窗口后应用会留在菜单栏，显示今日花费。
            安装位置非默认路径时，优先在上方「扫描路径」填写绝对路径（从 Dock 打开也能生效）。
            环境变量（如 <code>CODEX_HOME</code>、<code>CLAUDE_CONFIG_DIR</code>，逗号分隔可指定多个目录）仍然可用，设置页未填时才会用到。
            源文件被工具自身清理后，对应记录会转为「已归档」但仍计入统计，不会静默消失。
          </p>
        </div>
        <div className="row-actions">
          {totalArchived > 0 ? (
            <Button
              variant="danger"
              disabled={operationBusy || purging !== null}
              onClick={() => onPurgeArchived(null)}
              title="永久删除所有来源已归档的记录，此操作不可撤销"
            >
              {purging === "all" ? "正在清理…" : `清理全部已归档（${formatTokens(totalArchived)}）`}
            </Button>
          ) : null}
          <Button disabled={operationBusy || rebuilding !== null} onClick={() => onRebuild(null)}>
            {rebuilding === "all" ? "正在重建…" : "重建全部缓存"}
          </Button>
        </div>
      </div>
      <div className="table-scroll">
        <table>
          <thead>
            <tr>
              <th>来源</th>
              <th>状态</th>
              <th>统计口径</th>
              <th>缓存文件</th>
              <th>记录</th>
              <th>已归档</th>
              <th>Token</th>
              <th>扫描位置</th>
              <th>操作</th>
            </tr>
          </thead>
          <tbody>
            {diagnostics.map((row) => (
              <tr key={row.source}>
                <td>
                  <strong>
                    <SourceLabel
                      source={row.source}
                      fallback={row.application || sourceLabel(row.source)}
                    />
                  </strong>
                </td>
                <td>
                  <span className={row.detected ? "health-state ok" : "health-state"}>
                    {row.detected ? "已检测" : "未检测"}
                  </span>
                </td>
                <td>{row.coverage}</td>
                <td>{formatTokens(row.cached_files)}</td>
                <td>{formatTokens(row.record_count)}</td>
                <td>
                  {row.archived_record_count > 0 ? (
                    <span title="源文件已被工具自身清理，记录仍计入统计">
                      {formatTokens(row.archived_record_count)}
                    </span>
                  ) : (
                    "—"
                  )}
                </td>
                <td>{formatTokens(row.total_tokens)}</td>
                <td className="mono" title={row.root_path}>
                  {row.root_path}
                </td>
                <td className="row-actions">
                  <Button
                    size="sm"
                    disabled={operationBusy || rebuilding !== null || !row.detected}
                    onClick={() => onRebuild(row.source)}
                  >
                    {rebuilding === row.source ? "重建中…" : "重建"}
                  </Button>
                  {row.archived_record_count > 0 ? (
                    <Button
                      variant="danger"
                      size="sm"
                      disabled={operationBusy || purging !== null}
                      onClick={() => onPurgeArchived(row.source)}
                      title="永久删除该来源已归档的记录，此操作不可撤销"
                    >
                      {purging === row.source ? "清理中…" : "清理归档"}
                    </Button>
                  ) : null}
                </td>
              </tr>
            ))}
            {diagnostics.length === 0 ? (
              <tr>
                <td colSpan={9} className="analytics-empty">
                  正在读取来源状态…
                </td>
              </tr>
            ) : null}
          </tbody>
        </table>
      </div>
      {ingestReport ? (
        <>
          <IngestIssueList
            title={`本次摄取有 ${ingestReport.issues.length} 个文件保留了上次正确缓存`}
            issues={ingestReport.issues}
          />
          <IngestIssueList
            title={`对话索引有 ${ingestReport.conversation_issues.length} 个文件保留了上次正确元数据`}
            issues={ingestReport.conversation_issues}
          />
        </>
      ) : null}
    </section>
  );
}
