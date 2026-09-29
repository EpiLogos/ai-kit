// AIKit hook carrier for pi — the owned vehicle that translates pi's native
// extension events into `aikit hook dispatch pi <AIKitEvent>`.
//
// Capsule: hook/aikit/pi-extension-carrier. This file is the capsule payload;
// the projection content-addresses it (`aikit-hook-carrier-<sha256-12>.ts`)
// and registers it through the `extensions` array of pi's global
// `~/.pi/agent/settings.json`. Pi loads it through jiti; it is written in
// TypeScript but deliberately imports nothing, so loading never depends on
// module resolution inside pi's package tree.
//
// TRUST POSTURE — read before trusting a revision. An extension runs with the
// user's full permissions inside every pi session; that is pi's own model, not
// something this carrier adds. What this carrier can do inside a session:
//   - spawn `aikit --json hook dispatch pi <Event>` with the event JSON on
//     stdin and read the verdict from the machine envelope's `data`;
//   - forward a denial as a tool_call block (pi's { block: true, reason });
//   - forward a denial of typed input as { action: "handled" } plus a notice
//     (pi has no prompt-block-with-reason; this is the nearest honest stop);
//   - forward a denial of compaction as { cancel: true };
//   - forward injected context: as an input transform on user prompts (the
//     event's own channel), and on events with no content channel via a
//     persistent message returned from `before_agent_start` (pi's documented
//     context-injection seam);
//   - observe tool results, session shutdown and compaction.
// What it deliberately cannot do: rewrite tool arguments, rewrite tool
// results, replace the system prompt, or cancel anything the dispatcher did
// not deny. The dispatcher's decision is the only authority; where pi offers
// no channel for a verdict the verdict is recorded, not improvised.
//
// The mitigation for running with session permissions is AIKit's capsule
// trust gate: this payload ships as a `hook` capsule, every new revision is
// Unseen until `aikit trust record` reviews it, and an untrusted or blocked
// revision is never projected (and a blocked one is swept) — pi never loads
// code the owner has not reviewed.
//
// Fail-open law: a missing aikit binary, a timeout, a crash, or an unreadable
// reply degrades to a no-op. A session must never die because AIKit is
// absent; the only thing allowed to stop an action is a dispatcher DENIAL.

// pi's documented available imports include the Node built-ins; nothing else
// is imported, so loading never depends on module resolution.
import { spawnSync } from "node:child_process";

/** Environment override for the dispatcher binary; defaults to PATH's aikit. */
const AIKIT_BIN = process.env.AIKIT_BIN || "aikit";

/** Per-dispatch bound: a hung dispatcher degrades to a no-op, never a hang. */
const DISPATCH_TIMEOUT_MS = 10_000;

/** customType of the persistent message injected context rides on. */
const INJECTED_MESSAGE_TYPE = "aikit-hook-carrier";

/** One parsed dispatcher reply: the fields this carrier routes on. */
interface AikitDecision {
  allowed: boolean;
  denial: string | null;
  injected: string;
}

/**
 * Spawn the dispatcher for one event. Returns null on any system failure —
 * absent binary, timeout, crash, unparsable reply — which every caller
 * treats as "no verdict, continue". Only a parsed reply is a verdict.
 *
 * The dispatcher speaks its machine envelope (`--json`): plain mode is the
 * calling harness's protocol and keeps stdout empty on an allowance, which
 * no pi channel can read. The envelope arrives for both verdict shapes
 * (exit 0 allow, exit 2 deny) with the verdict fields under `data`.
 */
function dispatchAikit(event: string, payload: unknown): AikitDecision | null {
  let body: string;
  try {
    body = JSON.stringify(payload) ?? "{}";
  } catch {
    return null; // a payload that cannot serialise is not a verdict
  }
  try {
    const result = spawnSync(AIKIT_BIN, ["--json", "hook", "dispatch", "pi", event], {
      input: body,
      timeout: DISPATCH_TIMEOUT_MS,
      maxBuffer: 4 * 1024 * 1024,
      encoding: "utf8",
      windowsHide: true,
    });
    if (result.error || typeof result.stdout !== "string") {
      return null;
    }
    // Exit 0 (allow) and exit 2 (deny) both carry the envelope; any other
    // status is a dispatcher fault, not a verdict.
    if (result.status !== 0 && result.status !== 2) {
      return null;
    }
    const envelope = JSON.parse(result.stdout) as { data?: Record<string, unknown> };
    const reply = (envelope.data ?? {}) as Record<string, unknown>;
    if (typeof reply.allowed !== "boolean") {
      return null;
    }
    return {
      allowed: reply.allowed,
      denial: typeof reply.denial === "string" ? reply.denial : null,
      injected: typeof reply.injected === "string" ? reply.injected : "",
    };
  } catch {
    return null;
  }
}

/** Strip undefined values so a payload serialises without holes. */
function jsonSafe(value: Record<string, unknown>): Record<string, unknown> {
  const clean: Record<string, unknown> = {};
  for (const [key, item] of Object.entries(value)) {
    if (item !== undefined) {
      clean[key] = item;
    }
  }
  return clean;
}

/**
 * The session's working directory: pi hands every handler ctx.cwd; the
 * dispatcher's hooks read it to scope context and continuity work.
 */
function cwdOf(ctx: { cwd?: string }): string {
  return ctx.cwd || process.cwd();
}

/**
 * The resident session's id: pi exposes it through ctx.sessionManager.
 * The dispatcher binds SessionStart inhabitation (World/Position/current-work
 * lean entry) to the payload's session id; without it the occupant receives
 * only the historical temporal floor. Absent or throwing reads degrade to
 * undefined — the dispatcher then answers with the unbound floor, never a
 * fabricated identity.
 */
function sessionIdOf(ctx: {
  sessionManager?: { getSessionId?: () => string | null | undefined };
}): string | undefined {
  try {
    const id = ctx.sessionManager?.getSessionId?.();
    return typeof id === "string" && id.trim() ? id : undefined;
  } catch {
    return undefined;
  }
}

/**
 * Show a notice where pi has a UI. Fire-and-forget; never awaited, never
 * allowed to throw into the handler.
 */
function notify(ctx: { hasUI?: boolean; ui?: { notify?: (message: string, level?: string) => void } }, message: string): void {
  try {
    if (ctx.hasUI !== false && typeof ctx.ui?.notify === "function") {
      ctx.ui.notify(message, "warning");
    }
  } catch {
    // a notice is never worth a session
  }
}

/**
 * The carrier factory. Pi calls it once per extension binding; all state
 * (the pending-injection buffer) lives in this closure so a session switch —
 * shutdown, rebind, fresh session_start — starts from a clean buffer.
 */
export default function (pi: {
  on: (event: string, handler: (eventObject: any, ctx: any) => unknown) => void;
}) {
  // Injected context waiting for pi's next content channel. Queued by events
  // that have no content channel of their own (session_start, tool_call) and
  // delivered as one persistent message at the next `before_agent_start`.
  let pendingInjections: string[] = [];

  const queueInjection = (text: string) => {
    if (text && text.trim()) {
      pendingInjections.push(text);
    }
  };

  // --- session lifecycle -------------------------------------------------

  pi.on("session_start", async (eventObject: any, ctx: any) => {
    const decision = dispatchAikit("SessionStart", jsonSafe({ ...eventObject, cwd: cwdOf(ctx), session_id: sessionIdOf(ctx) }));
    if (decision) {
      queueInjection(decision.injected);
    }
    // session_start has no deny channel in pi; a denial is recorded by the
    // dispatcher and can never stop a session here.
  });

  pi.on("session_shutdown", async (eventObject: any, ctx: any) => {
    dispatchAikit("SessionEnd", jsonSafe({ ...eventObject, cwd: cwdOf(ctx), session_id: sessionIdOf(ctx) }));
  });

  // --- context injection seam ---------------------------------------------

  pi.on("before_agent_start", async (_eventObject: any, _ctx: any) => {
    if (pendingInjections.length === 0) {
      return undefined;
    }
    const content = pendingInjections.join("\n\n");
    pendingInjections = [];
    return {
      message: {
        customType: INJECTED_MESSAGE_TYPE,
        content,
        display: false,
      },
    };
  });

  // --- user input ----------------------------------------------------------

  pi.on("input", async (eventObject: any, ctx: any) => {
    const decision = dispatchAikit(
      "UserPromptSubmit",
      jsonSafe({ text: eventObject.text, source: eventObject.source, cwd: cwdOf(ctx), session_id: sessionIdOf(ctx) }),
    );
    if (!decision) {
      return undefined; // system failure: fail open, the prompt proceeds
    }
    if (!decision.allowed) {
      notify(ctx, `[aikit] prompt not submitted: ${decision.denial ?? "denied by the hook chain"}`);
      return { action: "handled" }; // pi's nearest stop for a prompt
    }
    if (decision.injected && decision.injected.trim()) {
      // The event's own transform channel: append after the user's text so
      // the leading token (the only one pi parses as a command) is untouched.
      return {
        action: "transform",
        text: `${eventObject.text}\n\n${decision.injected}`,
      };
    }
    return undefined;
  });

  // --- tools -----------------------------------------------------------------

  pi.on("tool_call", async (eventObject: any, ctx: any) => {
    const decision = dispatchAikit(
      "PreToolUse",
      jsonSafe({
        tool_name: eventObject.toolName,
        tool_call_id: eventObject.toolCallId,
        input: eventObject.input,
        cwd: cwdOf(ctx),
      }),
    );
    if (!decision) {
      return undefined; // system failure: fail open
    }
    if (!decision.allowed) {
      return { block: true, reason: decision.denial ?? "denied by the aikit hook chain" };
    }
    queueInjection(decision.injected); // no content channel here; next turn carries it
    return undefined;
  });

  pi.on("tool_result", async (eventObject: any, ctx: any) => {
    dispatchAikit(
      "PostToolUse",
      jsonSafe({
        tool_name: eventObject.toolName,
        tool_call_id: eventObject.toolCallId,
        is_error: eventObject.isError,
        cwd: cwdOf(ctx),
      }),
    );
    // Observed only: this carrier never rewrites results.
  });

  // --- compaction ---------------------------------------------------------

  pi.on("session_before_compact", async (eventObject: any, ctx: any) => {
    const decision = dispatchAikit("PreCompact", jsonSafe({ ...eventObject, cwd: cwdOf(ctx) }));
    if (decision && !decision.allowed) {
      // The event supports a cancel, so a policy denial cancels; a system
      // failure above already degraded to no-op and the compaction proceeds.
      notify(ctx, `[aikit] compaction denied: ${decision.denial ?? "denied by the hook chain"}`);
      return { cancel: true };
    }
    return undefined; // never customise, never cancel without a denial
  });
}
