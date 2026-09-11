import { act, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { useConversationTimelineFollow } from "./useConversationTimelineFollow";

describe("useConversationTimelineFollow", () => {
  it("accumulates unseen events after the reader leaves the bottom", () => {
    const { result } = renderHook(() => useConversationTimelineFollow());

    act(() => {
      result.current.captureTimelineAnchor();
      result.current.applyFollowedReplace(4, 7);
    });

    expect(result.current.unseenCount).toBe(3);
  });

  it("does not accumulate unseen events while still following the bottom", () => {
    const { result } = renderHook(() => useConversationTimelineFollow());

    act(() => {
      result.current.applyFollowedReplace(4, 7);
    });

    expect(result.current.unseenCount).toBe(0);
  });

  it("clears unseen events when pinning to latest", () => {
    const { result } = renderHook(() => useConversationTimelineFollow());

    act(() => {
      result.current.captureTimelineAnchor();
      result.current.applyFollowedReplace(4, 7);
      result.current.pinToLatest();
    });

    expect(result.current.unseenCount).toBe(0);
  });

  it("resets follow state when opening another session so the previous unseen count is not kept", () => {
    const { result } = renderHook(() => useConversationTimelineFollow());

    act(() => {
      result.current.captureTimelineAnchor();
      result.current.applyFollowedReplace(4, 7);
    });
    expect(result.current.unseenCount).toBe(3);

    act(() => {
      result.current.prepareOpen(false);
    });
    expect(result.current.unseenCount).toBe(0);

    act(() => {
      result.current.applyFollowedReplace(0, 2);
    });
    expect(result.current.unseenCount).toBe(0);
  });

  it("starts away from the bottom when opening a body search hit", () => {
    const { result } = renderHook(() => useConversationTimelineFollow());

    act(() => {
      result.current.prepareOpen(true);
      result.current.applyFollowedReplace(4, 6);
    });

    expect(result.current.unseenCount).toBe(2);
    expect(result.current.atBottom).toBe(false);
    expect(result.current.atTop).toBe(false);
  });

  it("clears unseen events when jumping to the bottom without a timeline node", async () => {
    const { result } = renderHook(() => useConversationTimelineFollow());

    act(() => {
      result.current.captureTimelineAnchor();
      result.current.applyFollowedReplace(4, 8);
    });
    expect(result.current.unseenCount).toBe(4);

    await act(async () => {
      await result.current.jumpTimeline("bottom");
    });

    expect(result.current.unseenCount).toBe(0);
    expect(result.current.atBottom).toBe(true);
    expect(result.current.atTop).toBe(false);
  });
});
