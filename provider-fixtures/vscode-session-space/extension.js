'use strict';

function activate() {
  // The conformance work lives in the extension-host test. This activation point
  // is intentionally empty: the fixture proves public VS Code APIs and does not
  // install a second AIKit workspace controller.
}

function deactivate() {}

module.exports = { activate, deactivate };
