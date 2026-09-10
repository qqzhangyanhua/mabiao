import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import {
  contextCompletenessNote,
  contextInjectedDegraded,
  contextItemCharText,
  contextItemExpandable,
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
  isUnusedInstall,
  partitionContextItems,
} from "../lib/conversationContext";
import { useKeyedAsyncLoad } from "../lib/useKeyedAsyncLoad";
import type {
  ConversationContextItem,
  ConversationContextItemContentDto,
  ConversationContextLayer,
  ConversationContextManifest,
} from "../types";
import { CollapsibleSection } from "./CollapsibleSection";
import { Spinner } from "./Spinner";

const LAYERS: ConversationContextLayer[] = ["injected", "observed", "on_disk_possible"];

export function ConversationContextManifestPanel({
  manifest,
  source,
  sessionId,
  highlightItemKey = null,
}: {
  manifest: ConversationContextManifest;
  source: string;
  sessionId: string;
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
        复盘噪音用。已注入来自首轮快照，可展开查看正文；已观测来自本会话事件；磁盘项只表示可能生效。
      </p>
      {mcpSummary ? <p className="conversation-context-mcp-summary">{mcpSummary}</p> : null}
      {completeness ? (
        <p className="conversation-context-completeness">{completeness}</p>
      ) : null}
      <div className="conversation-context-layers">
        {LAYERS.map((layer) => {
          const items = contextLayerItems(manifest, layer).filter(
            (item) => !isUnusedInstall(item),
          );
          if (layer === "on_disk_possible" && items.length === 0) {
            return null;
          }
          return (
            <ContextLayer
              key={layer}
              layer={layer}
              items={items}
              note={contextLayerNote(manifest, layer)}
              manifest={manifest}
              source={source}
              sessionId={sessionId}
              volumeIsEstimate={volumeIsEstimate}
              highlightItemKey={highlightItemKey}
            />
          );
        })}
      </div>
    </CollapsibleSection>
  );
}

function ContextLayer({
  layer,
  items,
  note,
  manifest,
  source,
  sessionId,
  volumeIsEstimate,
  highlightItemKey,
}: {
  layer: ConversationContextLayer;
  items: ConversationContextItem[];
  note?: string | null;
  manifest: ConversationContextManifest;
  source: string;
  sessionId: string;
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
          source={source}
          sessionId={sessionId}
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
  source,
  sessionId,
  volumeIsEstimate,
  highlightItemKey,
}: {
  items: ConversationContextItem[];
  source: string;
  sessionId: string;
  volumeIsEstimate: boolean;
  highlightItemKey: string | null;
}) {
  const { primary, editorBuiltin, disconnectedMcp } = partitionContextItems(items);
  const builtinSummary = contextItemsTokenSummary(editorBuiltin, volumeIsEstimate);
  return (
    <>
      {primary.length > 0 ? (
        <ul>
          {primary.map((item) => (
            <ContextItemRow
              key={`${item.layer}:${item.kind}:${item.id}`}
              item={item}
              source={source}
              sessionId={sessionId}
              volumeIsEstimate={volumeIsEstimate}
              highlightItemKey={highlightItemKey}
            />
          ))}
        </ul>
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
                source={source}
                sessionId={sessionId}
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
                source={source}
                sessionId={sessionId}
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
  source,
  sessionId,
  volumeIsEstimate,
  highlightItemKey,
}: {
  item: ConversationContextItem;
  source: string;
  sessionId: string;
  volumeIsEstimate: boolean;
  highlightItemKey: string | null;
}) {
  const meta = contextItemMetaText(item, { volumeIsEstimate });
  const itemKey = contextItemKey(item.layer, item.kind, item.id);
  const focused = itemKey === highlightItemKey;
  const expandable = contextItemExpandable(item);
  const [content, setContent] = useState<string | null>(null);
  const loadedRef = useRef(false);
  const { states, errors, run } = useKeyedAsyncLoad<string>();
  const loadContent = useCallback(() => {
    if (!expandable || loadedRef.current) {
      return;
    }
    void run(
      itemKey,
      () =>
        invoke<ConversationContextItemContentDto>("get_conversation_context_item_content", {
          source,
          sessionId,
          itemId: item.id,
        }),
      (result) => {
        loadedRef.current = true;
        setContent(result.content);
      },
    );
  }, [expandable, item.id, itemKey, run, sessionId, source]);
  useEffect(() => {
    if (focused && expandable) {
      loadContent();
    }
  }, [expandable, focused, loadContent]);
  const heading = (
    <>
      <strong>{item.label}</strong>
      {item.path ? <code>{item.path}</code> : null}
      {meta ? <span className="muted">{meta}</span> : null}
      {contextItemCharText(item) ? (
        <span className="muted">{contextItemCharText(item)}</span>
      ) : null}
    </>
  );
  return (
    <li
      className={[item.is_noise ? "is-noise" : "", focused ? "is-focus" : ""]
        .filter(Boolean)
        .join(" ") || undefined}
      data-context-item-key={itemKey}
    >
      <span className="conversation-context-kind">{contextKindLabel(item.kind)}</span>
      <div>
        {expandable ? (
          <details
            className="conversation-context-item-details"
            {...(focused ? { open: true } : {})}
            onToggle={(event) => {
              if (event.currentTarget.open) {
                loadContent();
              }
            }}
          >
            <summary>{heading}</summary>
            {states[itemKey] === "loading" ? <Spinner size={14} /> : null}
            {errors[itemKey] ? (
              <p className="muted conversation-context-item-error">{errors[itemKey]}</p>
            ) : null}
            {content !== null ? (
              <pre className="conversation-context-item-body">{content}</pre>
            ) : null}
          </details>
        ) : (
          heading
        )}
      </div>
    </li>
  );
}
