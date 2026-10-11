import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { ToolChips, parseToolTrace } from "./ToolChips";

describe("parseToolTrace", () => {
  it("reads stored chips and ignores junk", () => {
    expect(parseToolTrace('[{"tool":"read_file","args":{},"summary":"Read a.md","ok":true}]')).toEqual([{ tool: "read_file", summary: "Read a.md", ok: true }]);
    expect(parseToolTrace(null)).toEqual([]);
    expect(parseToolTrace("not json")).toEqual([]);
    expect(parseToolTrace('{"a":1}')).toEqual([]);
    expect(parseToolTrace('[1,{"nope":true}]')).toEqual([]);
  });
});

describe("ToolChips", () => {
  it("shows summaries, running state, and failures", () => {
    render(
      <ToolChips
        chips={[
          { tool: "read_file", summary: "Read notes.md", ok: true },
          { tool: "list_dir", summary: null, ok: null },
          { tool: "read_file", summary: "No access to /secret", ok: false },
        ]}
      />,
    );
    expect(screen.getByText("Read notes.md")).toBeTruthy();
    expect(screen.getByText("list dir")).toBeTruthy();
    expect(screen.getByText("No access to /secret").closest("li")?.className).toContain("text-danger");
  });
});
