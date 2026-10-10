import type { ContextItem, ContextManifest } from "../api/types";
import { groupByTier, TIER_LABEL, type ContextTier } from "../lib/context";
import { formatTokens } from "../lib/format";

const TIER_STYLE: Record<ContextTier, string> = {
  injected: "bg-green-100 text-green-800",
  injected_no_content: "bg-green-50 text-green-800",
  reconstructed: "bg-amber-100 text-amber-800",
  cached: "bg-slate-100 text-slate-600",
  on_disk: "bg-amber-100 text-amber-800",
  observed: "bg-slate-100 text-slate-600",
};

function ItemRow({ item, tier }: { item: ContextItem; tier: ContextTier }) {
  return (
    <li className="border-b border-slate-100 py-2 last:border-0">
      <div className="flex flex-wrap items-baseline gap-2">
        <span className="font-medium">{item.label}</span>
        <span className="text-xs text-slate-500">{item.kind}</span>
        {item.char_count !== undefined && (
          <span className="text-xs text-slate-500">{formatTokens(item.char_count)} 字符</span>
        )}
        {item.is_noise && <span className="text-xs text-slate-500">噪声</span>}
        {item.is_unused_install && <span className="text-xs text-slate-500">已安装未用</span>}
      </div>
      {item.path && (
        <div className="break-all text-xs text-slate-500" title="路径只说明位置，不代表当时加载过">
          {item.path}
        </div>
      )}
      {item.content !== undefined && tier === "injected" && (
        <details className="mt-1">
          <summary className="cursor-pointer text-xs text-blue-700">查看注入原文</summary>
          <pre className="mt-1 max-h-96 overflow-auto whitespace-pre-wrap rounded bg-slate-50 p-2 text-xs">
            {item.content}
          </pre>
        </details>
      )}
    </li>
  );
}

/** 上下文清单按证据层级分组：已注入原文、磁盘可能生效、来自缓存无原文，各说各的。 */
export function ContextManifestView({ manifest }: { manifest: ContextManifest | null }) {
  if (!manifest || manifest.items.length === 0) {
    return <p className="py-4 text-sm text-slate-500">这场会话没有推送上下文清单</p>;
  }
  return (
    <div className="space-y-4">
      {manifest.from_cache && (
        <p className="rounded bg-slate-50 px-3 py-2 text-xs text-slate-600">
          源快照已被清理，以下只是摄取时记下的度量，没有原文。
        </p>
      )}
      {!manifest.has_injected_snapshot && !manifest.from_cache && (
        <p className="rounded bg-amber-50 px-3 py-2 text-xs text-amber-800">
          没有会话当时的注入快照，以下按当前磁盘重建，不是当时实际注入的内容。
        </p>
      )}
      {manifest.volume_is_estimate && (
        <p className="text-xs text-slate-500">体积只是估算。</p>
      )}
      {groupByTier(manifest).map(({ tier, items }) => (
        <section key={tier}>
          <h3 className="mb-1 flex items-center gap-2 text-sm font-medium text-slate-700">
            <span className={`rounded px-2 py-0.5 text-xs ${TIER_STYLE[tier]}`}>
              {TIER_LABEL[tier]}
            </span>
            <span className="text-xs font-normal text-slate-500">{items.length} 项</span>
          </h3>
          <ul>
            {items.map((item) => (
              <ItemRow key={item.id} item={item} tier={tier} />
            ))}
          </ul>
        </section>
      ))}
    </div>
  );
}
