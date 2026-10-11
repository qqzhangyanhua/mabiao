import type { ContextItem, ContextManifest } from "../api/types";
import { groupByTier, TIER_LABEL, type ContextTier } from "../lib/context";
import { formatTokens } from "../lib/format";
import { Badge } from "./ui/badge";

const TIER_VARIANT: Record<
  ContextTier,
  "success" | "warning" | "secondary"
> = {
  injected: "success",
  injected_no_content: "success",
  reconstructed: "warning",
  cached: "secondary",
  on_disk: "warning",
  observed: "secondary",
};

function ItemRow({ item, tier }: { item: ContextItem; tier: ContextTier }) {
  return (
    <li className="border-b border-border/70 py-2 last:border-0">
      <div className="flex flex-wrap items-baseline gap-2">
        <span className="font-medium">{item.label}</span>
        <span className="text-xs text-muted-foreground">{item.kind}</span>
        {item.char_count !== undefined && (
          <span className="text-xs text-muted-foreground">{formatTokens(item.char_count)} 字符</span>
        )}
        {item.is_noise && <span className="text-xs text-muted-foreground">噪声</span>}
        {item.is_unused_install && <span className="text-xs text-muted-foreground">已安装未用</span>}
      </div>
      {item.path && (
        <div className="break-all text-xs text-muted-foreground" title="路径只说明位置，不代表当时加载过">
          {item.path}
        </div>
      )}
      {item.content !== undefined && tier === "injected" && (
        <details className="mt-1">
          <summary className="cursor-pointer text-xs text-primary">查看注入原文</summary>
          <pre className="mt-1 max-h-96 overflow-auto whitespace-pre-wrap rounded-md bg-muted p-2 text-xs">
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
    return <p className="py-4 text-sm text-muted-foreground">这场会话没有推送上下文清单</p>;
  }
  return (
    <div className="space-y-4">
      {manifest.from_cache && (
        <p className="rounded-md bg-muted px-3 py-2 text-xs text-muted-foreground">
          源快照已被清理，以下只是摄取时记下的度量，没有原文。
        </p>
      )}
      {!manifest.has_injected_snapshot && !manifest.from_cache && (
        <p className="rounded-md bg-warning/15 px-3 py-2 text-xs text-warning">
          没有会话当时的注入快照，以下按当前磁盘重建，不是当时实际注入的内容。
        </p>
      )}
      {manifest.volume_is_estimate && (
        <p className="text-xs text-muted-foreground">体积只是估算。</p>
      )}
      {groupByTier(manifest).map(({ tier, items }) => (
        <section key={tier}>
          <h3 className="mb-1 flex items-center gap-2 text-sm font-medium">
            <Badge variant={TIER_VARIANT[tier]}>{TIER_LABEL[tier]}</Badge>
            <span className="text-xs font-normal text-muted-foreground">{items.length} 项</span>
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
