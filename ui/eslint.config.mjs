import babelParser from "@babel/eslint-parser";
import eslint from "@eslint/js";

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
    rules: {
      ...eslint.configs.recommended.rules,
      "no-undef": "off",
      "no-unused-vars": "off",
    },
  },
];
