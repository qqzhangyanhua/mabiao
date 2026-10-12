import { Button } from "./ui/button";

/** 表格底部分页，成员/项目会话列表共用。 */
export function TablePager({
  page,
  pageCount,
  onPageChange,
}: {
  page: number;
  pageCount: number;
  onPageChange: (page: number) => void;
}) {
  if (pageCount <= 1) return null;
  return (
    <div className="mt-3 flex items-center justify-end gap-3 text-sm">
      <Button
        type="button"
        variant="outline"
        size="sm"
        disabled={page === 0}
        onClick={() => onPageChange(page - 1)}
      >
        上一页
      </Button>
      <span className="tabular-nums text-muted-foreground">
        {page + 1} / {pageCount}
      </span>
      <Button
        type="button"
        variant="outline"
        size="sm"
        disabled={page + 1 >= pageCount}
        onClick={() => onPageChange(page + 1)}
      >
        下一页
      </Button>
    </div>
  );
}
