import { useLayoutEffect } from "react";
import { CollapsibleSection } from "./CollapsibleSection";
import {
  contextCompletenessNote,
  contextInjectedDegraded,
  contextItemCharText,
  contextItemKey,
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
  highlightItemKey = null,
}: {
  manifest: ConversationContextManifest;
  highlightItemKey?: string | null;
}) {
  const mcpSummary = contextMcpInitSummary(manifest);
  const completeness = contextCompletenessNote(manifest);
  const volumeIsEstimate = manifest.volume_is_estimate === true;
  useLayoutEffect(() => {
    if (!highlightItemKey) {
      return;
    }
    const node = document.querySelector(
      `[data-context-item-key="${CSS.escape(highlightItemKey)}"]`,
    );
    node?.scrollIntoView({ block: "nearest" });
  }, [highlightItemKey]);
  return (
    <CollapsibleSection
      sectionId="conversation-context-manifest"
      title="上下文清单"
      defaultOpen={false}
      forceOpen={Boolean(highlightItemKey)}
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
            highlightItemKey={highlightItemKey}
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
  highlightItemKey,
}: {
  layer: ConversationContextLayer;
  items: ConversationContextItem[];
  note?: string | null;
  manifest: ConversationContextManifest;
  volumeIsEstimate: boolean;
  highlightItemKey: string | null;
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
        <ContextItemGroups
          items={items}
          volumeIsEstimate={volumeIsEstimate}
          highlightItemKey={highlightItemKey}
        />
      )}
      {items.length > 0 && note ? <p className="muted conversation-context-note">{note}</p> : null}
    </section>
  );
}

function ContextItemGroups({
  items,
  volumeIsEstimate,
  highlightItemKey,
}: {
  items: ConversationContextItem[];
  volumeIsEstimate: boolean;
  highlightItemKey: string | null;
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
              highlightItemKey={highlightItemKey}
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
                highlightItemKey={highlightItemKey}
              />
            ))}
          </ul>
        </div>
      ) : null}
      {editorBuiltin.length > 0 ? (
        <details
          className="conversation-context-builtin"
          {...(editorBuiltin.some(
            (item) => contextItemKey(item.layer, item.kind, item.id) === highlightItemKey,
          )
            ? { open: true }
            : {})}
        >
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
                highlightItemKey={highlightItemKey}
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
                highlightItemKey={highlightItemKey}
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
  highlightItemKey,
}: {
  item: ConversationContextItem;
  volumeIsEstimate: boolean;
  highlightItemKey: string | null;
}) {
  const meta = contextItemMetaText(item, { volumeIsEstimate });
  const itemKey = contextItemKey(item.layer, item.kind, item.id);
  const focused = itemKey === highlightItemKey;
  return (
    <li
      className={[item.is_noise ? "is-noise" : "", focused ? "is-focus" : ""]
        .filter(Boolean)
        .join(" ") || undefined}
      data-context-item-key={itemKey}
    >
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
