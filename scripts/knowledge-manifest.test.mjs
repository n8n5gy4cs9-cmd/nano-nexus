import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { knowledgeSize, resolveEntries, slug, stageKnowledge, verifyKnowledge } from "./knowledge-manifest.mjs";

function fixture() {
  const root = mkdtempSync(join(tmpdir(), "kb-"));
  const lib = join(root, "books", "Reaper Manual");
  mkdirSync(join(lib, "sub"), { recursive: true });
  writeFileSync(join(lib, "a.md"), "# A\n\nhello");
  writeFileSync(join(lib, "sub", "b.txt"), "world");
  writeFileSync(join(lib, "pic.png"), "x");
  return { root, profile: { knowledge: ["books/Reaper Manual"] } };
}

test("slug and entries", () => {
  assert.equal(slug("Reaper Manual (v7)"), "reaper-manual-v7");
  const { root, profile } = fixture();
  const [e] = resolveEntries(profile, root);
  assert.equal(e.id, "reaper-manual");
  assert.equal(e.name, "Reaper Manual");
  assert.throws(() => resolveEntries({ knowledge: ["a/x", "b/x"] }, root), /duplicate/);
});

test("stage then verify, then tamper", () => {
  const { root, profile } = fixture();
  const out = join(root, "out");
  const m = stageKnowledge(profile, root, out);
  assert.equal(m.libraries[0].files.length, 2);
  assert.deepEqual(verifyKnowledge(out), []);
  writeFileSync(join(out, "reaper-manual", "a.md"), "tampered");
  assert.match(verifyKnowledge(out)[0], /hash mismatch/);
  writeFileSync(join(out, "reaper-manual", "extra.md"), "x");
  assert.ok(verifyKnowledge(out).some((p) => /not in manifest/.test(p)));
});

test("size summary and empty profile", () => {
  const { root, profile } = fixture();
  assert.deepEqual(knowledgeSize(profile, root), { libraries: 1, files: 2, bytes: 15 });
  assert.equal(stageKnowledge({}, root, join(root, "o2")), null);
});
