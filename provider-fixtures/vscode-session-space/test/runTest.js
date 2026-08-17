'use strict';

const path = require('path');
const { runTests } = require('@vscode/test-electron');

async function main() {
  const root = path.resolve(__dirname, '..');
  await runTests({
    version: '1.133.0',
    extensionDevelopmentPath: root,
    extensionTestsPath: path.resolve(__dirname, 'suite', 'index.js'),
    launchArgs: [
      path.resolve(root, 'fixture.code-workspace'),
      '--disable-extensions'
    ]
  });
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
