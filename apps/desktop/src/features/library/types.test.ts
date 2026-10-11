import { describe, expect, it } from "vitest";

import { PASSAGE_URL_PREFIX, parsePassageUrl } from "./types";

describe("parsePassageUrl", () => {
  it("reads passage ids from citation links", () => {
    expect(parsePassageUrl(`${PASSAGE_URL_PREFIX}57`)).toBe(57);
  });
  it("ignores other links and bad ids", () => {
    expect(parsePassageUrl("https://example.com/passage/57")).toBeNull();
    expect(parsePassageUrl(`${PASSAGE_URL_PREFIX}abc`)).toBeNull();
    expect(parsePassageUrl(`${PASSAGE_URL_PREFIX}0`)).toBeNull();
    expect(parsePassageUrl(`${PASSAGE_URL_PREFIX}1.5`)).toBeNull();
  });
});
