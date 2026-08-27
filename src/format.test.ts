import { describe, expect, it } from "vitest";
import { desktopSummary } from "./format";

describe("desktopSummary", () => {
  it("uses singular and plural labels", () => {
    expect(desktopSummary(1, 1)).toBe("1 display · 1 icon");
    expect(desktopSummary(2, 31)).toBe("2 displays · 31 icons");
  });
});
