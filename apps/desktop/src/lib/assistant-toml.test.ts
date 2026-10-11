import { describe, expect, it } from "vitest";

import { extractAssistantToml } from "./assistant-toml";

describe("extractAssistantToml", () => {
  it("finds the assistant block among other code", () => {
    const md = "Here you go:\n```python\nprint(1)\n```\n```toml\nschema_version = 1\nid = \"x-y\"\n[prompt]\nsystem = '''\nhi\n'''\n```\nDone.";
    expect(extractAssistantToml(md)).toContain('id = "x-y"');
  });
  it("ignores other toml and unfinished blocks", () => {
    expect(extractAssistantToml("```toml\nname = \"x\"\n```")).toBeNull();
    expect(extractAssistantToml("```toml\nschema_version = 1\n")).toBeNull();
    expect(extractAssistantToml("no code")).toBeNull();
  });
});
