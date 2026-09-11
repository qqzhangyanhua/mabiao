import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ConversationDetailDto, ConversationDetailStateDto } from "../types";
import { conversationKey } from "./conversationCache";
import {
  bindInvokeQueue,
  conversationDetail,
  conversationSession,
  rejectAct,
  resolveAct,
  stubTimelineFollow,
} from "./conversationHookTestSupport";
import { useConversationDetailLoader } from "./useConversationDetailLoader";

const invokeMock = vi.hoisted(() =>
  vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(),
);

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (cmd: string, args?: Record<string, unknown>) => invokeMock(cmd, args),
}));

const queue = bindInvokeQueue(invokeMock);

function renderLoader(
  session: ReturnType<typeof conversationSession> | null,
  follow = stubTimelineFollow(),
) {
  return renderHook(
    (props: {
      selected: ReturnType<typeof conversationSession> | null;
      selectedKey: string | null;
    }) =>
      useConversationDetailLoader({
        selected: props.selected,
        selectedKey: props.selectedKey,
        follow,
      }),
    {
      initialProps: {
        selected: session,
        selectedKey: session ? conversationKey(session) : null,
      },
    },
  );
}

describe("useConversationDetailLoader", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    queue.reset();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("discards an older same-session response after a newer fetch starts", async () => {
    const session = conversationSession("conv-a");
    const follow = stubTimelineFollow({
      pinToLatest: vi.fn(),
      applyFollowedReplace: vi.fn(),
    });
    const { result } = renderLoader(session, follow);
    const firstDetail = queue.enqueue<ConversationDetailDto>("get_conversation_detail");
    const secondDetail = queue.enqueue<ConversationDetailDto>("get_conversation_detail");

    act(() => {
      result.current.fetchDetail(session);
      result.current.fetchDetail(session);
    });

    await resolveAct(
      firstDetail,
      conversationDetail(session, { revision: "r-old", event_count: 4 }),
    );
    expect(result.current.detail).toBeNull();
    expect(follow.pinToLatest).not.toHaveBeenCalled();

    await resolveAct(
      secondDetail,
      conversationDetail(session, { revision: "r-new", event_count: 9 }),
    );
    expect(result.current.detail?.revision).toBe("r-new");
    expect(result.current.detail?.event_count).toBe(9);
    expect(result.current.detailLoading).toBe(false);
    expect(follow.pinToLatest).toHaveBeenCalledOnce();
  });

  it("keeps the newly selected session when a previous request resolves late", async () => {
    const sessionA = conversationSession("conv-a");
    const sessionB = conversationSession("conv-b");
    const applyFollowedReplace = vi.fn();
    const pinToLatest = vi.fn();
    const follow = stubTimelineFollow({ applyFollowedReplace, pinToLatest });
    const { result, rerender } = renderLoader(sessionA, follow);
    const detailA = queue.enqueue<ConversationDetailDto>("get_conversation_detail");
    const detailB = queue.enqueue<ConversationDetailDto>("get_conversation_detail");

    act(() => {
      result.current.fetchDetail(sessionA);
    });
    rerender({ selected: sessionB, selectedKey: conversationKey(sessionB) });
    act(() => {
      result.current.fetchDetail(sessionB);
    });

    await resolveAct(detailA, conversationDetail(sessionA, { revision: "r-a", event_count: 3 }));
    expect(result.current.detail).toBeNull();
    expect(pinToLatest).not.toHaveBeenCalled();
    expect(applyFollowedReplace).not.toHaveBeenCalled();

    await resolveAct(detailB, conversationDetail(sessionB, { revision: "r-b", event_count: 8 }));
    expect(result.current.detail?.session.session_id).toBe("conv-b");
    expect(result.current.detail?.revision).toBe("r-b");
    expect(pinToLatest).toHaveBeenCalledOnce();
    expect(applyFollowedReplace).not.toHaveBeenCalled();
  });

  it("ignores a response that arrives after unmount", async () => {
    const session = conversationSession("conv-a");
    const pinToLatest = vi.fn();
    const cancelJumps = vi.fn();
    const follow = stubTimelineFollow({ pinToLatest, cancelJumps });
    const { result, unmount } = renderLoader(session, follow);
    const pending = queue.enqueue<ConversationDetailDto>("get_conversation_detail");

    act(() => {
      result.current.fetchDetail(session);
    });
    unmount();
    expect(cancelJumps).toHaveBeenCalledOnce();

    await resolveAct(pending, conversationDetail(session, { revision: "late" }));
    expect(pinToLatest).not.toHaveBeenCalled();
  });

  it("queues a manual refresh until an in-flight unchanged poll releases the gate", async () => {
    const session = conversationSession("conv-a");
    const applyFollowedReplace = vi.fn();
    const pinToLatest = vi.fn();
    const follow = stubTimelineFollow({ applyFollowedReplace, pinToLatest });
    const { result } = renderLoader(session, follow);
    const initial = queue.enqueue<ConversationDetailDto>("get_conversation_detail");

    act(() => {
      result.current.fetchDetail(session);
    });
    await resolveAct(initial, conversationDetail(session, { revision: "r1", event_count: 4 }));
    expect(result.current.detail?.revision).toBe("r1");
    pinToLatest.mockClear();

    const pollState = queue.enqueue<ConversationDetailStateDto>("get_conversation_detail_state");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2_000);
    });

    const manual = queue.enqueue<ConversationDetailDto>("get_conversation_detail");
    act(() => {
      result.current.fetchDetail(session);
    });

    await resolveAct(pollState, { revision: "r1", changed: false, file_available: true });
    expect(result.current.detail?.revision).toBe("r1");
    expect(applyFollowedReplace).not.toHaveBeenCalled();

    await resolveAct(manual, conversationDetail(session, { revision: "r2", event_count: 6 }));
    expect(result.current.detail?.revision).toBe("r2");
    expect(result.current.pollError).toBeNull();
    expect(pinToLatest).toHaveBeenCalledOnce();
  });

  it("does not apply a poll reload that finishes after a newer manual generation", async () => {
    const session = conversationSession("conv-a");
    const applyFollowedReplace = vi.fn();
    const pinToLatest = vi.fn();
    const follow = stubTimelineFollow({ applyFollowedReplace, pinToLatest });
    const { result } = renderLoader(session, follow);
    const initial = queue.enqueue<ConversationDetailDto>("get_conversation_detail");

    act(() => {
      result.current.fetchDetail(session);
    });
    await resolveAct(initial, conversationDetail(session, { revision: "r1", event_count: 4 }));
    pinToLatest.mockClear();

    const pollState = queue.enqueue<ConversationDetailStateDto>("get_conversation_detail_state");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2_000);
    });

    const pollDetail = queue.enqueue<ConversationDetailDto>("get_conversation_detail");
    await resolveAct(pollState, { revision: "r-poll", changed: true, file_available: true });

    const manual = queue.enqueue<ConversationDetailDto>("get_conversation_detail");
    act(() => {
      result.current.fetchDetail(session);
    });

    await resolveAct(
      pollDetail,
      conversationDetail(session, { revision: "r-poll", event_count: 5 }),
    );
    expect(result.current.detail?.revision).toBe("r1");
    expect(applyFollowedReplace).not.toHaveBeenCalled();

    await resolveAct(manual, conversationDetail(session, { revision: "r-manual", event_count: 7 }));
    expect(result.current.detail?.revision).toBe("r-manual");
    expect(pinToLatest).toHaveBeenCalledOnce();
  });

  it("follows a poll reload when no newer fetch has started", async () => {
    const session = conversationSession("conv-a");
    const applyFollowedReplace = vi.fn();
    const follow = stubTimelineFollow({ applyFollowedReplace, pinToLatest: vi.fn() });
    const { result } = renderLoader(session, follow);
    const initial = queue.enqueue<ConversationDetailDto>("get_conversation_detail");

    act(() => {
      result.current.fetchDetail(session);
    });
    await resolveAct(initial, conversationDetail(session, { revision: "r1", event_count: 4 }));

    const pollState = queue.enqueue<ConversationDetailStateDto>("get_conversation_detail_state");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2_000);
    });
    const pollDetail = queue.enqueue<ConversationDetailDto>("get_conversation_detail");
    await resolveAct(pollState, { revision: "r2", changed: true, file_available: true });
    await resolveAct(pollDetail, conversationDetail(session, { revision: "r2", event_count: 6 }));

    expect(result.current.detail?.revision).toBe("r2");
    expect(applyFollowedReplace).toHaveBeenCalledWith(4, 6);
    expect(result.current.pollError).toBeNull();
  });

  it("does not keep an error from a request that finished after a later fetch started", async () => {
    const session = conversationSession("conv-a");
    const { result } = renderLoader(session);
    const first = queue.enqueue<ConversationDetailDto>("get_conversation_detail");
    const second = queue.enqueue<ConversationDetailDto>("get_conversation_detail");

    act(() => {
      result.current.fetchDetail(session);
      result.current.fetchDetail(session);
    });

    await rejectAct(first, new Error("stale failure"));
    expect(result.current.detailError).toBeNull();

    await resolveAct(second, conversationDetail(session, { revision: "r2" }));
    expect(result.current.detailError).toBeNull();
    expect(result.current.detail?.revision).toBe("r2");
  });
});
