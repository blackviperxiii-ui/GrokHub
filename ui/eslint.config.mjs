import babelParser from "@babel/eslint-parser";
import eslint from "@eslint/js";
import reactHooks from "eslint-plugin-react-hooks";

/** The leftmost identifier of `A.B.C` in a type or JSX name. */
function root(node) {
  let n = node;
  while (n && (n.type === "TSQualifiedName" || n.type === "JSXMemberExpression")) {
    n = n.type === "TSQualifiedName" ? n.left : n.object;
  }
  return n && (n.type === "Identifier" || n.type === "JSXIdentifier") ? n.name : null;
}

/** Marks names used by types and JSX tags, which `no-unused-vars` cannot see
 * through the Babel parser (it has no TypeScript scope analysis). */
const usesTypesAndJsx = {
  rules: {
    "uses-types-and-jsx": {
      meta: { type: "problem", schema: [] },
      create(context) {
        const used = (node, name) => {
          if (name) context.sourceCode.markVariableAsUsed(name, node);
        };
        return {
          TSTypeReference: (n) => used(n, root(n.typeName)),
          TSTypeQuery: (n) => used(n, root(n.exprName)),
          TSExpressionWithTypeArguments: (n) => used(n, root(n.expression)),
          TSInterfaceHeritage: (n) => used(n, root(n.expression)),
          TSClassImplements: (n) => used(n, root(n.expression)),
          // A TypeScript `this` parameter is read through `this`.
          ThisExpression: (n) => used(n, "this"),
          JSXOpeningElement: (n) => {
            // Lowercase tags are HTML elements, not variables.
            const name = root(n.name);
            if (name && (n.name.type !== "JSXIdentifier" || /^[A-Z_$]/.test(name))) used(n, name);
          },
        };
      },
    },
  },
};

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
    plugins: { ...reactHooks.configs.flat.recommended.plugins, local: usesTypesAndJsx },
    rules: {
      ...eslint.configs.recommended.rules,
      ...reactHooks.configs.flat.recommended.rules,
      "no-undef": "off",
      "local/uses-types-and-jsx": "error",
      "no-unused-vars": [
        "error",
        {
          args: "after-used",
          argsIgnorePattern: "^_",
          caughtErrors: "all",
          ignoreRestSiblings: true,
        },
      ],
    },
  },
];
