# Icoo Language

VS Code syntax highlighting, snippets, comments, and Problems diagnostics for Icoo `.icoo` files.

## Features

- Associates `.icoo` files with the `icoo` language id.
- Highlights keywords, declarations, classes, functions, strings, template expressions, numbers, built-in modules, operators, `match`, and ternary expressions.
- Provides bracket pairing, auto-closing strings/brackets, indentation rules, `//` line comments, `#` legacy comments, and `/* ... */` block comments.
- Provides snippets for functions, async functions, classes, control flow, `match`, imports, and printing.
- Runs `icoo check` for open `.icoo` files and reports lexer, parse, resolve, and type errors in Problems.

## Settings

- `icoo.executablePath`: path to the `icoo` executable used by diagnostics. Defaults to `icoo`.
- `icoo.diagnostics.enabled`: enables or disables Problems diagnostics. Defaults to `true`.

## Development

Open this folder in VS Code and press `F5` to launch an Extension Development Host.

To package the extension:

```bash
npm install -g @vscode/vsce
vsce package
```

The extension is implemented with plain JavaScript and does not need a build step.
