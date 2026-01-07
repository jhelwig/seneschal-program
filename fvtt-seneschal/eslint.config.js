export default [
  {
    languageOptions: {
      ecmaVersion: 2022,
      sourceType: "module",
      globals: {
        // Browser globals
        window: "readonly",
        document: "readonly",
        console: "readonly",
        fetch: "readonly",
        AbortController: "readonly",
        TextDecoder: "readonly",
        Event: "readonly",
        // Foundry VTT globals
        game: "readonly",
        ui: "readonly",
        canvas: "readonly",
        foundry: "readonly",
        Hooks: "readonly",
        Application: "readonly",
        Actor: "readonly",
        Item: "readonly",
        JournalEntry: "readonly",
        Scene: "readonly",
        RollTable: "readonly",
        Macro: "readonly",
        Playlist: "readonly",
        Roll: "readonly",
        ChatMessage: "readonly",
        CONFIG: "readonly",
        CONST: "readonly",
        // Optional dependencies
        marked: "readonly",
      },
    },
    rules: {
      "no-unused-vars": ["warn", { argsIgnorePattern: "^_", varsIgnorePattern: "^_" }],
      "no-console": "off",
      "prefer-const": "error",
      "no-var": "error",
      eqeqeq: ["error", "always"],
      curly: ["error", "multi-line"],
      "no-throw-literal": "error",
    },
  },
  {
    ignores: ["node_modules/", "*.min.js"],
  },
];
