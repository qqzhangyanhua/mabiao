import { describe, expect, it } from "vitest";
import {
  BLANK_CUSTOM_QUOTA_DRAFT,
  fetchInputsOf,
  submittedSecret,
  type CustomQuotaDraft,
} from "./customQuotaDraft";

function draft(patch: Partial<CustomQuotaDraft> = {}): CustomQuotaDraft {
  return {
    ...BLANK_CUSTOM_QUOTA_DRAFT,
    name: "公司的中转",
    baseUrl: "https://relay.example.com",
    secret: "sk-relay-123456",
    ...patch,
  };
}

describe("fetchInputsOf", () => {
  it("改名不作废已经测出来的结果", () => {
    // 名称是纯展示标签，不参与取数。为它清掉「读到 $19」只会让用户以为哪里出错了。
    expect(fetchInputsOf(draft({ name: "老板的中转" }))).toBe(fetchInputsOf(draft()));
    expect(fetchInputsOf(draft({ id: "custom:a3f9c1" }))).toBe(fetchInputsOf(draft()));
  });

  it("地址 / 类型 / 密钥一改就作废", () => {
    // 这三样任何一样变了，上一次读到的额度都是对着另一份配置打出来的。
    for (const patch of [
      { baseUrl: "https://new.example.com" },
      { preset: "deepseek" as const },
      { secret: "sk-rotated-999999" },
    ]) {
      expect(fetchInputsOf(draft(patch))).not.toBe(fetchInputsOf(draft()));
    }
  });

  it("首尾空格不算改动：后端本来就会剃掉", () => {
    // 地址末尾多敲一个空格、或密钥粘贴时带上换行，请求的仍是同一个地址、同一把钥匙。
    expect(fetchInputsOf(draft({ baseUrl: "  https://relay.example.com  " }))).toBe(
      fetchInputsOf(draft()),
    );
    expect(fetchInputsOf(draft({ secret: "  sk-relay-123456  " }))).toBe(fetchInputsOf(draft()));
    // 空串与只有空格都是「沿用已存的那把」，本就同义。
    expect(fetchInputsOf(draft({ secret: "   " }))).toBe(fetchInputsOf(draft({ secret: "" })));
  });

  it("带 /v1 的写法与根地址仍算两份输入", () => {
    // 归一化只在后端一份。前端不认识「这两个其实是同一个地址」，也不该假装认识——
    // 多测一次是小事，前端偷偷归一化才是这个功能最怕的那种漂移。
    expect(fetchInputsOf(draft({ baseUrl: "https://relay.example.com/v1" }))).not.toBe(
      fetchInputsOf(draft()),
    );
  });
});

describe("submittedSecret", () => {
  it("留空表示沿用已存的那把，不是把密钥改成空", () => {
    // 界面上只有掩码，编辑名称或地址时留空才是常态。
    expect(submittedSecret("")).toBeNull();
    expect(submittedSecret("   ")).toBeNull();
  });

  it("填了就原样交出去", () => {
    expect(submittedSecret("sk-relay-123456")).toBe("sk-relay-123456");
    // 首尾空格交给后端剃：这里改写会让「我明明粘对了」变成一个查不出的问题。
    expect(submittedSecret("  sk-relay-123456  ")).toBe("  sk-relay-123456  ");
  });
});
