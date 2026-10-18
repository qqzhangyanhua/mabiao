import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.fn<(...args: unknown[]) => Promise<unknown>>();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

const { buildCsv, exportCsv, exportCcusageDailyJson, exportImage, exportJson } = await import("./exportFile");

beforeEach(() => {
  invokeMock.mockReset();
});

describe("buildCsv", () => {
  it("joins headers and rows with CRLF, comma-separated", () => {
    expect(buildCsv(["a", "b"], [["1", "2"]])).toBe("a,b\r\n1,2");
  });

  it("quotes cells containing commas, quotes, or newlines", () => {
    expect(buildCsv(["name"], [['a,b"c\nd']])).toBe('name\r\n"a,b""c\nd"');
  });

  it("stringifies numeric cells without quoting", () => {
    expect(buildCsv(["count"], [[42]])).toBe("count\r\n42");
  });
});

describe("exportCsv", () => {
  it("invokes the export_csv command with built CSV content", async () => {
    invokeMock.mockResolvedValueOnce(true);
    const result = await exportCsv("out.csv", ["a"], [["1"]]);
    expect(result).toBe(true);
    expect(invokeMock).toHaveBeenCalledWith("export_csv", {
      defaultName: "out.csv",
      content: "a\r\n1",
    });
  });
});

describe("exportJson", () => {
  it("invokes the export_json command with an array of row objects", async () => {
    invokeMock.mockResolvedValueOnce(true);
    const result = await exportJson("out.json", ["a", "b"], [[1, "x"]]);
    expect(result).toBe(true);
    expect(invokeMock).toHaveBeenCalledWith("export_json", {
      defaultName: "out.json",
      content: JSON.stringify([{ a: 1, b: "x" }], null, 2),
    });
  });
});

describe("exportCcusageDailyJson", () => {
  it("asks Rust for the daily JSON then saves it through export_json", async () => {
    invokeMock.mockResolvedValueOnce("{}\n").mockResolvedValueOnce(true);
    const filter = {
      from: null,
      to: null,
      sources: [],
      models: [],
      projects: [],
      providers: [],
    };
    const result = await exportCcusageDailyJson("ccusage-daily.json", filter);
    expect(result).toBe(true);
    expect(invokeMock).toHaveBeenNthCalledWith(1, "get_ccusage_daily_json", { filter });
    expect(invokeMock).toHaveBeenNthCalledWith(2, "export_json", {
      defaultName: "ccusage-daily.json",
      content: "{}\n",
    });
  });
});

describe("exportImage", () => {
  it("strips the data URL prefix before invoking export_image", async () => {
    invokeMock.mockResolvedValueOnce(true);
    const result = await exportImage("out.png", "data:image/png;base64,AAAA");
    expect(result).toBe(true);
    expect(invokeMock).toHaveBeenCalledWith("export_image", {
      defaultName: "out.png",
      base64: "AAAA",
    });
  });
});
