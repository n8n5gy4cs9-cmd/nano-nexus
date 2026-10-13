import { describe, expect, it } from "vitest";

import { parseDisabled, toggleTool } from "./ToolsSection";

describe("tool toggles", () => {
  it("turns a tool off and back on without leaving empty entries", () => {
    const off = toggleTool({}, "agent", "search_web", false);
    expect(off).toEqual({ agent: ["search_web"] });
    expect(toggleTool(off, "agent", "search_web", true)).toEqual({});
  });

  it("keeps other assistants and tools untouched", () => {
    const start = { agent: ["write_file"], nexus: ["now"] };
    expect(toggleTool(start, "agent", "trash_file", false)).toEqual({ agent: ["write_file", "trash_file"], nexus: ["now"] });
  });

  it("reads only well-formed settings values", () => {
    expect(parseDisabled(null)).toEqual({});
    expect(parseDisabled(["x"])).toEqual({});
    expect(parseDisabled({ agent: ["a", 3, "b"], bad: "x" })).toEqual({ agent: ["a", "b"] });
  });
});
