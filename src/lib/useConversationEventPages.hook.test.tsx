import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ConversationEventPage } from "../types";
import {
  bindInvokeQueue,
  conversationEventPage,
  rejectAct,
  resolveAct,
} from "./conversationHookTestSupport";
import { useConversationEventPages } from "./useConversationEventPages";

const invokeMock = vi.hoisted(() =>
  vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(),
);

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (cmd: string, args?: Record<string, unknown>) => invokeMock(cmd, args),
}));

const queue = bindInvokeQueue(invokeMock);

function renderPages(props: {
  source?: string;
  sessionId: string;
  revision: string;
  followLatest?: boolean;
  initialSequence?: number | null;
}) {
  return renderHook(
    (next: {
      source: string;
      sessionId: string;
      revision: string;
      followLatest: boolean;
      initialSequence: number | null;
    }) => useConversationEventPages(next),
    {
      initialProps: {
        source: props.source ?? "codex",
        sessionId: props.sessionId,
        revision: props.revision,
        followLatest: props.followLatest ?? false,
        initialSequence: props.initialSequence ?? null,
      },
    },
  );
}

describe("useConversationEventPages", () => {
  beforeEach(() => {
    queue.reset();
  });

  it("drops the previous session page when a later session request is already in flight", async () => {
    const pageA = queue.enqueue<ConversationEventPage>("get_conversation_events");
    const { result, rerender } = renderPages({ sessionId: "conv-a", revision: "r1" });
    expect(result.current.loading).toBe(true);

    const pageB = queue.enqueue<ConversationEventPage>("get_conversation_events");
    rerender({
      source: "codex",
      sessionId: "conv-b",
      revision: "r1",
      followLatest: false,
      initialSequence: null,
    });

    await resolveAct(pageA, conversationEventPage([1, 2], { before: true }));
    expect(result.current.eventWindow.events).toEqual([]);
    expect(result.current.loading).toBe(true);

    await resolveAct(pageB, conversationEventPage([8, 9]));
    expect(result.current.eventWindow.events.map((event) => event.sequence)).toEqual([8, 9]);
    expect(result.current.loading).toBe(false);
    expect(result.current.error).toBeNull();
  });

  it("does not prepend a late earlier page after a follow-latest replace starts", async () => {
    const initial = queue.enqueue<ConversationEventPage>("get_conversation_events");
    const { result, rerender } = renderPages({
      sessionId: "conv-a",
      revision: "r1",
      followLatest: true,
    });
    await resolveAct(initial, conversationEventPage([4, 5, 6], { before: true }));
    expect(result.current.eventWindow.events.map((event) => event.sequence)).toEqual([4, 5, 6]);

    const earlier = queue.enqueue<ConversationEventPage>("get_conversation_events");
    let earlierDone: Promise<boolean> = Promise.resolve(false);
    act(() => {
      earlierDone = result.current.loadEarlier();
    });

    const latest = queue.enqueue<ConversationEventPage>("get_conversation_events");
    rerender({
      source: "codex",
      sessionId: "conv-a",
      revision: "r2",
      followLatest: true,
      initialSequence: null,
    });

    await resolveAct(earlier, conversationEventPage([1, 2, 3], { after: true }));
    expect(result.current.eventWindow.events.map((event) => event.sequence)).toEqual([4, 5, 6]);
    await expect(earlierDone).resolves.toBe(false);

    await resolveAct(latest, conversationEventPage([7, 8, 9]));
    expect(result.current.eventWindow.events.map((event) => event.sequence)).toEqual([7, 8, 9]);
    expect(result.current.eventWindow.hasMoreBefore).toBe(false);
  });

  it("rejects a second earlier-page request while the first is still loading", async () => {
    const initial = queue.enqueue<ConversationEventPage>("get_conversation_events");
    const { result } = renderPages({ sessionId: "conv-a", revision: "r1" });
    await resolveAct(initial, conversationEventPage([7, 8], { before: true }));

    const firstEarlier = queue.enqueue<ConversationEventPage>("get_conversation_events");
    act(() => {
      void result.current.loadEarlier();
    });
    expect(result.current.loadingEarlier).toBe(true);

    let secondEarlier = true;
    await act(async () => {
      secondEarlier = await result.current.loadEarlier();
    });
    expect(secondEarlier).toBe(false);

    await resolveAct(firstEarlier, conversationEventPage([4, 5, 6], { before: true, after: true }));
    expect(result.current.eventWindow.events.map((event) => event.sequence)).toEqual([
      4, 5, 6, 7, 8,
    ]);

    const nextEarlier = queue.enqueue<ConversationEventPage>("get_conversation_events");
    let nextDone: Promise<boolean> = Promise.resolve(false);
    act(() => {
      nextDone = result.current.loadEarlier();
    });
    await resolveAct(nextEarlier, conversationEventPage([1, 2, 3], { after: true }));
    await expect(nextDone).resolves.toBe(true);
    expect(result.current.eventWindow.events.map((event) => event.sequence)).toEqual([
      1, 2, 3, 4, 5, 6, 7, 8,
    ]);
  });

  it("rejects a later-page request while an earlier page is still loading", async () => {
    const initial = queue.enqueue<ConversationEventPage>("get_conversation_events");
    const { result } = renderPages({ sessionId: "conv-a", revision: "r1" });
    await resolveAct(initial, conversationEventPage([4, 5], { before: true, after: true }));

    const earlier = queue.enqueue<ConversationEventPage>("get_conversation_events");
    act(() => {
      void result.current.loadEarlier();
    });
    expect(result.current.loadingEarlier).toBe(true);

    let laterResult = true;
    await act(async () => {
      laterResult = await result.current.loadLater();
    });
    expect(laterResult).toBe(false);

    await resolveAct(earlier, conversationEventPage([1, 2, 3], { after: true }));
    expect(result.current.eventWindow.events.map((event) => event.sequence)).toEqual([
      1, 2, 3, 4, 5,
    ]);
    expect(queue.calls.filter((call) => call.cmd === "get_conversation_events")).toHaveLength(2);
  });

  it("keeps the current window when revision changes while not following latest", async () => {
    const initial = queue.enqueue<ConversationEventPage>("get_conversation_events");
    const { result, rerender } = renderPages({
      sessionId: "conv-a",
      revision: "r1",
      followLatest: false,
    });
    await resolveAct(initial, conversationEventPage([4, 5, 6], { before: true }));

    rerender({
      source: "codex",
      sessionId: "conv-a",
      revision: "r2",
      followLatest: false,
      initialSequence: null,
    });

    expect(result.current.eventWindow.events.map((event) => event.sequence)).toEqual([4, 5, 6]);
    expect(queue.calls).toHaveLength(1);

    const latest = queue.enqueue<ConversationEventPage>("get_conversation_events");
    rerender({
      source: "codex",
      sessionId: "conv-a",
      revision: "r2",
      followLatest: true,
      initialSequence: null,
    });
    await resolveAct(latest, conversationEventPage([7, 8]));
    expect(result.current.eventWindow.events.map((event) => event.sequence)).toEqual([7, 8]);
  });

  it("does not surface a stale page error after the session has already changed", async () => {
    const pageA = queue.enqueue<ConversationEventPage>("get_conversation_events");
    const { result, rerender } = renderPages({ sessionId: "conv-a", revision: "r1" });
    const pageB = queue.enqueue<ConversationEventPage>("get_conversation_events");
    rerender({
      source: "codex",
      sessionId: "conv-b",
      revision: "r1",
      followLatest: false,
      initialSequence: null,
    });

    await rejectAct(pageA, new Error("gone"));
    expect(result.current.error).toBeNull();
    expect(result.current.loading).toBe(true);

    await resolveAct(pageB, conversationEventPage([10]));
    expect(result.current.error).toBeNull();
    expect(result.current.eventWindow.events.map((event) => event.sequence)).toEqual([10]);
  });
});
