import js from "@eslint/js";
import reactHooks from "eslint-plugin-react-hooks";
import globals from "globals";
import tseslint from "typescript-eslint";

export default tseslint.config(
  { ignores: ["dist", "src-tauri", "src/bindings.ts", "node_modules"] },
  js.configs.recommended,
  ...tseslint.configs.strict,
  {
    files: ["**/*.{ts,tsx}"],
    languageOptions: { globals: { ...globals.browser } },
    plugins: { "react-hooks": reactHooks },
    rules: {
      ...reactHooks.configs.recommended.rules,
      "@typescript-eslint/no-explicit-any": "error",
      "@typescript-eslint/consistent-type-imports": "error",
    },
  },
  {
    files: ["public/sw.js"],
    languageOptions: { globals: { ...globals.serviceworker } },
  },
  {
    // Feature code goes through `src/lib/api.ts` (Transport facade), never `invoke` or the generated
    // `commands`/`events`: the same UI must run over Tauri and, later, the LAN Share WebSocket.
    files: ["src/**/*.{ts,tsx}"],
    ignores: ["src/lib/transport/**", "src/bindings.ts"],
    rules: {
      "no-restricted-imports": [
        "error",
        {
          paths: [
            { name: "@tauri-apps/api/core", importNames: ["invoke", "Channel"], message: "Use src/lib/api.ts (Transport facade)." },
            { name: "@tauri-apps/api/event", message: "Use api.*.on... from src/lib/api.ts." },
            { name: "@tauri-apps/api", message: "Use src/lib/api.ts (Transport facade)." },
            { name: "@/bindings", importNames: ["commands", "events"], message: "Use src/lib/api.ts; import only types from bindings." },
          ],
        },
      ],
    },
  },
);
