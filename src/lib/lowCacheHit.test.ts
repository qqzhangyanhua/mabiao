import { describe, expect, it } from "vitest";
import { CURSOR_ACCOUNT_SOURCE, lowCacheHitEmptyHint, lowCacheHitEmptyTitle } from "./lowCacheHit";

describe("lowCacheHit empty copy", () => {
  it("prompts to click a source row when nothing is selected", () => {
    expect(lowCacheHitEmptyTitle(null, false)).toBe("点上来源行，查看该来源命中率最低的会话");
    expect(lowCacheHitEmptyHint(null, false)).toContain("无法计算");
  });

  it("shows 无法计算 for sources without cache metrics", () => {
    expect(lowCacheHitEmptyTitle("codex", false)).toBe("无法计算");
    expect(lowCacheHitEmptyHint("codex", false)).toContain("没有缓存读或缓存写");
  });

  it("does not send Cursor account usage into conversation records", () => {
    expect(lowCacheHitEmptyTitle(CURSOR_ACCOUNT_SOURCE, false)).toBe("无法计算");
    expect(lowCacheHitEmptyHint(CURSOR_ACCOUNT_SOURCE, false)).toContain("不是本机会话");
  });
});
