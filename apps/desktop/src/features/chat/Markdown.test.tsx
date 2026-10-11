import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";

import { Markdown, isSafeLink } from "./Markdown";

vi.mock("@/lib/highlight", () => ({ highlight: () => Promise.resolve(null) }));

test("renders headings, lists, tables and inline code", () => {
  render(<Markdown source={"## Findings\n\n- one\n- two\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\nUse `foo()` here."} />);
  expect(screen.getByRole("heading", { name: "Findings" })).toBeTruthy();
  expect(screen.getAllByRole("listitem")).toHaveLength(2);
  expect(screen.getByRole("table")).toBeTruthy();
  expect(screen.getByText("foo()").tagName).toBe("CODE");
});

test("fenced code becomes a code block with a copy button", () => {
  render(<Markdown source={"```ts\nconst x: number = 1;\n```"} />);
  expect(screen.getByText("typescript")).toBeTruthy();
  expect(screen.getByLabelText("Copy code")).toBeTruthy();
  expect(screen.getByText("const x: number = 1;")).toBeTruthy();
});

test("raw HTML from the model is not rendered as markup", () => {
  const { container } = render(<Markdown source={'Hello <img src=x onerror="alert(1)"> <script>alert(2)</script> world'} />);
  expect(container.querySelector("img")).toBeNull();
  expect(container.querySelector("script")).toBeNull();
});

test("links open through the callback and unsafe schemes are plain text", () => {
  const open = vi.fn();
  render(<Markdown source={"[site](https://example.com) and [bad](javascript:alert(1))"} onOpenLink={open} />);
  fireEvent.click(screen.getByText("site"));
  expect(open).toHaveBeenCalledWith("https://example.com");
  expect(screen.getByText("bad").tagName).toBe("SPAN");
  expect(isSafeLink("file:///etc/passwd")).toBe(false);
  expect(isSafeLink("https://a.b")).toBe(true);
});
