import babelParser from "@babel/eslint-parser";
import eslint from "@eslint/js";
import reactHooks from "eslint-plugin-react-hooks";

export default [
  {
    files: ["src/**/*.{ts,tsx}"],
    languageOptions: {
      parser: babelParser,
      parserOptions: {
        requireConfigFile: false,
        babelOptions: {
          parserOpts: {
            plugins: [["typescript", { isTSX: true }], "jsx"],
          },
        },
      },
    },
    // Rules of Hooks plus the React Compiler's checks (refs, purity, setState in effects…).
    plugins: reactHooks.configs.flat.recommended.plugins,
    rules: {
      ...eslint.configs.recommended.rules,
      ...reactHooks.configs.flat.recommended.rules,
      "no-undef": "off",
      "no-unused-vars": "off",
    },
  },
];
