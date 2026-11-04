import { describe, expect, it } from "vitest";
import { mergeLines } from "./DocumentsPanel";

describe("editing keeps citations honest", () => {
  const old = [{ text: "Built Smartsheet dashboards", sources: ["E0.1"] }, { text: "Ran UAT", sources: ["E0.4"] }];
  it("keeps sources for unchanged and edited lines, marks new lines as the user's own", () => {
    const r = mergeLines(old, ["Built Smartsheet dashboards", "Ran user acceptance testing", "Mentored new coordinators", ""]);
    expect(r).toEqual([
      { text: "Built Smartsheet dashboards", sources: ["E0.1"] },
      { text: "Ran user acceptance testing", sources: ["E0.4"] },
      { text: "Mentored new coordinators", sources: ["USER"] },
    ]);
  });
  it("finds moved lines by text", () => {
    expect(mergeLines(old, ["Ran UAT", "Built Smartsheet dashboards"]).map((l) => l.sources[0])).toEqual(["E0.4", "E0.1"]);
  });
});
