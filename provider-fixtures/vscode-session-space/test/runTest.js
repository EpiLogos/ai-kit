'use strict';

const fs = require('fs');
const os = require('os');
const path = require('path');
const { runTests } = require('@vscode/test-electron');

async function main() {
  const root = path.resolve(__dirname, '..');
  const controlFile = path.join(
    os.tmpdir(),
    `aikit-vscode-provider-${process.pid}-${Date.now()}.addr`
  );
  fs.rmSync(controlFile, { force: true });
  process.env.AIKIT_VSCODE_PROVIDER_CONTROL_FILE = controlFile;

  try {
    await runTests({
      version: '1.133.0',
      extensionDevelopmentPath: root,
      extensionTestsPath: path.resolve(__dirname, 'suite', 'index.js'),
      launchArgs: [
        path.resolve(root, 'fixture.code-workspace'),
        '--disable-extensions'
      ]
    });
  } finally {
    fs.rmSync(controlFile, { force: true });
  }
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
