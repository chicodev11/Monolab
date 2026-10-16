import js from "@eslint/js";
import reactHooks from "eslint-plugin-react-hooks";
import globals from "globals";
import tseslint from "typescript-eslint";

export default tseslint.config(
  { ignores: ["dist/**", "src-tauri/**", "release/**", "node_modules/**", "addon/**", "playwright-report/**", "test-results/**", "scratchpad/**",
    "src/lib/enRust.ts"] },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ["**/*.{ts,tsx}"],
    plugins: { "react-hooks": reactHooks },
    languageOptions: { globals: { ...globals.browser, ...globals.node } },
    rules: {
      ...reactHooks.configs.recommended.rules,
      "react-hooks/set-state-in-effect": "off",
      "react-hooks/refs": "off",
      "react-hooks/incompatible-library": "off",
      "@typescript-eslint/no-unused-vars": ["error", { argsIgnorePattern: "^_", varsIgnorePattern: "^_", caughtErrors: "none" }],
      "no-empty": ["error", { allowEmptyCatch: true }],
    },
  },
  {
    files: ["site/**/*.js"],
    languageOptions: { globals: { ...globals.browser } },
    rules: { "no-empty": ["error", { allowEmptyCatch: true }] },
  },
  {
    files: ["scripts/**/*.mjs", "*.config.{js,ts}"],
    languageOptions: { globals: { ...globals.node } },
    rules: { "no-empty": ["error", { allowEmptyCatch: true }] },
  },
);
