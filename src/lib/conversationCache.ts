/**
 * 会话详情的内存缓存策略。
 *
 * 详情 DTO 现在只有元数据，但导航栈和当前展开的子会话仍应留着，其余按最近使用淘汰，
 * 避免在子代理之间反复下钻时把每一层的关系图都攒在内存里。
 */

export const CONVERSATION_DETAIL_CACHE_LIMIT = 8;

export type ConversationCacheChild = { relationship_id: string; key: string | null };

export function conversationKey(session: { source: string; session_id: string }): string {
  return `${session.source}\u{1f}${session.session_id}`;
}

/** 导航栈上的会话，以及从它们出发、经由已展开关系可达的子会话——这些正在渲染，不能淘汰。 */
export function pinnedConversationKeys({
  rootKeys,
  expandedRelationshipIds,
  childrenOf,
}: {
  rootKeys: readonly string[];
  expandedRelationshipIds: readonly string[];
  childrenOf: (key: string) => readonly ConversationCacheChild[];
}): string[] {
  const expanded = new Set(expandedRelationshipIds);
  const pinned = new Set<string>();
  const queue = [...rootKeys];
  while (queue.length > 0) {
    const key = queue.shift();
    if (key === undefined || pinned.has(key)) {
      continue;
    }
    pinned.add(key);
    for (const child of childrenOf(key)) {
      if (child.key !== null && expanded.has(child.relationship_id)) {
        queue.push(child.key);
      }
    }
  }
  return [...pinned];
}

/** 最近使用的排在最后。 */
export function touchConversationOrder(order: readonly string[], key: string): string[] {
  return [...order.filter((entry) => entry !== key), key];
}

export function pruneConversationDetails<T>({
  details,
  order,
  pinned,
  limit = CONVERSATION_DETAIL_CACHE_LIMIT,
}: {
  details: Readonly<Record<string, T>>;
  order: readonly string[];
  pinned: readonly string[];
  limit?: number;
}): { details: Record<string, T>; order: string[] } {
  const present = (key: string) => Object.hasOwn(details, key);
  const keep = new Set(pinned.filter(present));
  const recent = order.filter(present);
  const budget = Math.max(0, limit - keep.size);

  let extra = 0;
  for (let index = recent.length - 1; index >= 0 && extra < budget; index -= 1) {
    const key = recent[index];
    if (keep.has(key)) {
      continue;
    }
    keep.add(key);
    extra += 1;
  }

  const nextDetails: Record<string, T> = {};
  for (const [key, value] of Object.entries(details)) {
    if (keep.has(key)) {
      nextDetails[key] = value;
    }
  }
  return { details: nextDetails, order: recent.filter((key) => keep.has(key)) };
}
