import { CollapsibleSection } from "./CollapsibleSection";
import {
  CONTEXT_LAYER_BADGE,
  CONTEXT_LAYER_HINT,
  CONTEXT_LAYER_TITLE,
  contextItemCharText,
  contextItemMetaText,
  contextKindLabel,
  contextLayerItems,
  contextLayerNote,
  contextManifestSummary,
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
  return (
    <CollapsibleSection
      sectionId="conversation-context-manifest"
      title="上下文清单"
      defaultOpen={false}
      className="conversation-context-manifest"
      collapsedSummary={contextManifestSummary(manifest)}
    >
      <p className="muted conversation-context-lead">
        复盘噪音用。已注入来自首轮快照；已观测来自本会话事件；磁盘项只表示可能生效，不是本轮一定进了上下文。
      </p>
      <div className="conversation-context-layers">
        {LAYERS.map((layer) => (
          <ContextLayer
            key={layer}
            layer={layer}
            items={contextLayerItems(manifest, layer)}
            note={contextLayerNote(manifest, layer)}
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
}: {
  layer: ConversationContextLayer;
  items: ConversationContextItem[];
  note?: string | null;
}) {
  return (
    <section className={`conversation-context-layer layer-${layer}`} aria-label={CONTEXT_LAYER_TITLE[layer]}>
      <header>
        <h3>{CONTEXT_LAYER_TITLE[layer]}</h3>
        <span className={`conversation-context-badge layer-${layer}`}>
          {CONTEXT_LAYER_BADGE[layer]}
        </span>
      </header>
      <p className="muted">{CONTEXT_LAYER_HINT[layer]}</p>
      {items.length === 0 ? (
        <p className="conversation-context-empty" role="status">
          {note ?? "源文件未落盘，无法确认。"}
        </p>
      ) : (
        <ul>
          {items.map((item) => (
            <li key={`${item.layer}:${item.kind}:${item.id}`}>
              <span className="conversation-context-kind">{contextKindLabel(item.kind)}</span>
              <div>
                <strong>{item.label}</strong>
                {item.path ? <code>{item.path}</code> : null}
                {contextItemMetaText(item) ? (
                  <span className="muted">{contextItemMetaText(item)}</span>
                ) : null}
                {contextItemCharText(item) ? (
                  <span className="muted">{contextItemCharText(item)}</span>
                ) : null}
              </div>
            </li>
          ))}
        </ul>
      )}
      {items.length > 0 && note ? <p className="muted conversation-context-note">{note}</p> : null}
    </section>
  );
}
