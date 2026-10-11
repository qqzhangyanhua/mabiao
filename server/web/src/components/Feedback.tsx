export function ErrorNote({ error, onRetry }: { error: Error; onRetry?: () => void }) {
  return (
    <div
      role="alert"
      className="rounded-lg border border-destructive/30 bg-destructive/10 px-4 py-3 text-sm text-destructive"
    >
      {error.message}
      {onRetry && (
        <button type="button" onClick={onRetry} className="ml-3 underline">
          重试
        </button>
      )}
    </div>
  );
}

export function Loading() {
  return <p className="py-8 text-center text-sm text-muted-foreground">加载中…</p>;
}

export function Empty({ children }: { children: string }) {
  return <p className="py-8 text-center text-sm text-muted-foreground">{children}</p>;
}
