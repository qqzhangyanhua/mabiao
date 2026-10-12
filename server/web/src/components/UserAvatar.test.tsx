import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { UserAvatar } from "./UserAvatar";

describe("UserAvatar", () => {
  it("keeps the same hue for the same session seed", () => {
    const first = render(<UserAvatar seed="session-a" />);
    const fill = first.container.querySelector("rect")?.getAttribute("fill");
    first.unmount();
    const second = render(<UserAvatar seed="session-a" />);
    expect(second.container.querySelector("rect")?.getAttribute("fill")).toBe(fill);
    expect(fill).toMatch(/^hsl\(\d+ 42% 46%\)$/);
  });
});
