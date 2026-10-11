import { expect, test } from "vitest";

import { formatBytes, formatDuration, relativeTime } from "./format";

test("bytes", () => {
  expect(formatBytes(0)).toBe("0 B");
  expect(formatBytes(1536)).toBe("1.5 KB");
  expect(formatBytes(1_181_116_006)).toBe("1.1 GB");
  expect(formatBytes(-1)).toBe("—");
});

test("durations", () => {
  expect(formatDuration(1900)).toBe("1.9s");
  expect(formatDuration(12_000)).toBe("12s");
  expect(formatDuration(125_000)).toBe("2m 05s");
  expect(formatDuration(3_780_000)).toBe("1h 03m");
});

test("relative time", () => {
  const now = 1_700_000_000_000;
  expect(relativeTime(now - 10_000, now)).toBe("just now");
  expect(relativeTime(now - 5 * 60_000, now)).toBe("5 min ago");
  expect(relativeTime(now - 3 * 3_600_000, now)).toBe("3 h ago");
  expect(relativeTime(now - 30 * 3_600_000, now)).toBe("yesterday");
  expect(relativeTime(now - 3 * 86_400_000, now)).toBe("3 d ago");
});
