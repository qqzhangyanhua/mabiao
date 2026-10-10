import { describe, expect, it } from "vitest";
import type { ContextItem, ContextManifest } from "../api/types";
import { groupByTier, tierOf } from "./context";

function item(layer: ContextItem["layer"], id: string, content?: string): ContextItem {
  return { layer, kind: "instruction", id, label: id, ...(content === undefined ? {} : { content }) };
}

function manifest(items: ContextItem[], extra: Partial<ContextManifest> = {}): ContextManifest {
  return { items, has_injected_snapshot: true, ...extra };
}

describe("tierOf", () => {
  it("only an injected item with original text is called injected", () => {
    const m = manifest([]);
    expect(tierOf(m, item("injected", "a", "原文"))).toBe("injected");
    expect(tierOf(m, item("injected", "a"))).toBe("injected_no_content");
  });

  it("never calls on-disk or observed items injected", () => {
    const m = manifest([]);
    expect(tierOf(m, item("on_disk_possible", "a"))).toBe("on_disk");
    expect(tierOf(m, item("observed", "a"))).toBe("observed");
  });

  it("a cached manifest has no original text even for injected-layer items", () => {
    const m = manifest([], { from_cache: true });
    expect(tierOf(m, item("injected", "a"))).toBe("cached");
    expect(tierOf(m, item("on_disk_possible", "b"))).toBe("on_disk");
  });

  it("a manifest without an injection snapshot is a reconstruction, not an injection", () => {
    const m = manifest([], { has_injected_snapshot: false });
    expect(tierOf(m, item("injected", "a"))).toBe("reconstructed");
  });
});

describe("groupByTier", () => {
  it("groups in a fixed order and drops empty groups", () => {
    const m = manifest([
      item("on_disk_possible", "d1"),
      item("injected", "i1", "x"),
      item("on_disk_possible", "d2"),
    ]);
    const groups = groupByTier(m);
    expect(groups.map((g) => g.tier)).toEqual(["injected", "on_disk"]);
    expect(groups[1]?.items.map((i) => i.id)).toEqual(["d1", "d2"]);
  });
});
