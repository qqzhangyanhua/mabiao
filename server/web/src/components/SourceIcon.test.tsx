import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { SourceIcon } from "./SourceIcon";

describe("SourceIcon", () => {
  it("renders generated svg marks and data-URI image marks without leaving this package", () => {
    const { container } = render(
      <>
        <SourceIcon source="claude" />
        <SourceIcon source="codebuddy_ide" />
        <SourceIcon source="alma" />
        <SourceIcon source="agy" />
        <SourceIcon source="workbuddy" />
        <SourceIcon source="zcode" />
        <SourceIcon source="windsurf" />
        <SourceIcon source="unknown-source" />
      </>,
    );
    const svgs = container.querySelectorAll("svg");
    const imgs = container.querySelectorAll("img");
    expect(svgs).toHaveLength(4);
    expect(imgs).toHaveLength(4);

    for (const img of imgs) {
      expect(img.getAttribute("src")).toMatch(/^data:image\/(png|svg\+xml);base64,/);
    }

    const wrappers = container.querySelectorAll("span");
    expect(wrappers[2]?.style.background).toBe("transparent");
    expect(wrappers[3]?.style.background).not.toBe("transparent");
  });
});
