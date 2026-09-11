import { createRef } from "react";
import { act } from "@testing-library/react";
import type { Mock } from "vitest";
import type {
  ConversationDetailDto,
  ConversationEvent,
  ConversationEventPage,
  ConversationSessionRow,
} from "../types";
import type { ConversationTimelineFollow } from "./useConversationTimelineFollow";

export type ConversationInvokeCall = {
  cmd: string;
  args: Record<string, unknown> | undefined;
};

export type Deferred<T> = {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (reason?: unknown) => void;
};

export function createDeferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

export function bindInvokeQueue(
  invokeMock: Mock<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>,
) {
  const queues = new Map<string, Array<Deferred<unknown>>>();
  const calls: ConversationInvokeCall[] = [];

  invokeMock.mockImplementation(async (cmd, args) => {
    calls.push({ cmd, args });
    const next = queues.get(cmd)?.shift();
    if (!next) {
      throw new Error(`unexpected invoke: ${cmd}`);
    }
    return next.promise;
  });

  return {
    calls,
    enqueue<T>(cmd: string): Deferred<T> {
      const item = createDeferred<T>();
      const queue = queues.get(cmd) ?? [];
      queue.push(item as Deferred<unknown>);
      queues.set(cmd, queue);
      return item;
    },
    reset() {
      queues.clear();
      calls.length = 0;
      invokeMock.mockClear();
    },
  };
}

export async function resolveAct<T>(pending: Deferred<T>, value: T): Promise<void> {
  await act(async () => {
    pending.resolve(value);
    await pending.promise;
  });
}

export async function rejectAct<T>(pending: Deferred<T>, reason: unknown): Promise<void> {
  await act(async () => {
    pending.reject(reason);
    await pending.promise.catch(() => undefined);
  });
}

export function conversationSession(
  sessionId: string,
  overrides: Partial<ConversationSessionRow> = {},
): ConversationSessionRow {
  return {
    source: "codex",
    session_id: sessionId,
    title: sessionId,
    project: "/workspace/project",
    model: "gpt-test",
    started_at: "2026-08-21T00:00:00Z",
    ended_at: "2026-08-21T00:01:00Z",
    source_file: `${sessionId}.jsonl`,
    source_files: [`${sessionId}.jsonl`],
    capabilities: ["messages", "events", "usage"],
    support_status: "experimental",
    file_available: true,
    total_tokens: 0,
    cost: null,
    unpriced: false,
    ...overrides,
  };
}

export function conversationDetail(
  session: ConversationSessionRow,
  overrides: Partial<ConversationDetailDto> = {},
): ConversationDetailDto {
  return {
    revision: "r1",
    session,
    event_count: 4,
    usage_record_count: 0,
    agent_relations: { capability_status: "unavailable", parent: null, children: [] },
    ...overrides,
  };
}

export function conversationEvent(sequence: number): ConversationEvent {
  return {
    event_id: `e${sequence}`,
    sequence,
    source_file: "conv.jsonl",
    source_sequence: sequence,
    kind: "message",
    occurred_at: "2026-08-21T00:00:00Z",
    actor: "user",
    name: null,
    text: `msg ${sequence}`,
    details: null,
    attachments: [],
    capability_status: "complete",
    content_status: "complete",
  };
}

export function conversationEventPage(
  sequences: number[],
  neighbors: { before?: boolean; after?: boolean } = {},
): ConversationEventPage {
  return {
    events: sequences.map((sequence) => conversationEvent(sequence)),
    has_more_before: neighbors.before ?? false,
    has_more_after: neighbors.after ?? false,
  };
}

export function stubTimelineFollow(
  overrides: Partial<ConversationTimelineFollow> = {},
): ConversationTimelineFollow {
  return {
    timelineRef: createRef<HTMLDivElement>(),
    timelineApiRef: createRef(),
    atTop: true,
    atBottom: true,
    unseenCount: 0,
    captureTimelineAnchor: () => undefined,
    handleWindowChange: () => undefined,
    handleTimelineScroll: () => undefined,
    jumpTimeline: () => Promise.resolve(),
    rememberEventsScroll: () => undefined,
    pinTimelineLayout: () => undefined,
    applyFollowedReplace: () => undefined,
    pinToLatest: () => undefined,
    prepareOpen: () => undefined,
    prepareEnterChild: () => undefined,
    prepareBack: () => undefined,
    prepareClose: () => undefined,
    cancelJumps: () => undefined,
    parentScrollTop: () => 0,
    ...overrides,
  };
}
