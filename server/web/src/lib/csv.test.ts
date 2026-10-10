import { describe, expect, it } from "vitest";
import { toCsv } from "./csv";

const body = (rows: Parameters<typeof toCsv>[0]) => toCsv(rows).replace("\uFEFF", "");

describe("toCsv", () => {
  it("starts with a BOM so Excel reads Chinese as UTF-8", () => {
    expect(toCsv([["名称"]]).startsWith("\uFEFF")).toBe(true);
  });

  it("joins cells with commas and rows with CRLF", () => {
    expect(
      body([
        ["a", "b"],
        ["c", 2],
      ]),
    ).toBe("a,b\r\nc,2\r\n");
  });

  it("quotes cells with commas, quotes or line breaks, doubling inner quotes", () => {
    expect(body([["a,b", 'say "hi"', "x\ny"]])).toBe('"a,b","say ""hi""","x\ny"\r\n');
  });

  it("leaves null, undefined and non-finite numbers empty", () => {
    expect(body([[null, undefined, NaN, Infinity, 0]])).toBe(",,,,0\r\n");
  });

  it("neutralises spreadsheet formulas in text but keeps negative numbers", () => {
    expect(body([["=1+1", "+cmd", "-x", "@sum", "\tx"]])).toBe("'=1+1,'+cmd,'-x,'@sum,'\tx\r\n");
    expect(body([[-5]])).toBe("-5\r\n");
  });
});
