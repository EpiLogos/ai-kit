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
import { execFile, spawnSync } from "node:child_process";
import { watch, openSync, fstatSync, readSync, closeSync, type FSWatcher } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { StringDecoder } from "node:string_decoder";

/** Environment override for the dispatcher binary; defaults to PATH's aikit. */
const AIKIT_BIN = process.env.AIKIT_BIN || "aikit";

/** Per-dispatch bound: a hung dispatcher degrades to a no-op, never a hang. */
const DISPATCH_TIMEOUT_MS = 10_000;

/** customType of the persistent message injected context rides on. */
const INJECTED_MESSAGE_TYPE = "aikit-hook-carrier";
const PEER_MESSAGE_TYPE = "aikit-native-peer-handoff";
const HANDOFF_OFFER_TYPE = "aikit-native-handoff-offer";
const MAX_AUTOMATIC_PEER_TURNS = 8;

/** Native journals supply availability notifications; a bounded fallback
 * reconciles missed notifications. Inspired by my-pi's CoordinationPoller
 * (spences10/my-pi@261a9fd33109b79eafac98f08ec17bb14d1e2a16), without
 * importing its mailbox/database or treating a delivery as completed work.
 */
export class NativePeerDelivery {
  private timer: ReturnType<typeof setInterval> | undefined;
  private watcher: FSWatcher | undefined;
  private context: any;
  private epoch = 0;
  private inFlight = false;
  private stopped = true;
  constructor(private readonly pi: any) {}

  start(ctx: any) {
    this.stop();
    this.context = ctx;
    this.stopped = false;
    if (!process.env.OI_POSITION_REF || !process.env.OI_OCCUPANT_GENERATION) return;
    const state = join(process.env.AIKIT_HOME || join(homedir(), ".aikit"), "state");
    try { this.watcher = watch(state, () => { void this.poll(); }); } catch { /* fallback below */ }
    this.timer = setInterval(() => { void this.poll(); }, 5_000);
    this.timer.unref();
    void this.poll();
  }
  stop() {
    this.stopped = true;
    this.epoch++;
    if (this.timer) clearInterval(this.timer);
    this.timer = undefined;
    this.watcher?.close();
    this.watcher = undefined;
  }
  update(ctx: any) { this.context = ctx; }
  track(delivery: any) {
    if (!delivery?.text || !sessionIdOf(this.context)) return;
    // A native session offer records intent, never delivery. Only an actual
    // retained user/custom message carrying the complete text commits it.
    this.pi.appendEntry?.(HANDOFF_OFFER_TYPE,{schema:"aikit.pi-handoff-offer/v1",recipient_session_id:sessionIdOf(this.context),delivery});
  }
  private retainedOnDisk(entry: any): boolean {
    const path=this.context.sessionManager?.getSessionFile?.();
    if (!path || !entry?.id) return false;
    let fd: number|undefined;
    try {
      fd=openSync(path,"r");
      const size=fstatSync(fd).size;
      // Native history remains complete. This focused confirmation refuses
      // excess material rather than loading unbounded history into a turn.
      if (size>32*1024*1024) return false;
      const buffer=Buffer.alloc(64*1024);
      const decoder=new StringDecoder("utf8");
      let offset=0, line="",discard=false, validSession=false;
      while (offset<size) {
        const count=readSync(fd,buffer,0,Math.min(buffer.length,size-offset),offset);if(!count)break;offset+=count;
        const chunk=decoder.write(buffer.subarray(0,count));
        for(const part of chunk.split(/(?<=\n)/)) {
          if (!discard) line+=part;
          if(line.length>1024*1024){line="";discard=true;}
          if(!part.endsWith("\n"))continue;
          if(!discard) {
            let record:any;try{record=JSON.parse(line);}catch{record=null;}
            if(record?.type==="session") validSession=record.id===sessionIdOf(this.context);
            if(validSession && record?.id===entry.id) return JSON.stringify(record)===JSON.stringify(entry);
          }
          line="";discard=false;
        }
      }
    } catch { return false; }
    finally {if(fd!==undefined)closeSync(fd);}
    return false;
  }
  private call(commit?: unknown): Promise<any> {
    return new Promise((resolve) => {
      const child = execFile(AIKIT_BIN, ["--json", "gateway", "handoff", ...(commit ? ["--commit"] : [])], {
        cwd: cwdOf(this.context), timeout: DISPATCH_TIMEOUT_MS, maxBuffer: 1024 * 1024, encoding: "utf8",
      }, (error, stdout) => {
        if (error) return resolve(null);
        try { const reply = JSON.parse(stdout); resolve(reply.ok === true ? reply.data : null); } catch { resolve(null); }
      });
      if (commit) child.stdin?.end(JSON.stringify(commit));
      else child.stdin?.end();
    });
  }
  async poll(): Promise<void> {
    if (this.stopped || this.inFlight) return;
    this.inFlight = true;
    const epoch = this.epoch;
    const session = sessionIdOf(this.context);
    try {
      if (!session) return;
      const entries = this.context.sessionManager?.getBranch?.() || this.context.sessionManager?.getEntries?.() || [];
      const textOf = (content: any) => typeof content === "string" ? content : Array.isArray(content) ? content.filter((item: any)=>item.type==="text").map((item: any)=>item.text).join("\n") : "";
      const offer = await this.call();
      if (epoch !== this.epoch || this.stopped || session !== sessionIdOf(this.context)) return;
      const delivery = offer?.delivery;
      if (!delivery?.text || !Array.isArray(delivery.communique_refs) || delivery.communique_refs.length === 0) return;
      // Reconcile only retained carrying that overlaps this pending offer.
      // Historical committed entries must not cause repeated owner effects.
      for (const entry of entries) {
        const saved=entry.type==="custom" && entry.customType===HANDOFF_OFFER_TYPE ? entry.data : null;
        if (saved?.recipient_session_id===session && saved.delivery?.text
            && saved.delivery.communique_refs?.some((ref: string)=>delivery.communique_refs.includes(ref))
            && entries.some((candidate: any)=>candidate.type==="message" && candidate.message?.role==="user" && textOf(candidate.message.content).includes(saved.delivery.text) && this.retainedOnDisk(candidate))) {
          await this.call({delivery:saved.delivery,carried_text:saved.delivery.text});
          return; // success or uncertainty: reread the retained owner next time
        }
      }
      // Pi's actual session tree is the retained operation. Reconcile a crash
      // after persistence/before acknowledgement, never send the turn again.
      for (const entry of entries) {
        const prior = entry.type === "custom_message" && entry.customType === PEER_MESSAGE_TYPE ? entry : entry.message;
        const details = prior?.details;
        if (prior?.customType === PEER_MESSAGE_TYPE && details?.recipient_session_id === session
            && details.delivery?.communique_refs?.some((ref: string) => delivery.communique_refs.includes(ref))) {
          if (!this.retainedOnDisk(entry)) return; // memory-only new-session entries cannot confirm durable carrying
          const committed = await this.call({ delivery: details.delivery, carried_text: prior.content });
          if (!committed) return; // unavailable/uncertain: retain the same operation
          return; // obtain a fresh bounded offer after acknowledged material
        }
      }
      if (this.context.isIdle?.() !== true || this.context.hasPendingMessages?.()) return;
      const automaticTurns=entries.filter((entry:any)=>entry.type==="custom_message" && entry.customType===PEER_MESSAGE_TYPE && entry.details?.recipient_session_id===session).length;
      if (automaticTurns>=MAX_AUTOMATIC_PEER_TURNS) return; // remaining records stay pending; no unlimited peer exchange
      this.pi.sendMessage({
        customType: PEER_MESSAGE_TYPE,
        content: delivery.text, display: true,
        details: { schema: "aikit.pi-peer-handoff/v1", source: "native-gateway", authority: "peer-only", direct_user_authority: false,
          recipient_session_id: session, delivery },
      }, { triggerTurn: true, deliverAs: "followUp" });
      // The native message event/session tree confirms actual carrying before
      // commit. A queued message or offer is never marked delivered here.
    } catch { /* owner outage leaves the durable inbox pending */ }
    finally { this.inFlight = false; }
  }
}

/** One parsed dispatcher reply: the fields this carrier routes on. */
interface AikitDecision {
  allowed: boolean;
  denial: string | null;
  injected: string;
  communique_handoff?: unknown;
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
      communique_handoff: reply.communique_handoff,
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
  sendMessage?: (message: any, options?: any) => void;
  appendEntry?: (customType: string, data: any) => void;
}) {
  const peers = new NativePeerDelivery(pi);
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
    peers.start(ctx);
    const decision = dispatchAikit("SessionStart", jsonSafe({ ...eventObject, cwd: cwdOf(ctx), session_id: sessionIdOf(ctx) }));
    if (decision) {
      queueInjection(decision.injected);
    }
    // session_start has no deny channel in pi; a denial is recorded by the
    // dispatcher and can never stop a session here.
  });

  pi.on("session_shutdown", async (eventObject: any, ctx: any) => {
    peers.stop();
    dispatchAikit("SessionEnd", jsonSafe({ ...eventObject, cwd: cwdOf(ctx), session_id: sessionIdOf(ctx) }));
  });
  pi.on("session_before_switch", async () => { peers.stop(); });
  pi.on("agent_end", async (_event: any, ctx: any) => { peers.update(ctx); await peers.poll(); });
  pi.on("message_end", async (_event: any, ctx: any) => { peers.update(ctx); await peers.poll(); });

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
      peers.update(ctx);
      peers.track(decision.communique_handoff);
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
        session_id: sessionIdOf(ctx),
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
        session_id: sessionIdOf(ctx),
      }),
    );
    // Observed only: this carrier never rewrites results.
  });

  // --- compaction ---------------------------------------------------------

  pi.on("session_before_compact", async (eventObject: any, ctx: any) => {
    // The session's own transcript (branchEntries, preparation) is not sent:
    // the dispatcher needs which session is compacting and why, not its
    // whole history on stdin.
    const decision = dispatchAikit(
      "PreCompact",
      jsonSafe({
        reason: eventObject.reason,
        will_retry: eventObject.willRetry,
        cwd: cwdOf(ctx),
        session_id: sessionIdOf(ctx),
      }),
    );
    if (decision && !decision.allowed) {
      // The event supports a cancel, so a policy denial cancels; a system
      // failure above already degraded to no-op and the compaction proceeds.
      notify(ctx, `[aikit] compaction denied: ${decision.denial ?? "denied by the hook chain"}`);
      return { cancel: true };
    }
    return undefined; // never customise, never cancel without a denial
  });
}
