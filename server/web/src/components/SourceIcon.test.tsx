import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { SourceIcon } from "./SourceIcon";

describe("SourceIcon", () => {
  it("renders svg marks and image plates without leaving this package", () => {
    const { container } = render(
      <>
        <SourceIcon source="claude" />
        <SourceIcon source="alma" />
        <SourceIcon source="unknown-source" />
      </>,
    );
    expect(container.querySelectorAll("svg").length).toBeGreaterThan(0);
    expect(container.querySelectorAll("img").length).toBe(1);
  });
});
