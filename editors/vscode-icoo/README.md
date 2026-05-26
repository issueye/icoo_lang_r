# Icoo Language

VS Code syntax highlighting and editing support for Icoo `.icoo` files.

## Features

- Associates `.icoo` files with the `icoo` language id.
- Highlights keywords, declarations, classes, functions, strings, template expressions, numbers, built-in modules, operators, `match`, and ternary expressions.
- Provides bracket pairing, auto-closing strings/brackets, indentation rules, and `#` line comments.
- Provides snippets for functions, async functions, classes, control flow, `match`, imports, and printing.

## Development

Open this folder in VS Code and press `F5` to launch an Extension Development Host.

To package the extension:

```bash
npm install -g @vscode/vsce
vsce package
```

The extension is intentionally a grammar-only package, so it does not need a build step.
