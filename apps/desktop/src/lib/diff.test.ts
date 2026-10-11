import { describe, expect, it } from "vitest";

import { parseUnifiedDiff, toSplitRows } from "./diff";

const DIFF = ["--- a/x", "+++ b/x", "@@ -1,3 +1,4 @@", " keep", "-old", "+new", "+extra", " tail", ""].join("\n");

describe("parseUnifiedDiff", () => {
  it("numbers lines on both sides and skips headers", () => {
    const lines = parseUnifiedDiff(DIFF);
    expect(lines.map((l) => l.kind)).toEqual(["hunk", "context", "del", "add", "add", "context"]);
    expect(lines[2]).toMatchObject({ oldNo: 2, newNo: null, text: "old" });
    expect(lines[4]).toMatchObject({ oldNo: null, newNo: 3, text: "extra" });
    expect(lines[5]).toMatchObject({ oldNo: 3, newNo: 4 });
  });
});

describe("toSplitRows", () => {
  it("pairs a deletion with the first addition", () => {
    const rows = toSplitRows(parseUnifiedDiff(DIFF));
    expect(rows).toHaveLength(5);
    expect(rows[2]?.left?.text).toBe("old");
    expect(rows[2]?.right?.text).toBe("new");
    expect(rows[3]?.left).toBeNull();
    expect(rows[3]?.right?.text).toBe("extra");
  });
});
