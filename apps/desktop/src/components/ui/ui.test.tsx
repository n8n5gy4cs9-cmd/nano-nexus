import { render, screen } from "@testing-library/react";
import { expect, test } from "vitest";

import { Badge, Button, EmptyState, IconButton, Progress, StatCard } from "./index";

test("button defaults to type=button and supports variants", () => {
  render(<Button variant="primary">Go</Button>);
  const b = screen.getByRole("button", { name: "Go" });
  expect(b.getAttribute("type")).toBe("button");
  expect(b.className).toContain("nexus-gradient");
});

test("icon buttons keep their aria-label", () => {
  render(
    <IconButton aria-label="Close">
      <span>x</span>
    </IconButton>,
  );
  expect(screen.getByLabelText("Close")).toBeTruthy();
});

test("progress clamps and reports", () => {
  render(<Progress value={1.7} label="Download" />);
  expect(screen.getByRole("progressbar").getAttribute("aria-valuenow")).toBe("100");
});

test("empty state and stat card render their text", () => {
  render(
    <>
      <EmptyState title="Nothing here" description="Add something" />
      <StatCard label="Files" value="12" />
      <Badge tone="success">ok</Badge>
    </>,
  );
  expect(screen.getByText("Nothing here")).toBeTruthy();
  expect(screen.getByText("12")).toBeTruthy();
});
