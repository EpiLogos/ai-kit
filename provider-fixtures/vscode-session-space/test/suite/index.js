'use strict';

const assert = require('assert');
const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');
const vscode = require('vscode');

const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function waitForControlAddress() {
  const controlFile = process.env.AIKIT_VSCODE_PROVIDER_CONTROL_FILE;
  assert.ok(controlFile, 'VS Code provider control file must be configured');
  for (let attempt = 0; attempt < 100; attempt += 1) {
    if (fs.existsSync(controlFile)) {
      const address = fs.readFileSync(controlFile, 'utf8').trim();
      if (address) return address;
    }
    await delay(50);
  }
  throw new Error(`VS Code provider control address was not published at ${controlFile}`);
}

async function run() {
  const folders = vscode.workspace.workspaceFolders || [];
  assert.strictEqual(folders.length, 2, 'VS Code must expose the two Project roots');
  assert.deepStrictEqual(folders.map((folder) => folder.name).sort(), ['project-a', 'project-b']);

  const a = vscode.Uri.file(path.join(folders.find((folder) => folder.name === 'project-a').uri.fsPath, 'main.txt'));
  const b = vscode.Uri.file(path.join(folders.find((folder) => folder.name === 'project-b').uri.fsPath, 'main.txt'));

  const document = await vscode.workspace.openTextDocument(a);
  const editor = await vscode.window.showTextDocument(document, { preview: false });
  editor.selection = new vscode.Selection(0, 0, 0, 5);
  assert.strictEqual(vscode.window.activeTextEditor.document.uri.fsPath, a.fsPath);
  assert.strictEqual(vscode.window.activeTextEditor.selection.active.character, 5);

  const terminal = vscode.window.createTerminal({ name: 'AIKit SessionSpace terminal' });
  terminal.show(true);
  await delay(150);
  assert.ok(vscode.window.terminals.includes(terminal), 'integrated terminal must be a live VS Code Surface');
  assert.ok(vscode.window.activeTerminal, 'VS Code must expose terminal focus');

  await vscode.commands.executeCommand('vscode.diff', a, b, 'AIKit Project diff');
  await delay(150);
  assert.ok(vscode.window.tabGroups.all.length > 0, 'tab groups must expose editor/diff placement');
  assert.ok(vscode.window.tabGroups.activeTabGroup.activeTab, 'a focused tab must be observable');

  const preview = vscode.window.createWebviewPanel(
    'aikitSessionSpacePreview',
    'AIKit Preview',
    vscode.ViewColumn.Beside,
    { enableScripts: false }
  );
  preview.webview.html = '<!doctype html><html><body>AIKit preview</body></html>';
  assert.ok(preview.visible, 'webview preview must be a real visible Surface');

  const controller = vscode.tests.createTestController('aikitSessionSpaceTests', 'AIKit SessionSpace Tests');
  const item = controller.createTestItem('provider-proof', 'provider proof', a);
  controller.items.add(item);
  assert.ok(controller.items.get('provider-proof'), 'VS Code test surface must be addressable');

  assert.ok(vscode.chat, 'current VS Code must expose the Chat API namespace');
  assert.strictEqual(
    typeof vscode.chat.createChatParticipant,
    'function',
    'current VS Code must expose agent/conversation participant creation'
  );

  // The rich IDE requirement is not closed by Chat API visibility alone. Drive
  // AIKit's external provider control seam into this real Extension Host. The
  // Rust probe supplies caller-owned canonical AgentSession/Surface refs and an
  // already-existing connection-native session binding, then proves attach,
  // detach, reattach and rebind without VS Code minting canonical identity.
  const address = await waitForControlAddress();
  const probe = process.env.AIKIT_VSCODE_PROVIDER_PROBE;
  assert.ok(probe, 'AIKit working-environment control probe must be configured');
  const result = spawnSync(probe, [], {
    env: {
      ...process.env,
      AIKIT_WORKING_ENVIRONMENT_CONTROL_ADDR: address
    },
    encoding: 'utf8',
    timeout: 30000
  });
  assert.strictEqual(
    result.status,
    0,
    `AIKit provider lifecycle probe failed\nstdout:\n${result.stdout || ''}\nstderr:\n${result.stderr || ''}`
  );
  assert.match(result.stdout || '', /AgentSession lifecycle: .*PASS/);

  // Provider-native editor/terminal/tab/panel/session identities remain
  // provenance. Canonical refs are supplied by AIKit; ACP/native continuity is
  // supplied below the IDE through aikit.connection-adapter/v1.
  controller.dispose();
  preview.dispose();
  terminal.dispose();
}

module.exports = { run };
