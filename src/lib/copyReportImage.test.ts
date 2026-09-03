import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.fn<(...args: unknown[]) => Promise<unknown>>();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

const { copyReportImage } = await import("./copyReportImage");

describe("copyReportImage", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("strips the data URL prefix before invoking copy_image_to_clipboard", async () => {
    invokeMock.mockResolvedValueOnce(undefined);
    await copyReportImage("data:image/png;base64,AAAA");
    expect(invokeMock).toHaveBeenCalledWith("copy_image_to_clipboard", { base64: "AAAA" });
  });

  it("rejects a payload that is not a data URL", async () => {
    await expect(copyReportImage("AAAA")).rejects.toThrow("截图结果不是 PNG data URL");
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("rejects an empty PNG payload", async () => {
    await expect(copyReportImage("data:image/png;base64,")).rejects.toThrow("截图结果为空");
    expect(invokeMock).not.toHaveBeenCalled();
  });
});
