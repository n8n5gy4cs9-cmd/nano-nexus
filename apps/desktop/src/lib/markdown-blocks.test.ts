import { expect, test } from "vitest";

import { normalizeLanguage, splitMarkdownBlocks } from "./markdown-blocks";

test("splits on blank lines but never inside fences", () => {
  const src = "# Title\n\nPara one\nstill one\n\n```js\nconst a = 1;\n\nconst b = 2;\n```\n\n- item";
  expect(splitMarkdownBlocks(src)).toEqual(["# Title", "Para one\nstill one", "```js\nconst a = 1;\n\nconst b = 2;\n```", "- item"]);
});

test("an unfinished fence stays one block while streaming", () => {
  expect(splitMarkdownBlocks("Intro\n\n```py\nx = 1\n\ny = 2")).toEqual(["Intro", "```py\nx = 1\n\ny = 2"]);
});

test("completed blocks are stable as text grows", () => {
  const a = splitMarkdownBlocks("One\n\nTwo");
  const b = splitMarkdownBlocks("One\n\nTwo more words");
  expect(a[0]).toBe(b[0]);
});

test("language aliases", () => {
  expect(normalizeLanguage("ts")).toBe("typescript");
  expect(normalizeLanguage("language-js")).toBe("javascript");
  expect(normalizeLanguage(undefined)).toBe("text");
  expect(normalizeLanguage("Rust")).toBe("rust");
});
