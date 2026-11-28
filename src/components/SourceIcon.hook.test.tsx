import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { SourceIcon } from "./SourceIcon";

describe("SourceIcon", () => {
  it("renders generated vector marks with custom and neutral fallback tones", () => {
    const { container } = render(
      <>
        <SourceIcon source="claude" />
        <SourceIcon source="codebuddy_ide" />
        <SourceIcon source="unknown-source" />
      </>,
    );
    const icons = container.querySelectorAll(".source-ico");
    expect(icons).toHaveLength(3);
    expect(icons[0]?.className).toContain("tone-claude");
    expect(icons[0]?.querySelectorAll("svg path").length).toBe(1);

    expect(icons[1]?.className).toContain("tone-other");
    expect(icons[1]?.querySelectorAll("svg path").length).toBe(2);

    expect(icons[2]?.className).toContain("tone-other");
    expect(icons[2]?.querySelectorAll("svg path").length).toBe(3);
  });

  it("renders generated data-URI image marks with expected plate styling", () => {
    const { container } = render(
      <>
        <SourceIcon source="alma" />
        <SourceIcon source="agy" />
        <SourceIcon source="workbuddy" />
        <SourceIcon source="zcode" />
      </>,
    );
    const icons = container.querySelectorAll(".source-ico");
    expect(icons).toHaveLength(4);

    expect(icons[0]?.className).toContain("plate");
    expect(icons[0]?.querySelector("img")?.getAttribute("src")).toMatch(/^data:image\/png;base64,/);

    expect(icons[1]?.className).toContain("tone-gemini");
    expect(icons[1]?.className).not.toContain("plate");
    expect(icons[1]?.querySelector("img")?.getAttribute("src")).toMatch(
      /^data:image\/svg\+xml;base64,/,
    );

    expect(icons[2]?.className).toContain("plate");
    expect(icons[2]?.querySelector("img")?.getAttribute("src")).toMatch(
      /^data:image\/svg\+xml;base64,/,
    );

    expect(icons[3]?.className).toContain("plate");
    expect(icons[3]?.querySelector("img")?.getAttribute("src")).toMatch(/^data:image\/png;base64,/);
  });
});
