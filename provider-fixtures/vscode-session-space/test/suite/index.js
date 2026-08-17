'use strict';

const assert = require('assert');
const path = require('path');
const vscode = require('vscode');

const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

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

  // Provider-native editor/terminal/tab identities are deliberately not used as
  // SessionSpace, Project, AgentSession or Surface identity. The fixture proves
  // addressable provider Surfaces only; canonical refs remain AIKit-owned.
  controller.dispose();
  preview.dispose();
  terminal.dispose();
}

module.exports = { run };
