import { ESLint } from "eslint";
import { expect, test } from "vitest";

async function lint(code: string, filePath: string) {
  const eslint = new ESLint({ cwd: process.cwd() });
  const [result] = await eslint.lintText(code, { filePath });
  return result?.messages.map((m) => m.ruleId) ?? [];
}

test("feature code may not import invoke", async () => {
  const rules = await lint(
    'import { invoke } from "@tauri-apps/api/core";\nexport const x = invoke;\n',
    "src/features/chat/bad.ts",
  );
  expect(rules).toContain("no-restricted-imports");
});

test("feature code may not use generated commands", async () => {
  const rules = await lint(
    'import { commands } from "@/bindings";\nexport const x = commands;\n',
    "src/features/chat/bad.ts",
  );
  expect(rules).toContain("no-restricted-imports");
});

test("type-only imports from bindings are fine, and the transport may import invoke", async () => {
  expect(await lint('import type { AppInfo } from "@/bindings";\nexport type T = AppInfo;\n', "src/features/x.ts")).toEqual([]);
  expect(
    await lint('import { invoke } from "@tauri-apps/api/core";\nexport const x = invoke;\n', "src/lib/transport/t.ts"),
  ).toEqual([]);
});
