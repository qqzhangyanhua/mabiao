import { describe, expect, it } from "vitest";
import { dataUrlToBlob } from "./objectUrl";

async function bytesOf(blob: Blob): Promise<number[]> {
  return [...new Uint8Array(await blob.arrayBuffer())];
}

describe("dataUrlToBlob", () => {
  it("解析 base64 图片并保留 media type", async () => {
    const blob = dataUrlToBlob("data:image/png;base64,AAECAw==");
    expect(blob).not.toBeNull();
    expect(blob?.type).toBe("image/png");
    expect(await bytesOf(blob!)).toEqual([0, 1, 2, 3]);
  });

  it("接受非 base64 的百分号编码正文", async () => {
    const blob = dataUrlToBlob("data:text/plain,hi%20there");
    expect(await blob!.text()).toBe("hi there");
  });

  it("缺少 media type 时退回二进制流", () => {
    expect(dataUrlToBlob("data:;base64,AAA=")?.type).toBe("application/octet-stream");
  });

  it("非 data URL 与损坏的 base64 都返回 null", () => {
    expect(dataUrlToBlob("https://example.com/a.png")).toBeNull();
    expect(dataUrlToBlob("data:image/png;base64,!!!not-base64!!!")).toBeNull();
  });
});
