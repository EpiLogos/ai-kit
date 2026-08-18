'use strict';

const fs = require('fs');
const net = require('net');
const vscode = require('vscode');

const CONTROL_SCHEMA = 'aikit.working-environment-control/v1';
const PROVIDER_SCHEMA = 'aikit.working-environment-provider/v1';
const PROVIDER_REF = 'provider/ide/vscode-1-133';

const capabilities = {
  discover: true,
  open: true,
  focus: true,
  select: true,
  multi_project: true,
  editor_surface: true,
  terminal_surface: true,
  conversation_surface: true,
  diff_surface: true,
  preview_surface: true,
  test_surface: true,
  surface_attach_detach: true,
  agent_session_attach_detach: true,
  reconstruct: true
};

function activate(context) {
  const sessions = new Map();
  let nextPanel = 1;
  let focusedNativeId = null;

  function canonicalAgentSession(binding) {
    const canonical = binding && binding.session && binding.session.agent_session;
    if (typeof canonical !== 'string' || canonical.length === 0) {
      throw new Error('an explicit canonical AgentSession ref is required');
    }
    if (typeof binding.surface !== 'string' || binding.surface.length === 0) {
      throw new Error('an explicit canonical conversation Surface ref is required');
    }
    if (typeof binding.session.native_session_id !== 'string' || binding.session.native_session_id.length === 0) {
      throw new Error('a connection-native session id is required');
    }
    return canonical;
  }

  function escapeHtml(value) {
    return String(value)
      .replaceAll('&', '&amp;')
      .replaceAll('<', '&lt;')
      .replaceAll('>', '&gt;')
      .replaceAll('"', '&quot;');
  }

  function createConversation(binding) {
    const canonical = canonicalAgentSession(binding);
    const nativeId = binding.session.native_session_id;
    const panelId = `vscode-conversation-${nextPanel++}`;
    const panel = vscode.window.createWebviewPanel(
      'aikitSessionSpaceAgentSession',
      `AIKit AgentSession · ${nativeId}`,
      vscode.ViewColumn.Beside,
      { enableScripts: false }
    );
    panel.webview.html = `<!doctype html><html><body><h1>AIKit AgentSession</h1><p>${escapeHtml(canonical)}</p><p>${escapeHtml(nativeId)}</p></body></html>`;
    const record = {
      canonical,
      nativeId,
      surface: binding.surface,
      openedAs: binding.session.opened_as,
      provenance: Array.isArray(binding.session.provenance) ? binding.session.provenance.slice() : [],
      panel,
      panelId
    };
    sessions.set(canonical, record);
    focusedNativeId = panelId;
    panel.onDidChangeViewState((event) => {
      if (event.webviewPanel.active) {
        focusedNativeId = panelId;
      }
    });
    panel.onDidDispose(() => {
      const current = sessions.get(canonical);
      if (current && current.panelId === panelId) {
        sessions.delete(canonical);
      }
      if (focusedNativeId === panelId) {
        focusedNativeId = null;
      }
    });
    return record;
  }

  function removeRecord(record) {
    sessions.delete(record.canonical);
    if (focusedNativeId === record.panelId) {
      focusedNativeId = null;
    }
    record.panel.dispose();
  }

  function attachAgentSession(binding) {
    const canonical = canonicalAgentSession(binding);
    if (sessions.has(canonical)) {
      throw new Error(`canonical AgentSession ${canonical} is already attached`);
    }
    createConversation(binding);
  }

  function detachAgentSession(agentSession) {
    const record = sessions.get(agentSession);
    if (!record) {
      throw new Error(`canonical AgentSession ${agentSession} is not attached`);
    }
    removeRecord(record);
  }

  function rebindAgentSession(binding) {
    const canonical = canonicalAgentSession(binding);
    const previous = sessions.get(canonical);
    if (!previous) {
      throw new Error(`canonical AgentSession ${canonical} is not attached and cannot be rebound`);
    }
    removeRecord(previous);
    createConversation(binding);
  }

  function focusSurface(surface) {
    const record = Array.from(sessions.values()).find((candidate) => candidate.surface === surface);
    if (!record) {
      throw new Error(`canonical Surface ${surface} is not attached`);
    }
    record.panel.reveal(vscode.ViewColumn.Beside, false);
    focusedNativeId = record.panelId;
  }

  function detachSurface(surface) {
    const record = Array.from(sessions.values()).find((candidate) => candidate.surface === surface);
    if (!record) {
      throw new Error(`canonical Surface ${surface} is not attached`);
    }
    removeRecord(record);
  }

  function observation() {
    const bindings = [];
    for (const record of sessions.values()) {
      bindings.push({
        kind: 'agent-session',
        native_id: record.nativeId,
        canonical_ref: record.canonical,
        provenance: [
          `explicit VS Code AgentSession provider binding opened-as=${record.openedAs}`,
          ...record.provenance
        ]
      });
      bindings.push({
        kind: 'surface',
        native_id: record.panelId,
        canonical_ref: record.surface,
        provenance: ['real VS Code webview conversation Surface; provider-local panel id is provenance']
      });
    }
    return {
      schema: PROVIDER_SCHEMA,
      provider: PROVIDER_REF,
      provider_version: vscode.version,
      health: 'healthy',
      capabilities,
      bindings,
      focused_native_id: focusedNativeId,
      provenance: [
        `VS Code ${vscode.version} real Extension Host`,
        'AgentSession identity supplied by caller; extension binds only provider encounter state'
      ]
    };
  }

  context.subscriptions.push(
    vscode.commands.registerCommand('aikit.sessionSpace.attachAgentSession', attachAgentSession),
    vscode.commands.registerCommand('aikit.sessionSpace.detachAgentSession', detachAgentSession),
    vscode.commands.registerCommand('aikit.sessionSpace.rebindAgentSession', rebindAgentSession)
  );

  const controlFile = process.env.AIKIT_VSCODE_PROVIDER_CONTROL_FILE;
  if (!controlFile) {
    return;
  }

  async function handle(request) {
    if (!request || request.schema !== CONTROL_SCHEMA) {
      throw new Error(`control schema must be ${CONTROL_SCHEMA}`);
    }
    switch (request.operation) {
      case 'describe':
        return { provider: PROVIDER_REF, capabilities };
      case 'observe':
      case 'open':
        return { observation: observation() };
      case 'focus-surface':
        focusSurface(request.surface);
        return {};
      case 'detach-surface':
        detachSurface(request.surface);
        return {};
      case 'attach-agent-session':
        attachAgentSession(request.binding);
        return {};
      case 'detach-agent-session':
        detachAgentSession(request.agent_session);
        return {};
      case 'rebind-agent-session':
        rebindAgentSession(request.binding);
        return {};
      default:
        throw new Error(`unsupported control operation ${request.operation}`);
    }
  }

  const server = net.createServer((socket) => {
    let buffer = '';
    let handled = false;
    socket.setEncoding('utf8');
    socket.on('data', (chunk) => {
      if (handled) return;
      buffer += chunk;
      const newline = buffer.indexOf('\n');
      if (newline < 0) return;
      handled = true;
      const line = buffer.slice(0, newline);
      Promise.resolve()
        .then(() => handle(JSON.parse(line)))
        .then((payload) => {
          socket.end(`${JSON.stringify({ schema: CONTROL_SCHEMA, ok: true, ...payload })}\n`);
        })
        .catch((error) => {
          socket.end(`${JSON.stringify({ schema: CONTROL_SCHEMA, ok: false, error: String(error.message || error) })}\n`);
        });
    });
  });

  server.listen(0, '127.0.0.1', () => {
    const address = server.address();
    if (!address || typeof address === 'string') {
      throw new Error('VS Code provider control server did not receive a TCP address');
    }
    fs.writeFileSync(controlFile, `127.0.0.1:${address.port}\n`, 'utf8');
  });

  context.subscriptions.push({
    dispose() {
      for (const record of Array.from(sessions.values())) {
        removeRecord(record);
      }
      server.close();
      try {
        fs.rmSync(controlFile, { force: true });
      } catch (_) {
        // Fixture cleanup must not counterfeit provider lifecycle semantics.
      }
    }
  });
}

function deactivate() {}

module.exports = { activate, deactivate };
