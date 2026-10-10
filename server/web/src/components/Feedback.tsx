export function ErrorNote({ error, onRetry }: { error: Error; onRetry?: () => void }) {
  return (
    <div
      role="alert"
      className="rounded border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-800"
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
  return <p className="py-8 text-center text-sm text-slate-500">加载中…</p>;
}

export function Empty({ children }: { children: string }) {
  return <p className="py-8 text-center text-sm text-slate-500">{children}</p>;
}
