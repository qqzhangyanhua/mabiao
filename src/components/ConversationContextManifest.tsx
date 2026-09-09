import { CollapsibleSection } from "./CollapsibleSection";
import {
  contextCompletenessNote,
  contextInjectedDegraded,
  contextItemCharText,
  contextItemMetaText,
  contextItemsTokenSummary,
  contextKindLabel,
  contextLayerBadge,
  contextLayerHint,
  contextLayerItems,
  contextLayerNote,
  contextLayerTitle,
  contextManifestSummary,
  contextMcpInitSummary,
  contextMetricsFromCache,
  partitionContextItems,
} from "../lib/conversationContext";
import type {
  ConversationContextItem,
  ConversationContextLayer,
  ConversationContextManifest,
} from "../types";

const LAYERS: ConversationContextLayer[] = ["injected", "observed", "on_disk_possible"];

export function ConversationContextManifestPanel({
  manifest,
}: {
  manifest: ConversationContextManifest;
}) {
  const mcpSummary = contextMcpInitSummary(manifest);
  const completeness = contextCompletenessNote(manifest);
  const volumeIsEstimate = manifest.volume_is_estimate === true;
  return (
    <CollapsibleSection
      sectionId="conversation-context-manifest"
      title="上下文清单"
      defaultOpen={false}
      className="conversation-context-manifest"
      collapsedSummary={contextManifestSummary(manifest)}
    >
      <p className="muted conversation-context-lead">
        复盘噪音用。已注入来自首轮快照；已观测来自本会话事件；磁盘项只表示可能生效。白装了是磁盘有、本轮没送进上下文的差集。
      </p>
      {mcpSummary ? <p className="conversation-context-mcp-summary">{mcpSummary}</p> : null}
      {completeness ? (
        <p className="conversation-context-completeness">{completeness}</p>
      ) : null}
      <div className="conversation-context-layers">
        {LAYERS.map((layer) => (
          <ContextLayer
            key={layer}
            layer={layer}
            items={contextLayerItems(manifest, layer)}
            note={contextLayerNote(manifest, layer)}
            manifest={manifest}
            volumeIsEstimate={volumeIsEstimate}
          />
        ))}
      </div>
    </CollapsibleSection>
  );
}

function ContextLayer({
  layer,
  items,
  note,
  manifest,
  volumeIsEstimate,
}: {
  layer: ConversationContextLayer;
  items: ConversationContextItem[];
  note?: string | null;
  manifest: ConversationContextManifest;
  volumeIsEstimate: boolean;
}) {
  const title = contextLayerTitle(layer, manifest);
  const degraded = contextInjectedDegraded(layer, manifest);
  const fromCache = layer === "injected" && contextMetricsFromCache(manifest);
  const stateClass = degraded ? " is-degraded" : fromCache ? " is-from-cache" : "";

  return (
    <section
      className={`conversation-context-layer layer-${layer}${stateClass}`}
      aria-label={title}
    >
      <header>
        <h3>{title}</h3>
        <span className={`conversation-context-badge layer-${layer}${stateClass}`}>
          {contextLayerBadge(layer, manifest)}
        </span>
      </header>
      <p className="muted">{contextLayerHint(layer, manifest)}</p>
      {items.length === 0 ? (
        <p className="conversation-context-empty" role="status">
          {note ?? "源文件未落盘，无法确认。"}
        </p>
      ) : (
        <ContextItemGroups items={items} volumeIsEstimate={volumeIsEstimate} />
      )}
      {items.length > 0 && note ? <p className="muted conversation-context-note">{note}</p> : null}
    </section>
  );
}

function ContextItemGroups({
  items,
  volumeIsEstimate,
}: {
  items: ConversationContextItem[];
  volumeIsEstimate: boolean;
}) {
  const { primary, unusedInstalls, editorBuiltin, disconnectedMcp } =
    partitionContextItems(items);
  const builtinSummary = contextItemsTokenSummary(editorBuiltin, volumeIsEstimate);
  return (
    <>
      {primary.length > 0 ? (
        <ul>
          {primary.map((item) => (
            <ContextItemRow
              key={`${item.layer}:${item.kind}:${item.id}`}
              item={item}
              volumeIsEstimate={volumeIsEstimate}
            />
          ))}
        </ul>
      ) : null}
      {unusedInstalls.length > 0 ? (
        <div className="conversation-context-unused">
          <p className="muted">白装了（磁盘有、本轮没送进上下文）</p>
          <ul>
            {unusedInstalls.map((item) => (
              <ContextItemRow
                key={`${item.layer}:${item.kind}:${item.id}`}
                item={item}
                volumeIsEstimate={volumeIsEstimate}
              />
            ))}
          </ul>
        </div>
      ) : null}
      {editorBuiltin.length > 0 ? (
        <details className="conversation-context-builtin">
          <summary>
            编辑器内置 · {editorBuiltin.length}
            {builtinSummary ? ` · ${builtinSummary}` : ""}
          </summary>
          <ul>
            {editorBuiltin.map((item) => (
              <ContextItemRow
                key={`${item.layer}:${item.kind}:${item.id}`}
                item={item}
                volumeIsEstimate={volumeIsEstimate}
              />
            ))}
          </ul>
        </details>
      ) : null}
      {disconnectedMcp.length > 0 ? (
        <div className="conversation-context-mcp-failed">
          <p className="muted">未连上（不占 token，不是噪音）</p>
          <ul>
            {disconnectedMcp.map((item) => (
              <ContextItemRow
                key={`${item.layer}:${item.kind}:${item.id}`}
                item={item}
                volumeIsEstimate={volumeIsEstimate}
              />
            ))}
          </ul>
        </div>
      ) : null}
    </>
  );
}

function ContextItemRow({
  item,
  volumeIsEstimate,
}: {
  item: ConversationContextItem;
  volumeIsEstimate: boolean;
}) {
  const meta = contextItemMetaText(item, { volumeIsEstimate });
  return (
    <li className={item.is_noise ? "is-noise" : undefined}>
      <span className="conversation-context-kind">{contextKindLabel(item.kind)}</span>
      <div>
        <strong>{item.label}</strong>
        {item.path ? <code>{item.path}</code> : null}
        {meta ? <span className="muted">{meta}</span> : null}
        {contextItemCharText(item) ? (
          <span className="muted">{contextItemCharText(item)}</span>
        ) : null}
      </div>
    </li>
  );
}
