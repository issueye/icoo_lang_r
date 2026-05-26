const vscode = require("vscode");
const childProcess = require("child_process");
const fs = require("fs");
const os = require("os");
const path = require("path");

let diagnostics;
const timers = new Map();

function activate(context) {
  diagnostics = vscode.languages.createDiagnosticCollection("icoo");
  context.subscriptions.push(diagnostics);

  context.subscriptions.push(
    vscode.workspace.onDidOpenTextDocument(scheduleDiagnostics),
    vscode.workspace.onDidChangeTextDocument((event) => scheduleDiagnostics(event.document)),
    vscode.workspace.onDidSaveTextDocument(scheduleDiagnostics),
    vscode.workspace.onDidCloseTextDocument((document) => diagnostics.delete(document.uri)),
    vscode.workspace.onDidChangeConfiguration((event) => {
      if (event.affectsConfiguration("icoo")) {
        vscode.workspace.textDocuments.forEach(scheduleDiagnostics);
      }
    })
  );

  vscode.workspace.textDocuments.forEach(scheduleDiagnostics);
}

function deactivate() {
  timers.forEach((timer) => clearTimeout(timer));
  timers.clear();
  diagnostics?.dispose();
}

function scheduleDiagnostics(document) {
  if (!isIcooDocument(document)) {
    return;
  }
  if (!vscode.workspace.getConfiguration("icoo").get("diagnostics.enabled", true)) {
    diagnostics.delete(document.uri);
    return;
  }

  const key = document.uri.toString();
  const previous = timers.get(key);
  if (previous) {
    clearTimeout(previous);
  }
  timers.set(
    key,
    setTimeout(() => {
      timers.delete(key);
      runDiagnostics(document);
    }, 350)
  );
}

function runDiagnostics(document) {
  if (!isIcooDocument(document) || document.isClosed) {
    return;
  }

  const config = vscode.workspace.getConfiguration("icoo");
  const executable = resolveExecutable(config.get("executablePath", "icoo"), document);
  const tempDir = path.dirname(document.fileName);
  const tempFile = path.join(
    tempDir,
    `.icoo-vscode-check-${process.pid}-${Date.now()}-${path.basename(document.fileName)}`
  );

  try {
    fs.writeFileSync(tempFile, document.getText(), "utf8");
    const result = childProcess.spawnSync(executable, ["check", tempFile], {
      encoding: "utf8",
      cwd: workspaceFolderFor(document) || undefined,
      windowsHide: true,
      timeout: 10000
    });

    const output = `${result.stderr || ""}\n${result.stdout || ""}`;
    if (result.error) {
      diagnostics.set(document.uri, [toolDiagnostic(document, result.error.message)]);
      return;
    }
    diagnostics.set(document.uri, parseDiagnostics(document, output));
  } finally {
    fs.rmSync(tempFile, { force: true });
  }
}

function resolveExecutable(configured, document) {
  if (configured && configured !== "icoo") {
    return configured;
  }
  const folder = vscode.workspace.getWorkspaceFolder(document.uri);
  if (folder) {
    const candidate = path.join(
      folder.uri.fsPath,
      "target",
      "debug",
      process.platform === "win32" ? "icoo.exe" : "icoo"
    );
    if (fs.existsSync(candidate)) {
      return candidate;
    }
  }
  return configured || "icoo";
}

function parseDiagnostics(document, output) {
  const parsed = [];
  const pattern = /^(\d+):(\d+):\s+((?:lexer|parse|resolve|type|runtime) error:\s+.+)$/gm;
  let match;
  while ((match = pattern.exec(output)) !== null) {
    const line = Math.max(Number(match[1]) - 1, 0);
    const column = Math.max(Number(match[2]) - 1, 0);
    const range = wordRangeAt(document, line, column);
    parsed.push(new vscode.Diagnostic(range, match[3], vscode.DiagnosticSeverity.Error));
  }
  return parsed;
}

function wordRangeAt(document, line, column) {
  const safeLine = Math.min(line, Math.max(document.lineCount - 1, 0));
  const textLine = document.lineAt(safeLine);
  const safeColumn = Math.min(column, textLine.text.length);
  const range = document.getWordRangeAtPosition(new vscode.Position(safeLine, safeColumn));
  return range || new vscode.Range(safeLine, safeColumn, safeLine, safeColumn + 1);
}

function toolDiagnostic(document, message) {
  const range = document.lineAt(0).range;
  return new vscode.Diagnostic(
    range,
    `icoo diagnostics failed: ${message}. Set icoo.executablePath to your icoo binary.`,
    vscode.DiagnosticSeverity.Warning
  );
}

function workspaceFolderFor(document) {
  const folder = vscode.workspace.getWorkspaceFolder(document.uri);
  return folder?.uri.fsPath;
}

function isIcooDocument(document) {
  return document.languageId === "icoo" && document.uri.scheme === "file";
}

module.exports = { activate, deactivate };
