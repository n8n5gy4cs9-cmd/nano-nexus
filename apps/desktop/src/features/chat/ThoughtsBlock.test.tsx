import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test } from "vitest";

import { ThoughtsBlock } from "./ThoughtsBlock";

test("collapsed by default and expands on click", () => {
  render(<ThoughtsBlock text="step one" streaming={false} />);
  expect(screen.queryByText("step one")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: /Thoughts/ }));
  expect(screen.getByText("step one")).toBeTruthy();
});

test("shows a live label while streaming", () => {
  render(<ThoughtsBlock text="" streaming startedAt={Date.now() - 3000} />);
  expect(screen.getByText("Thinking…")).toBeTruthy();
});
