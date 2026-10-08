// eslint-plugin-boundaries — defense in depth alongside the structural test.
// The structural test is the source of truth; this catches the same violations
// in the editor for faster feedback.
//
// Note: requires `eslint-plugin-boundaries` ≥ 5. Install if missing:
//   npm i -D eslint-plugin-boundaries

import boundaries from "eslint-plugin-boundaries";

export default [
  {
    plugins: { boundaries },
    settings: {
      "boundaries/elements": [
        { type: "types",   pattern: "src/*/types/**" },
        { type: "config",  pattern: "src/*/config/**" },
        { type: "repo",    pattern: "src/*/repo/**" },
        { type: "service", pattern: "src/*/service/**" },
        { type: "runtime", pattern: "src/*/runtime/**" },
        { type: "ui",      pattern: "src/*/ui/**" },
      ],
      "boundaries/include": ["src/**/*"],
    },
    rules: {
      // eslint-plugin-boundaries v5: rule name is `element-types`, not `dependencies`.
      // Schema: `{ from: ["t1"], allow: ["t2", "t3"] }` — flat arrays of element-type names.
      "boundaries/element-types": [2, {
        default: "disallow",
        rules: [
          { from: ["ui"],      allow: ["runtime", "service", "config", "types"] },
          { from: ["runtime"], allow: ["service", "repo", "config", "types"] },
          { from: ["service"], allow: ["repo", "config", "types"] },
          { from: ["repo"],    allow: ["config", "types"] },
          { from: ["config"],  allow: ["types"] },
          { from: ["types"],   disallow: ["*"] },
        ],
      }],
    },
  },
];
