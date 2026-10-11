import { describe, expect, it } from "vitest";

import { languageFor, languageIds } from "./languages";
import { contextAround, extractFence, isSuspicious, matchTrailingNewline } from "./rewrite";

describe("extractFence", () => {
  it("takes the first block and ignores the bullets after it", () => {
    const reply = "```python\ndef f(x):\n    return x\n```\n- renamed a";
    expect(extractFence(reply)).toBe("def f(x):\n    return x");
  });
  it("accepts a fence that was cut off", () => {
    expect(extractFence("```js\nlet a = 1;")).toBe("let a = 1;");
  });
  it("returns null without a fence", () => {
    expect(extractFence("no code here")).toBeNull();
  });
});

describe("isSuspicious", () => {
  it("flags big growth and big shrinkage only", () => {
    const o = "x".repeat(100);
    expect(isSuspicious(o, "x".repeat(250))).toBe(true);
    expect(isSuspicious(o, "x".repeat(20))).toBe(true);
    expect(isSuspicious(o, "x".repeat(120))).toBe(false);
    expect(isSuspicious("", "anything")).toBe(false);
  });
});

describe("matchTrailingNewline", () => {
  it("copies the original's ending", () => {
    expect(matchTrailingNewline("a\n", "b")).toBe("b\n");
    expect(matchTrailingNewline("a", "b\n")).toBe("b");
  });
});

describe("contextAround", () => {
  it("returns the neighbouring lines", () => {
    const doc = "l1\nl2\nl3\nl4\nl5";
    const from = doc.indexOf("l3");
    const c = contextAround(doc, from, from + 2, 1, 1);
    expect(c).toEqual({ before: "l2", after: "l4" });
  });
});

describe("languages", () => {
  it("maps extensions and falls back to plain text", () => {
    expect(languageFor("player.py").id).toBe("python");
    expect(languageFor("App.TSX").id).toBe("typescript");
    expect(languageFor("main.go").id).toBe("go");
    expect(languageFor("Makefile").id).toBe("text");
    expect(languageFor("notes.unknown").id).toBe("text");
  });
  it("covers the spec's language list", () => {
    for (const id of ["javascript", "rust", "python", "json", "markdown", "html", "css", "cpp", "java", "sql", "xml", "yaml", "php", "shell", "lua", "toml", "csharp", "swift", "go", "ruby", "ini"]) {
      expect(languageIds()).toContain(id);
    }
  });
});
