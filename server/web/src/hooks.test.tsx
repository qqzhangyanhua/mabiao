import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useAsync } from "./hooks";

function Probe({ load }: { load: (() => Promise<string>) | null }) {
  const state = useAsync(load);
  return (
    <div>
      <span data-testid="loading">{String(state.loading)}</span>
      <span data-testid="data">{state.data ?? ""}</span>
      <span data-testid="error">{state.error?.message ?? ""}</span>
      <button type="button" onClick={state.reload}>
        reload
      </button>
    </div>
  );
}

afterEach(cleanup);

describe("useAsync", () => {
  it("stays idle without loading or error when load is null", () => {
    render(<Probe load={null} />);
    expect(screen.getByTestId("loading").textContent).toBe("false");
    expect(screen.getByTestId("data").textContent).toBe("");
    expect(screen.getByTestId("error").textContent).toBe("");
  });

  it("clears previous data and stops loading when transitioning from a loader to null", async () => {
    const loader = vi.fn(() => Promise.resolve("ready"));
    const { rerender } = render(<Probe load={loader} />);
    await waitFor(() => expect(screen.getByTestId("data").textContent).toBe("ready"));
    expect(screen.getByTestId("loading").textContent).toBe("false");

    rerender(<Probe load={null} />);
    expect(screen.getByTestId("loading").textContent).toBe("false");
    expect(screen.getByTestId("data").textContent).toBe("");
    expect(screen.getByTestId("error").textContent).toBe("");
    expect(loader).toHaveBeenCalledTimes(1);
  });

  it("reloading while idle is a safe no-op and a later loader runs normally", async () => {
    const { rerender } = render(<Probe load={null} />);
    act(() => {
      screen.getByRole("button", { name: "reload" }).click();
    });
    expect(screen.getByTestId("loading").textContent).toBe("false");

    const loader = vi.fn(() => Promise.resolve("loaded"));
    rerender(<Probe load={loader} />);
    await waitFor(() => expect(screen.getByTestId("data").textContent).toBe("loaded"));
    expect(loader).toHaveBeenCalledTimes(1);
  });
});
