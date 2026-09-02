import type { ConversationIndexProgressDto, ConversationPage, ConversationSessionRow } from "../types";
import {
  CATALOG_PAGE_SIZE,
  conversationCatalogEmptyCopy,
  conversationCatalogItems,
  conversationIndexIncomplete,
} from "../lib/conversationCatalogItems";
import { SESSION_ENTRY_COPY } from "../lib/sessionEntryCopy";
import { ConversationCatalogRow } from "./ConversationCatalogRow";
import { EmptyState } from "./EmptyState";
import { LoadingOverlay } from "./LoadingOverlay";
import { Pagination } from "./Pagination";
import { Spinner } from "./Spinner";
import { SearchField } from "./ui/Field";
import { Segmented } from "./ui/Segmented";
import { Select } from "./ui/Select";

const FAILURE_OPTIONS = [
  { value: "all", label: "全部" },
  { value: "failed", label: "工具失败" },
] as const;

export function ConversationCatalog({
  searchInput,
  onSearchInput,
  search,
  page,
  onPage,
  pageData,
  loading,
  error,
  indexProgress,
  toolNames,
  toolNameOptions,
  toolFailed,
  onToolNames,
  onToolFailed,
  onOpen,
}: {
  searchInput: string;
  onSearchInput: (value: string) => void;
  search: string;
  page: number;
  onPage: (page: number) => void;
  pageData: ConversationPage;
  loading: boolean;
  error: string | null;
  indexProgress: ConversationIndexProgressDto | null;
  toolNames: string[];
  toolNameOptions: string[];
  toolFailed: boolean;
  onToolNames: (names: string[]) => void;
  onToolFailed: (failed: boolean) => void;
  onOpen: (row: ConversationSessionRow) => void;
}) {
  const { rows, total } = pageData;
  const pageCount = Math.max(1, Math.ceil(total / CATALOG_PAGE_SIZE));
  const maxTotal = Math.max(1, ...rows.map((row) => row.total_tokens));
  const searching = search.length > 0;
  const indexIncomplete = conversationIndexIncomplete(indexProgress);
  const catalogItems = conversationCatalogItems(rows, searching);
  const emptyCopy = conversationCatalogEmptyCopy({
    searching,
    query: search,
    indexIncomplete,
  });

  return (
    <section className="panel conversation-catalog">
      <div className="panel-head conversation-catalog-head">
        <div>
          <h2>本地会话目录</h2>
          <p className="panel-note">{SESSION_ENTRY_COPY.conversationCatalogNote}</p>
        </div>
        <div className="conversation-catalog-filters">
          <SearchField
            value={searchInput}
            onChange={onSearchInput}
            placeholder="搜索标题、正文、来源、项目、模型、ID 或时间"
            ariaLabel="搜索对话记录"
          />
          <Select
            ariaLabel="工具名"
            value={toolNames[0] ?? ""}
            options={[
              { value: "", label: "全部工具" },
              ...toolNameOptions.map((name) => ({ value: name, label: name })),
            ]}
            onChange={(value) => onToolNames(value ? [value] : [])}
          />
          <Segmented
            ariaLabel="工具失败"
            value={toolFailed ? "failed" : "all"}
            options={FAILURE_OPTIONS}
            onChange={(value) => onToolFailed(value === "failed")}
          />
        </div>
        <span className="muted conversation-total">
          共 {total} 条
          {loading ? (
            <span className="inline-loading">
              <Spinner size={12} />
              加载中…
            </span>
          ) : null}
        </span>
      </div>
      {indexProgress && indexIncomplete ? (
        <p className="conversation-index-progress" role="status">
          正文索引补建中 {indexProgress.indexed} / {indexProgress.total}
          {searching ? "。未就绪会话只搜标题，结果可能不全。" : "。未就绪会话目前只搜标题。"}
        </p>
      ) : null}

      {error && rows.length === 0 ? (
        <div role="alert">
          <EmptyState icon="alertTriangle" tone="warn" title="无法加载对话目录" hint={error} />
        </div>
      ) : (
        <LoadingOverlay
          active={loading && rows.length > 0}
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
              {catalogItems.map((item) =>
                item.type === "heading" ? (
                  <tr key={`heading-${item.field}`} className="conversation-catalog-group-row">
                    <td colSpan={9}>{item.field === "title" ? "标题命中" : "正文命中"}</td>
                  </tr>
                ) : (
                  <ConversationCatalogRow
                    key={`${item.row.source}-${item.row.session_id}`}
                    row={item.row}
                    maxTotal={maxTotal}
                    searching={searching}
                    highlightQuery={search}
                    onOpen={onOpen}
                  />
                ),
              )}
              {rows.length === 0 ? (
                <tr>
                  <td colSpan={9} className="analytics-empty">
                    {loading ? (
                      <EmptyState icon="chat" title="正在加载对话目录…" />
                    ) : (
                      <EmptyState icon="chat" title={emptyCopy.title} hint={emptyCopy.hint} />
                    )}
                  </td>
                </tr>
              ) : null}
            </tbody>
          </table>
        </LoadingOverlay>
      )}
      <Pagination page={page} pageCount={pageCount} totalCount={total} onPageChange={onPage} />
    </section>
  );
}
