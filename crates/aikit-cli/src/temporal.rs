//! Causal temporal re-grounding for Agent lifecycle hooks.
//!
//! Central owns NOW and the Day; AIKit owns the lifecycle delivery plane. This
//! module composes those existing responsibilities by reading Central afresh for
//! every session-start, prompt-turn and pre-compaction event, then returning the
//! bounded owner reading through AIKit's existing hook injection channel.
//!
//! A prompt turn carries the ground only when it changed since this session
//! last received it ([`FloorLedger`]): re-reading is causal, re-sending the
//! same ground every turn is not. A body occupying a World Position gets no
//! prompt-turn floor at all — its lean entry, Refocus and `aikit whoami` carry
//! orientation, so no historical NOW strap rides its turns.
//!
//! A session keeps its own record (O-I #1016, [`session_boundary`]): its start
//! and end are recorded in AIKit's lifecycle. Where its caller can close a NOW,
//! the start also opens one for it in Central, its end closes that NOW, and a
//! resume re-enters it; the ground then names the session's own NOW and scratch
//! place, and no other session's NOW items. The hook cannot close one yet, so
//! it opens none, and its ground says so in the NOW's place.

use std::path::{Path, PathBuf};

use aikit_adapters::central_temporal::{
    close_session_now, open_session_now, project_member, read_central_temporal_ground,
    read_control_unpushed, read_day_pointer, render_root_ground, reopen_session_now, session_ref,
    SessionNow,
};
use aikit_adapters::connection_process::SessionLifecycleHandoff;
use aikit_adapters::runner::CommandRunner;
use aikit_core::hooks::{HookDecision, HookEvent, HookEventKind};
use aikit_store::AikitHome;
use serde::{Deserialize, Serialize};

use crate::session_lifecycle_ops::{record_harness_end, record_harness_start, HarnessStart};

pub fn event_needs_reground(kind: &HookEventKind) -> bool {
    matches!(
        kind,
        HookEventKind::SessionStart | HookEventKind::UserPromptSubmit
    )
}

/// Add current Central orientation to an already-computed hook decision.
///
/// This is fail-soft because a non-Central Project remains a valid AIKit world and
/// a temporarily unavailable `ctrl` binary must not counterfeit a hook denial.
/// When a Project is actually bound to Central and the owner read fails, the
/// warning stays visible in the decision.
///
/// A session standing in the Central world without being a Project — the root
/// itself, `Control` — has no NOW to report, but it is owed one thing: whether
/// Control's commits have reached their remote. `process_central_root` reads
/// such a session as no root, so the root is resolved here.
pub fn reground<R: CommandRunner>(
    decision: &mut HookDecision,
    event: &HookEvent,
    project_root: Option<&Path>,
    central_root: Option<&Path>,
    runner: &R,
    ledger: Option<&FloorLedger>,
) {
    if !event_needs_reground(&event.kind) {
        return;
    }
    let Some(project_root) = project_root else {
        return;
    };
    let Some(central_root) = central_root
        .map(Path::to_path_buf)
        .or_else(|| central_root_enclosing(Some(project_root)))
    else {
        return;
    };

    let session = crate::refocus::hook_session(&event.payload);
    let session_now = ledger
        .zip(session.as_deref())
        .and_then(|(ledger, session)| ledger.session_now(session))
        .filter(|held| !held.closed)
        .map(|held| held.now);
    // Its start opened no NOW because it could not close one.
    let not_opened = session_now.is_none()
        && ledger
            .zip(session.as_deref())
            .is_some_and(|(ledger, session)| ledger.now_not_opened(session));
    let rendered = match read_central_temporal_ground(runner, &central_root, project_root) {
        Ok(Some(mut ground)) => {
            ground.session_now = session_now;
            ground.session_now_not_opened = not_opened;
            ground.render()
        }
        Ok(None) => {
            // The Day is read only for a root session that holds a NOW, or
            // whose start opened none because it could not close one: the
            // Control warning alone costs no Central Action.
            let day = (session_now.is_some() || not_opened)
                .then(|| read_day_pointer(runner, &central_root))
                .flatten();
            match render_root_ground(
                read_control_unpushed(runner, &central_root).as_ref(),
                session_now.as_ref(),
                not_opened,
                day.as_ref(),
            ) {
                Some(rendered) => rendered,
                None => return,
            }
        }
        Err(error) => {
            decision.warnings.push(format!(
                "Central temporal re-grounding unavailable for this turn: {}",
                error.message()
            ));
            // A failed NOW read says nothing about Control's remote.
            match read_control_unpushed(runner, &central_root) {
                Some(unpushed) => unpushed.render_standalone(),
                None => return,
            }
        }
    };
    if let (Some(ledger), Some(session)) = (ledger, session) {
        let digest = blake3::hash(rendered.as_bytes()).to_hex().to_string();
        if event.kind == HookEventKind::UserPromptSubmit
            && ledger.last(&session).as_deref() == Some(digest.as_str())
        {
            return;
        }
        ledger.record(&session, &digest);
    }
    decision.injected.insert(0, rendered)
}

/// The lifecycle floor. With a lean World-inhabitation entry (a Position
/// occupancy resolved for this body) the entry replaces the historical NOW
/// ground at SessionStart and names where to read it on demand; an occupied
/// body's prompt turns carry no floor (Refocus owns their re-orientation).
/// Every other body re-grounds as before, prompt turns only on change.
///
/// Pre-compaction is not a delivery boundary: no harness adds that event's
/// output to the model context. What compaction does is discard the ground the
/// session already holds, so the session's floor record is cleared there and
/// the first prompt after compaction re-grounds instead of being skipped as
/// "unchanged".
#[allow(clippy::too_many_arguments)]
pub fn session_floor<R: CommandRunner>(
    decision: &mut HookDecision,
    event: &HookEvent,
    project_root: Option<&Path>,
    central_root: Option<&Path>,
    runner: &R,
    lean_entry: Option<&str>,
    occupied: bool,
    ledger: Option<&FloorLedger>,
) {
    if event.kind == HookEventKind::PreCompact {
        if let (Some(ledger), Some(session)) =
            (ledger, crate::refocus::hook_session(&event.payload))
        {
            ledger.forget(&session);
        }
        return;
    }
    match lean_entry {
        Some(entry) if event.kind == HookEventKind::SessionStart => {
            decision.injected.insert(0, entry.to_owned())
        }
        _ if occupied && event.kind == HookEventKind::UserPromptSubmit => {}
        _ => reground(decision, event, project_root, central_root, runner, ledger),
    }
}

/// Per-session record of the last Central ground a turn carried. A prompt turn
/// whose ground is byte-identical to what this session already received is
/// skipped. The record is advisory orientation state, not delivery evidence.
#[derive(Debug, Clone)]
pub struct FloorLedger {
    dir: PathBuf,
}

impl FloorLedger {
    pub fn in_state(state: &Path) -> Self {
        Self {
            dir: state.join("temporal-floor"),
        }
    }

    fn path(&self, session: &str) -> PathBuf {
        self.dir.join(format!(
            "{}.digest",
            blake3::hash(session.as_bytes()).to_hex()
        ))
    }

    pub fn last(&self, session: &str) -> Option<String> {
        std::fs::read_to_string(self.path(session))
            .ok()
            .map(|digest| digest.trim().to_owned())
    }

    pub fn record(&self, session: &str, digest: &str) {
        if std::fs::create_dir_all(&self.dir).is_ok() {
            let _ = std::fs::write(self.path(session), digest);
        }
    }

    /// Drop what this session is recorded as holding (its context was
    /// compacted away), so the next prompt turn carries the ground again.
    pub fn forget(&self, session: &str) {
        let _ = std::fs::remove_file(self.path(session));
    }

    fn now_path(&self, session: &str) -> PathBuf {
        self.dir.join(format!(
            "{}.now.json",
            blake3::hash(session.as_bytes()).to_hex()
        ))
    }

    /// The NOW Central opened for this session, and where. Kept past the
    /// session's end, marked closed, so a resume re-enters that NOW: it
    /// changes only at a start or an end.
    pub fn session_now(&self, session: &str) -> Option<HeldNow> {
        let text = std::fs::read_to_string(self.now_path(session)).ok()?;
        serde_json::from_str(&text).ok()
    }

    fn not_opened_path(&self, session: &str) -> PathBuf {
        self.dir.join(format!(
            "{}.now-not-opened",
            blake3::hash(session.as_bytes()).to_hex()
        ))
    }

    /// The session's last start opened no NOW because its caller could not
    /// close one ([`session_boundary`]'s `can_close`): its ground says so in
    /// the NOW's place. It changes only at a start.
    pub fn now_not_opened(&self, session: &str) -> bool {
        self.not_opened_path(session).is_file()
    }

    fn mark_now_not_opened(&self, session: &str, not_opened: bool) {
        let path = self.not_opened_path(session);
        if !not_opened {
            let _ = std::fs::remove_file(path);
        } else if std::fs::create_dir_all(&self.dir).is_ok() {
            let _ = std::fs::write(path, "");
        }
    }

    fn hold_session_now(&self, session: &str, held: &HeldNow) {
        if let Ok(text) = serde_json::to_string(held) {
            if std::fs::create_dir_all(&self.dir).is_ok() {
                let _ = std::fs::write(self.now_path(session), text);
            }
        }
    }

    fn claim_path(&self, handed: &str) -> PathBuf {
        self.dir.join(format!(
            "{}.claim.json",
            blake3::hash(handed.as_bytes()).to_hex()
        ))
    }

    fn handed_claim(&self, handed: &str) -> Option<HandedClaim> {
        let text = std::fs::read_to_string(self.claim_path(handed)).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// Keep `claim` on the record `handed`, written whole and then moved into place, so a hook
    /// never reads half of one. A claim that cannot be kept leaves the one before.
    fn keep_handed_claim(&self, handed: &str, claim: &HandedClaim) {
        let (Ok(text), Ok(())) = (
            serde_json::to_string(claim),
            std::fs::create_dir_all(&self.dir),
        ) else {
            return;
        };
        let path = self.claim_path(handed);
        let partial = path.with_extension(format!("{}.partial", std::process::id()));
        if std::fs::write(&partial, text).is_ok() && std::fs::rename(&partial, &path).is_err() {
            let _ = std::fs::remove_file(&partial);
        }
    }

    /// The owner's open `open` launches a body handed the record `handed` (ai-kit#640): from now
    /// on the record is claimed by the first session to start under that open
    /// ([`holds_handed`]), and held by no session of an earlier open.
    pub fn open_handed(&self, handed: &str, open: &str) {
        self.keep_handed_claim(
            handed,
            &HandedClaim {
                open: open.to_owned(),
                session: None,
            },
        );
    }

    /// Claim the record `handed` for `session`, started under the open `open`, unless another
    /// session holds it under that open. A session of an open the record has moved on from claims
    /// nothing. Answers whether `session` holds it.
    fn claim_handed(&self, handed: &str, open: &str, session: &str) -> bool {
        match self.handed_claim(handed) {
            Some(claim) if claim.open != open => false,
            Some(HandedClaim {
                session: Some(claimant),
                ..
            }) => claimant == session,
            // The open's first session; or no open was kept, which leaves the record to the
            // session that starts on it, as before claims were kept.
            _ => {
                self.keep_handed_claim(
                    handed,
                    &HandedClaim {
                        open: open.to_owned(),
                        session: Some(session.to_owned()),
                    },
                );
                true
            }
        }
    }

    /// Whether `session`, started under the open `open`, holds the record `handed`.
    fn holds_handed_claim(&self, handed: &str, open: &str, session: &str) -> bool {
        self.handed_claim(handed)
            .is_some_and(|claim| claim.open == open && claim.session.as_deref() == Some(session))
    }
}

/// The open a handed record was last launched under, and the session that claimed it under that
/// open, if one has started yet ([`holds_handed`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct HandedClaim {
    open: String,
    session: Option<String>,
}

/// Whether the harness session `session` holds the record it was `handed` (ai-kit#640). The
/// record is the body's: the owner's open names itself on the record as it launches the body
/// ([`FloorLedger::open_handed`]), and the first session to start under that open claims it. Any
/// other session handed the same open is a harness the body launched in turn, which inherited its
/// environment, and a session of an earlier open is a body the record has moved on from: each
/// keeps its own record, so its end never ends the body's. With no ledger to keep the claim in, no
/// session holds the record.
fn holds_handed(
    ledger: Option<&FloorLedger>,
    handed: &SessionLifecycleHandoff,
    session: &str,
    kind: &HookEventKind,
) -> bool {
    let Some(ledger) = ledger else {
        return false;
    };
    if *kind == HookEventKind::SessionStart {
        ledger.claim_handed(&handed.session_ref, &handed.open, session)
    } else {
        ledger.holds_handed_claim(&handed.session_ref, &handed.open, session)
    }
}

/// Where a session stands in a Central world: the root, and the Project it is
/// in (`None` in the root register — the root itself, `Control`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionWorld {
    pub central_root: PathBuf,
    #[serde(default)]
    pub project: Option<String>,
}

impl SessionWorld {
    /// The Central world `cwd` stands in, resolved as the floor resolves it.
    pub fn of(cwd: Option<&Path>) -> Option<Self> {
        let central_root = process_central_root(cwd).or_else(|| central_root_enclosing(cwd))?;
        let project = cwd.and_then(|cwd| project_member(&central_root, cwd));
        Some(Self {
            central_root,
            project,
        })
    }
}

/// A session's NOW as its start opened it, and the world it was opened in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeldNow {
    pub world: SessionWorld,
    pub now: SessionNow,
    /// The session's end closed this NOW in Central; a resume re-enters it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub closed: bool,
}

/// A session's own start and end (O-I #1016). SessionStart records the start
/// in AIKit's lifecycle and opens the session's NOW in Central; SessionEnd
/// records the end and closes that NOW. Every failure — no home, `ctrl`
/// unavailable, Central's refusal — is a warning on the decision, never a
/// verdict change, and nothing is retried.
///
/// One session holds one NOW. A start replayed for a live session (resume,
/// clear, compact) records no second start and opens nothing: the session
/// keeps the NOW it holds, wherever its start opened it. A session resumed
/// after its end (`claude --resume`, `/resume`, `codex resume` reuse the id)
/// starts again in the lifecycle and re-enters the NOW its end closed, where
/// it was opened. `world` is `None` outside any Central world: the lifecycle
/// is still recorded, and there is no NOW.
///
/// A session opens or re-enters a NOW only when its caller `can_close` one:
/// Central closes a NOW only for a caller it authenticates, while allocating
/// is open to anyone, so a session that cannot close would leave its NOW open
/// for good. Such a session still records its start and end, asks Central
/// for nothing, and raises no warning; its ground says in one line that its
/// NOW was not opened. The hook passes `false`: what authority a hook may hold
/// is O-I #1016's to decide.
///
/// A body whose owner opened its session (an AIKit conversation) is `handed`
/// that session's record and the open that launched it
/// ([`process_session_lifecycle`]), never a second record under the harness's
/// own id (ai-kit#640). Its start is the owner's to record, once that open has
/// answered, so a body whose open fails leaves the record as it stood; its end
/// is recorded there, and after the owner's End is already recorded. A harness
/// the body launches in turn inherits what it was handed and keeps its own
/// record ([`holds_handed`]). See [`handed_session_ref`].
#[allow(clippy::too_many_arguments)]
pub fn session_boundary<R: CommandRunner>(
    decision: &mut HookDecision,
    event: &HookEvent,
    world: Option<&SessionWorld>,
    home: Option<&AikitHome>,
    ledger: Option<&FloorLedger>,
    runner: &R,
    can_close: bool,
    handed: Option<&SessionLifecycleHandoff>,
) {
    if !matches!(
        event.kind,
        HookEventKind::SessionStart | HookEventKind::SessionEnd
    ) {
        return;
    }
    let Some(session) = crate::refocus::hook_session(&event.payload) else {
        return;
    };
    let handed = handed.filter(|handed| {
        handed_session_ref(Some(handed), &event.client).is_some()
            && holds_handed(ledger, handed, &session, &event.kind)
    });
    let reference = handed.map_or_else(
        || session_ref(&event.client, &session),
        |handed| handed.session_ref.clone(),
    );
    let held = ledger.and_then(|ledger| ledger.session_now(&session));
    if event.kind == HookEventKind::SessionStart {
        // A start on a handed record is the owner's to record, once its open has answered.
        let recorder = home.filter(|_| handed.is_none());
        let start = match recorder.map(|home| record_harness_start(home, &reference)) {
            Some(Ok(start)) => Some(start),
            Some(Err(error)) => {
                decision.warnings.push(format!(
                    "session lifecycle start not recorded for {reference}: {}",
                    error.message()
                ));
                None
            }
            None => None,
        };
        let Some(world) = world else {
            return;
        };
        if let Some(ledger) = ledger {
            ledger.mark_now_not_opened(&session, !can_close);
        }
        if !can_close {
            return;
        }
        let (world, opened) = match &held {
            Some(held) if !held.closed => return,
            Some(held) => (
                &held.world,
                reopen_session_now(
                    runner,
                    &held.world.central_root,
                    held.world.project.as_deref(),
                    &event.client,
                    &reference,
                ),
            ),
            // Only a first start is known to have no NOW yet: any other may
            // find one, closed, where it stands.
            None if start == Some(HarnessStart::First) => (
                world,
                open_session_now(
                    runner,
                    &world.central_root,
                    world.project.as_deref(),
                    &event.client,
                    &reference,
                ),
            ),
            None => (
                world,
                reopen_session_now(
                    runner,
                    &world.central_root,
                    world.project.as_deref(),
                    &event.client,
                    &reference,
                ),
            ),
        };
        match opened {
            Ok(now) => {
                if let Some(ledger) = ledger {
                    ledger.hold_session_now(
                        &session,
                        &HeldNow {
                            world: world.clone(),
                            now,
                            closed: false,
                        },
                    );
                }
            }
            Err(error) => decision.warnings.push(format!(
                "this session's NOW was not opened in Central: {}",
                error.message()
            )),
        }
        return;
    }
    if let Some(Err(error)) = home.map(|home| record_harness_end(home, &reference)) {
        decision.warnings.push(format!(
            "session lifecycle end not recorded for {reference}: {}",
            error.message()
        ));
    }
    if held.as_ref().is_some_and(|held| held.closed) {
        return;
    }
    // A session that cannot close a NOW opened nothing to look for. A NOW it
    // holds from a start that could is still closed, or warned of.
    if held.is_none() && !can_close {
        return;
    }
    // Closed where it was opened; a session whose start predates this record
    // is looked for where it ends.
    let Some(world) = held.as_ref().map(|held| &held.world).or(world) else {
        return;
    };
    match close_session_now(
        runner,
        &world.central_root,
        world.project.as_deref(),
        &reference,
    ) {
        // Kept, closed, so a resume re-enters it where it was opened. A
        // refused close leaves the NOW active and held: a resume keeps it.
        Ok(closed) => {
            let now = match (&held, closed) {
                (Some(held), _) => Some(held.now.clone()),
                (None, Some(now_ref)) => Some(SessionNow {
                    now_ref,
                    scratch: None,
                }),
                (None, None) => None,
            };
            if let (Some(ledger), Some(now)) = (ledger, now) {
                ledger.hold_session_now(
                    &session,
                    &HeldNow {
                        world: world.clone(),
                        now,
                        closed: true,
                    },
                );
            }
        }
        Err(error) => decision.warnings.push(format!(
            "this session's NOW was not closed in Central: {}",
            error.message()
        )),
    }
}

/// Whether this process is a body stamped into a World Position occupancy
/// (`aikit inhabit` exports `OI_POSITION_REF` into the harness).
pub fn process_is_occupied() -> bool {
    std::env::var_os("OI_POSITION_REF").is_some_and(|value| !value.is_empty())
}

/// What this process's body was handed of its session's record by the owner
/// that opened the session ([`SessionLifecycleHandoff`], ai-kit#640), if
/// anything.
pub fn process_session_lifecycle() -> Option<SessionLifecycleHandoff> {
    SessionLifecycleHandoff::from_process()
}

/// The record a hook of `client` records its session's end on when its body
/// was `handed` one: the handed record, when the hook is the body's own
/// harness (the client its owner handed beside the record) and the record
/// names a session (`aikit:session:<client>:<id>`). The record keeps the
/// spelling of the harness that first ran the session, whichever harness runs
/// it now. A hook of another client was inherited by a harness the body
/// launched in turn, and is not this session's; that session keeps its own
/// record.
pub fn handed_session_ref(
    handed: Option<&SessionLifecycleHandoff>,
    client: &str,
) -> Option<String> {
    let handed = handed.filter(|handed| handed.client == client)?;
    let names_a_session = handed
        .session_ref
        .strip_prefix("aikit:session:")
        .and_then(|rest| rest.split_once(':'))
        .is_some_and(|(spelt, id)| !spelt.is_empty() && !id.is_empty());
    (names_a_session && crate::session_lifecycle_ops::harness_session(&handed.session_ref).is_ok())
        .then(|| handed.session_ref.clone())
}

/// Resolve the Central root without teaching AIKit Central's internal file
/// layout. `CENTRAL_ROOT` is authoritative when present; otherwise the standard
/// Central home is considered only when the current Project is physically inside
/// its `Work` tree.
pub fn process_central_root(project_root: Option<&Path>) -> Option<PathBuf> {
    let project_root = project_root?;
    let root =
        if let Some(root) = std::env::var_os("CENTRAL_ROOT").filter(|value| !value.is_empty()) {
            PathBuf::from(root)
        } else {
            let home = std::env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .or_else(|| std::env::var_os("USERPROFILE").filter(|value| !value.is_empty()))?;
            PathBuf::from(home).join("Central")
        };
    // A Project at any depth beneath `Work`, or a workcell seat that is a
    // linked checkout of one: both stand in this Central world.
    aikit_adapters::central_temporal::project_ground_root(&root, project_root).map(|_| root)
}

/// The Central root enclosing `path`, when `path` is the root itself or
/// anywhere beneath it — the `Control` register and the `Work` tree alike.
///
/// This is a *recognition* rule, not a project claim: it answers "is this path
/// inside a Central world?". The entity disclosure needs exactly that, because
/// the entities it names are materialised from the world root and the root
/// register is where they live. Project-scoped consumers keep
/// [`process_central_root`], which additionally requires the path to be a
/// Project beneath `Work` — so `compose` and temporal re-grounding are
/// unaffected by this widening.
pub fn central_root_enclosing(path: Option<&Path>) -> Option<PathBuf> {
    let path = path?;
    if let Some(root) = std::env::var_os("CENTRAL_ROOT").filter(|value| !value.is_empty()) {
        let root = PathBuf::from(root);
        return path.starts_with(&root).then_some(root);
    }
    let home = std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .or_else(|| std::env::var_os("USERPROFILE").filter(|value| !value.is_empty()))?;
    let root = PathBuf::from(home).join("Central");
    path.starts_with(&root).then_some(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_adapters::runner::ScriptedRunner;
    use serde_json::json;

    fn success(data: serde_json::Value) -> String {
        json!({"ok":true,"status":"success","action":"fixture","data":data}).to_string()
    }

    fn decision(kind: HookEventKind) -> HookDecision {
        HookDecision {
            event: kind,
            allowed: true,
            denial: None,
            payload: json!({}),
            injected: vec![],
            warnings: vec![],
            steps: vec![],
            groups: vec![],
            bypass_consumed: false,
        }
    }

    #[test]
    fn only_causal_orientation_events_read_central() {
        assert!(event_needs_reground(&HookEventKind::SessionStart));
        assert!(event_needs_reground(&HookEventKind::UserPromptSubmit));
        assert!(!event_needs_reground(&HookEventKind::PreCompact));
        assert!(!event_needs_reground(&HookEventKind::PreToolUse));
        assert!(!event_needs_reground(&HookEventKind::PostToolUse));
    }

    fn day() -> String {
        day_of("2026-09-23")
    }

    fn day_of(date: &str) -> String {
        success(
            json!({"day_ref": format!("central:day:control:root:{date}"), "revision": "rev-day"}),
        )
    }

    fn now_with(subject: &str) -> String {
        success(json!({
            "exists": true,
            "active_items": [{"id":"h1","kind":"handoff","actor":"agent","status":"active","subject":subject}],
            "human_scratch": [], "day_records": []
        }))
    }

    #[test]
    fn next_prompt_reads_the_turned_day_and_never_other_sessions_now_items() {
        let (a, b) = (now_with("state A"), now_with("state B"));
        let runner = ScriptedRunner::new()
            .sequence("projectcentral.now.inspect", &[&a, &b])
            .sequence(
                "central.day.read",
                &[&day_of("2026-09-23"), &day_of("2026-09-24")],
            );
        let central = Path::new("/home/me/Central");
        let project = Path::new("/home/me/Central/Work/example");

        let start = HookEvent::new("claude", HookEventKind::SessionStart, json!({}));
        let mut first = decision(HookEventKind::SessionStart);
        reground(
            &mut first,
            &start,
            Some(project),
            Some(central),
            &runner,
            None,
        );
        assert!(first
            .injected_text()
            .contains("central:day:control:root:2026-09-23"));

        let prompt = HookEvent::new("claude", HookEventKind::UserPromptSubmit, json!({}));
        let mut second = decision(HookEventKind::UserPromptSubmit);
        reground(
            &mut second,
            &prompt,
            Some(project),
            Some(central),
            &runner,
            None,
        );
        assert!(second
            .injected_text()
            .contains("central:day:control:root:2026-09-24"));
        assert!(!second.injected_text().contains("2026-09-23"));
        // Another session's handoff is not this session's ground.
        for turn in [&first, &second] {
            assert!(!turn.injected_text().contains("state A"));
            assert!(!turn.injected_text().contains("state B"));
        }
        assert!(!runner
            .call_lines()
            .iter()
            .any(|line| line.contains("projectcentral.flow.")));
    }

    #[test]
    fn a_prompt_turn_carries_the_ground_only_when_it_changed() {
        let temp = tempfile::tempdir().unwrap();
        let ledger = FloorLedger::in_state(temp.path());
        let (a, b) = (day_of("2026-09-23"), day_of("2026-09-24"));
        let runner = ScriptedRunner::new()
            .on("projectcentral.now.inspect", &now_with("state A"))
            .sequence("central.day.read", &[&a, &a, &a, &b]);
        let central = Path::new("/home/me/Central");
        let project = Path::new("/home/me/Central/Work/example");
        let session = json!({"session_id": "sess-1"});

        let start = HookEvent::new("claude", HookEventKind::SessionStart, session.clone());
        let mut first = decision(HookEventKind::SessionStart);
        reground(
            &mut first,
            &start,
            Some(project),
            Some(central),
            &runner,
            Some(&ledger),
        );
        assert!(first.injected_text().contains("2026-09-23"));
        assert!(!first.injected_text().contains("state A"));

        // Same ground: the prompt turn is silent, twice.
        for _ in 0..2 {
            let prompt = HookEvent::new("claude", HookEventKind::UserPromptSubmit, session.clone());
            let mut turn = decision(HookEventKind::UserPromptSubmit);
            reground(
                &mut turn,
                &prompt,
                Some(project),
                Some(central),
                &runner,
                Some(&ledger),
            );
            assert!(turn.injected.is_empty(), "unchanged ground is not re-sent");
        }

        // The Day turned: delivered once.
        let prompt = HookEvent::new("claude", HookEventKind::UserPromptSubmit, session.clone());
        let mut changed = decision(HookEventKind::UserPromptSubmit);
        reground(
            &mut changed,
            &prompt,
            Some(project),
            Some(central),
            &runner,
            Some(&ledger),
        );
        assert!(changed.injected_text().contains("2026-09-24"));

        // Another session starts from nothing.
        let other = HookEvent::new(
            "claude",
            HookEventKind::UserPromptSubmit,
            json!({"session_id": "sess-2"}),
        );
        let runner = ScriptedRunner::new()
            .on("projectcentral.now.inspect", &now_with("state B"))
            .on("central.day.read", &b);
        let mut fresh = decision(HookEventKind::UserPromptSubmit);
        reground(
            &mut fresh,
            &other,
            Some(project),
            Some(central),
            &runner,
            Some(&ledger),
        );
        assert!(fresh.injected_text().contains("2026-09-24"));
    }

    fn handoff_runner() -> ScriptedRunner {
        let now = success(json!({
            "exists": true,
            "active_items": [{"id": "h1", "kind": "handoff", "actor": "agent", "status": "active",
                              "subject": "historical handoff", "result": "a long historical NOW dump"}],
            "human_scratch": [], "day_records": []
        }));
        ScriptedRunner::new()
            .on("projectcentral.now.inspect", &now)
            .on("central.day.read", &day())
    }

    #[test]
    fn a_lean_entry_replaces_the_historical_floor_at_session_start_only() {
        let central = Path::new("/home/me/Central");
        let project = Path::new("/home/me/Central/Work/example");
        let start = HookEvent::new("claude", HookEventKind::SessionStart, json!({}));

        let runner = handoff_runner();
        let mut lean = decision(HookEventKind::SessionStart);
        session_floor(
            &mut lean,
            &start,
            Some(project),
            Some(central),
            &runner,
            Some("[O:I World inhabitation — lean entry]"),
            true,
            None,
        );
        assert_eq!(
            lean.injected,
            vec!["[O:I World inhabitation — lean entry]".to_owned()]
        );
        assert!(!lean.injected_text().contains("historical handoff"));
        assert!(runner.calls().is_empty(), "the dump is not even read");

        // Without an occupancy the floor is byte-identical to the re-ground.
        let runner = handoff_runner();
        let mut unchanged = decision(HookEventKind::SessionStart);
        session_floor(
            &mut unchanged,
            &start,
            Some(project),
            Some(central),
            &runner,
            None,
            false,
            None,
        );
        let mut direct = decision(HookEventKind::SessionStart);
        reground(
            &mut direct,
            &start,
            Some(project),
            Some(central),
            &handoff_runner(),
            None,
        );
        assert_eq!(unchanged.injected, direct.injected);
        assert!(unchanged
            .injected_text()
            .starts_with("[Central temporal ground]"));
        assert!(!unchanged.injected_text().contains("historical handoff"));

        // An occupied body's prompt turn carries no historical floor…
        let prompt = HookEvent::new("claude", HookEventKind::UserPromptSubmit, json!({}));
        let runner = handoff_runner();
        let mut occupied = decision(HookEventKind::UserPromptSubmit);
        session_floor(
            &mut occupied,
            &prompt,
            Some(project),
            Some(central),
            &runner,
            None,
            true,
            None,
        );
        assert!(occupied.injected.is_empty());
        assert!(runner.calls().is_empty(), "the dump is not even read");

        // …while an unoccupied body's prompt turn re-grounds as before, and
        // that ground is its own: no historical NOW dump rides it.
        let mut turn = decision(HookEventKind::UserPromptSubmit);
        session_floor(
            &mut turn,
            &prompt,
            Some(project),
            Some(central),
            &handoff_runner(),
            None,
            false,
            None,
        );
        assert!(turn.injected_text().contains("project: example"));
        assert!(!turn.injected_text().contains("historical handoff"));
        assert!(!turn.injected_text().contains("historical NOW dump"));
    }

    #[test]
    fn central_failure_is_visible_but_never_becomes_hook_denial() {
        let runner =
            ScriptedRunner::new().failing("projectcentral.now.inspect", 9, "owner unavailable");
        let event = HookEvent::new("codex", HookEventKind::UserPromptSubmit, json!({}));
        let mut result = decision(HookEventKind::UserPromptSubmit);
        reground(
            &mut result,
            &event,
            Some(Path::new("/home/me/Central/Work/example")),
            Some(Path::new("/home/me/Central")),
            &runner,
            None,
        );
        assert!(result.allowed);
        assert!(result.injected.is_empty());
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.contains("Central temporal")));
    }

    #[test]
    fn compaction_clears_the_record_so_the_next_prompt_regrounds() {
        let state = tempfile::tempdir().unwrap();
        let ledger = FloorLedger::in_state(state.path());
        let project = Path::new("/home/me/Central/Work/example");
        let central = Path::new("/home/me/Central");
        let payload = json!({"session_id": "s-compact"});
        let prompt = HookEvent::new("codex", HookEventKind::UserPromptSubmit, payload.clone());
        let mut first = decision(HookEventKind::UserPromptSubmit);
        session_floor(
            &mut first,
            &prompt,
            Some(project),
            Some(central),
            &handoff_runner(),
            None,
            false,
            Some(&ledger),
        );
        assert!(
            !first.injected.is_empty(),
            "first prompt carries the ground"
        );
        let mut repeat = decision(HookEventKind::UserPromptSubmit);
        session_floor(
            &mut repeat,
            &prompt,
            Some(project),
            Some(central),
            &handoff_runner(),
            None,
            false,
            Some(&ledger),
        );
        assert!(
            repeat.injected.is_empty(),
            "unchanged ground is not re-sent"
        );
        let compact = HookEvent::new("codex", HookEventKind::PreCompact, payload.clone());
        let mut before = decision(HookEventKind::PreCompact);
        session_floor(
            &mut before,
            &compact,
            Some(project),
            Some(central),
            &handoff_runner(),
            None,
            false,
            Some(&ledger),
        );
        assert!(
            before.injected.is_empty(),
            "pre-compaction delivers nothing"
        );
        let mut after = decision(HookEventKind::UserPromptSubmit);
        session_floor(
            &mut after,
            &prompt,
            Some(project),
            Some(central),
            &handoff_runner(),
            None,
            false,
            Some(&ledger),
        );
        assert!(
            !after.injected.is_empty(),
            "the first prompt after compaction re-grounds the compacted session"
        );
    }

    fn control_ahead() -> ScriptedRunner {
        ScriptedRunner::new()
            .on("rev-list --count origin/main..HEAD", "6\n")
            .on("--skip=", "2026-10-08 14194ff\n")
    }

    #[test]
    fn a_session_at_the_central_root_still_hears_that_control_is_unpushed() {
        // The root is no Project, so there is no NOW to report — but the
        // owner's ground being unpushed is exactly what such a session must hear.
        let central = Path::new("/home/me/Central");
        let runner = control_ahead();
        let start = HookEvent::new("claude", HookEventKind::SessionStart, json!({}));
        let mut result = decision(HookEventKind::SessionStart);
        reground(
            &mut result,
            &start,
            Some(central),
            Some(central),
            &runner,
            None,
        );
        let text = result.injected_text();
        assert!(text.starts_with("[Central temporal ground]"));
        assert!(text.contains("6 commits ahead of origin/main, unpushed since 2026-10-08"));
        assert!(text.contains("does not grant authority"));
        assert!(!text.contains("project:"), "the root is no Project");
        assert!(result.warnings.is_empty());
        assert!(
            runner
                .call_lines()
                .iter()
                .all(|line| line.starts_with("git -C /home/me/Central/Control ")),
            "no Central Action is invoked for a non-Project"
        );
    }

    #[test]
    fn control_itself_is_no_project_but_hears_it_too() {
        let central = Path::new("/home/me/Central");
        let start = HookEvent::new("codex", HookEventKind::SessionStart, json!({}));
        let mut result = decision(HookEventKind::SessionStart);
        reground(
            &mut result,
            &start,
            Some(Path::new("/home/me/Central/Control")),
            Some(central),
            &control_ahead(),
            None,
        );
        assert!(result.injected_text().contains("6 commits ahead"));
    }

    #[test]
    fn a_root_session_in_step_with_its_remote_gets_nothing_at_all() {
        let central = Path::new("/home/me/Central");
        let runner = ScriptedRunner::new().on("rev-list --count origin/main..HEAD", "0\n");
        let start = HookEvent::new("claude", HookEventKind::SessionStart, json!({}));
        let mut result = decision(HookEventKind::SessionStart);
        reground(
            &mut result,
            &start,
            Some(central),
            Some(central),
            &runner,
            None,
        );
        assert!(result.injected.is_empty());
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn an_unreadable_control_never_costs_a_turn_or_a_warning() {
        // Nothing scripted: git answers with an error, which reads as silence.
        let central = Path::new("/home/me/Central");
        let prompt = HookEvent::new("claude", HookEventKind::UserPromptSubmit, json!({}));
        let mut result = decision(HookEventKind::UserPromptSubmit);
        reground(
            &mut result,
            &prompt,
            Some(central),
            Some(central),
            &ScriptedRunner::new(),
            None,
        );
        assert!(result.allowed);
        assert!(result.injected.is_empty());
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn the_root_warning_is_said_once_per_session_until_it_changes() {
        let state = tempfile::tempdir().unwrap();
        let ledger = FloorLedger::in_state(state.path());
        let central = Path::new("/home/me/Central");
        let session = json!({"session_id": "sess-root"});

        let start = HookEvent::new("claude", HookEventKind::SessionStart, session.clone());
        let mut first = decision(HookEventKind::SessionStart);
        reground(
            &mut first,
            &start,
            Some(central),
            Some(central),
            &control_ahead(),
            Some(&ledger),
        );
        assert!(first.injected_text().contains("6 commits ahead"));

        let prompt = HookEvent::new("claude", HookEventKind::UserPromptSubmit, session.clone());
        let mut same = decision(HookEventKind::UserPromptSubmit);
        reground(
            &mut same,
            &prompt,
            Some(central),
            Some(central),
            &control_ahead(),
            Some(&ledger),
        );
        assert!(
            same.injected.is_empty(),
            "an unchanged warning is not re-sent"
        );

        // Another commit lands: the warning changed, so it is said again.
        let more = ScriptedRunner::new()
            .on("rev-list --count origin/main..HEAD", "7\n")
            .on("--skip=", "2026-10-08 14194ff\n");
        let mut changed = decision(HookEventKind::UserPromptSubmit);
        reground(
            &mut changed,
            &prompt,
            Some(central),
            Some(central),
            &more,
            Some(&ledger),
        );
        assert!(changed.injected_text().contains("7 commits ahead"));
    }

    #[test]
    fn a_session_outside_any_central_world_is_untouched() {
        let runner = control_ahead();
        let start = HookEvent::new("claude", HookEventKind::SessionStart, json!({}));
        let mut result = decision(HookEventKind::SessionStart);
        reground(
            &mut result,
            &start,
            Some(Path::new("/nonexistent-elsewhere/project")),
            None,
            &runner,
            None,
        );
        assert!(result.injected.is_empty());
        assert!(runner.calls().is_empty());
    }

    #[test]
    fn a_failed_now_read_does_not_hide_that_control_is_unpushed() {
        // The NOW read errors; the owner's ground being unpushed is a separate
        // fact and still reaches the session beside the "unavailable" notice.
        let central = Path::new("/home/me/Central");
        let project = Path::new("/home/me/Central/Work/example");
        let runner = control_ahead().failing("projectcentral.now.inspect", 9, "owner unavailable");
        let start = HookEvent::new("claude", HookEventKind::SessionStart, json!({}));
        let mut result = decision(HookEventKind::SessionStart);
        reground(
            &mut result,
            &start,
            Some(project),
            Some(central),
            &runner,
            None,
        );
        assert!(result.allowed);
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.contains("Central temporal")));
        assert!(result.injected_text().contains("6 commits ahead"));
    }

    #[test]
    fn a_project_session_carries_the_warning_inside_its_ground() {
        let central = Path::new("/home/me/Central");
        let project = Path::new("/home/me/Central/Work/example");
        let runner = control_ahead()
            .on("projectcentral.now.inspect", &now_with("state A"))
            .on("central.day.read", &day());
        let start = HookEvent::new("claude", HookEventKind::SessionStart, json!({}));
        let mut result = decision(HookEventKind::SessionStart);
        reground(
            &mut result,
            &start,
            Some(project),
            Some(central),
            &runner,
            None,
        );
        let text = result.injected_text();
        assert!(text.contains("project: example"));
        assert!(!text.contains("state A"), "no other session's handoff");
        assert!(text.contains("6 commits ahead of origin/main"));
        assert_eq!(
            text.matches("[Central temporal ground]").count(),
            1,
            "one block, not a block and a second warning"
        );
    }

    // ---- The session's own record (O-I #1016) ------------------------------

    use crate::session_lifecycle_ops::{harness_session, SessionLifecycleServiceOps};
    use aikit_core::session_lifecycle::{SessionLifecycleEventKind, SessionLifecycleState};

    const REF: &str = "aikit:session:claude:sess-1";
    const NOW_REF: &str = "central:now:project:example:k1";
    const SCRATCH: &str = "/home/me/Central/Work/example/ProjectCentral/agents/now/clearings/k1/T";
    const CENTRAL: &str = "/home/me/Central";
    const PROJECT: &str = "/home/me/Central/Work/example";

    fn example_world() -> SessionWorld {
        SessionWorld {
            central_root: CENTRAL.into(),
            project: Some("example".into()),
        }
    }

    fn listed(lifecycle: &str) -> String {
        success(
            json!({"records": [{"now_ref": NOW_REF, "task_ref": REF, "lifecycle": lifecycle,
            "participant_refs": [REF], "revision": {"byte_len": 9, "revision": "rev-now"}}]}),
        )
    }

    /// Central as the session's start, turns and end meet it. The Project's
    /// NOW holds another session's handoff, which must never ride this one.
    fn owner_runner() -> ScriptedRunner {
        owner_runner_listing(&[&listed("active")])
    }

    /// [`owner_runner`], with `central.now.list` answering `listings` in turn.
    fn owner_runner_listing(listings: &[&str]) -> ScriptedRunner {
        ScriptedRunner::new()
            .on(
                "central.work.policy",
                &success(json!({"revision": "rev-policy"})),
            )
            .on(
                "central.now.allocate",
                &success(
                    json!({"created": true, "now_ref": NOW_REF, "writable_destination": SCRATCH}),
                ),
            )
            .sequence("central.now.list", listings)
            .on(
                "central.now.lifecycle",
                &success(json!({"lifecycle": "closed"})),
            )
            .on(
                "projectcentral.now.inspect",
                &now_with("another session's handoff"),
            )
            .on("central.day.read", &day())
    }

    fn calls_of(runner: &ScriptedRunner, action: &str) -> Vec<String> {
        runner
            .call_lines()
            .into_iter()
            .filter(|line| line.contains(action))
            .collect()
    }

    fn hook_at(cwd: &str, kind: HookEventKind, payload: serde_json::Value) -> HookEvent {
        HookEvent::new("claude", kind, payload).in_cwd(cwd)
    }

    /// One dispatch's floor, in the order `hook::dispatch_with_entry` runs it,
    /// by a session whose caller can close a NOW.
    fn dispatch(
        event: &HookEvent,
        world: Option<&SessionWorld>,
        home: &AikitHome,
        ledger: &FloorLedger,
        runner: &ScriptedRunner,
    ) -> HookDecision {
        dispatch_holding(event, world, home, ledger, runner, true)
    }

    /// [`dispatch`], by a session whose caller can close a NOW (`can_close`)
    /// or not.
    fn dispatch_holding(
        event: &HookEvent,
        world: Option<&SessionWorld>,
        home: &AikitHome,
        ledger: &FloorLedger,
        runner: &ScriptedRunner,
        can_close: bool,
    ) -> HookDecision {
        let mut result = decision(event.kind.clone());
        session_boundary(
            &mut result,
            event,
            world,
            Some(home),
            Some(ledger),
            runner,
            can_close,
            None,
        );
        session_floor(
            &mut result,
            event,
            event.cwd.as_deref(),
            world.map(|world| world.central_root.as_path()),
            runner,
            None,
            false,
            Some(ledger),
        );
        result
    }

    #[test]
    fn a_session_start_records_its_lifecycle_and_opens_its_own_now_once() {
        let temp = tempfile::tempdir().unwrap();
        let home = AikitHome::at(temp.path());
        let ledger = FloorLedger::in_state(&home.state());
        let runner = owner_runner();
        let world = example_world();
        let start = hook_at(
            PROJECT,
            HookEventKind::SessionStart,
            json!({"session_id": "sess-1", "source": "startup"}),
        );
        let first = dispatch(&start, Some(&world), &home, &ledger, &runner);
        assert!(first.allowed);
        assert!(first.warnings.is_empty(), "{:?}", first.warnings);

        // AIKit's lifecycle holds the start, by the session's ref.
        let session = harness_session(REF).unwrap();
        assert_eq!(
            home.session_lifecycle_list().unwrap(),
            vec![session.clone()]
        );
        let history = home.session_lifecycle_history(&session).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].kind, SessionLifecycleEventKind::SessionStarted);
        assert_eq!(history[0].origin, REF);

        // Central allocated one NOW for it, and the start names that NOW,
        // its scratch place and the Day — no other session's handoff.
        let allocations = calls_of(&runner, "central.now.allocate");
        assert_eq!(allocations.len(), 1);
        assert!(allocations[0].contains("\"task_ref\":\"aikit:session:claude:sess-1\""));
        let text = first.injected_text();
        assert!(text.contains(&format!("NOW: {NOW_REF} (this session's own)")));
        assert!(text.contains(&format!("scratch: {SCRATCH}")));
        assert!(text.contains("Day: central:day:control:root:2026-09-23"));
        assert!(!text.contains("another session's handoff"), "{text}");

        // A resumed start of the live session: no second start, no second
        // allocation, and its ground names the same NOW.
        let resume = hook_at(
            PROJECT,
            HookEventKind::SessionStart,
            json!({"session_id": "sess-1", "source": "resume"}),
        );
        let again = dispatch(&resume, Some(&world), &home, &ledger, &runner);
        assert!(again.warnings.is_empty(), "{:?}", again.warnings);
        assert_eq!(home.session_lifecycle_history(&session).unwrap().len(), 1);
        assert_eq!(calls_of(&runner, "central.now.allocate").len(), 1);
        assert!(again.injected_text().contains(NOW_REF));

        // The prompt turn after it is quiet: nothing changed.
        let prompt = hook_at(
            PROJECT,
            HookEventKind::UserPromptSubmit,
            json!({"session_id": "sess-1"}),
        );
        let turn = dispatch(&prompt, Some(&world), &home, &ledger, &runner);
        assert!(turn.injected.is_empty(), "{}", turn.injected_text());
        assert_eq!(calls_of(&runner, "central.now.allocate").len(), 1);
    }

    #[test]
    fn a_session_that_cannot_close_a_now_records_its_lifecycle_and_opens_none() {
        let temp = tempfile::tempdir().unwrap();
        let home = AikitHome::at(temp.path());
        let ledger = FloorLedger::in_state(&home.state());
        let runner = owner_runner();
        let world = example_world();
        let start = hook_at(
            PROJECT,
            HookEventKind::SessionStart,
            json!({"session_id": "sess-1", "source": "startup"}),
        );
        let started = dispatch_holding(&start, Some(&world), &home, &ledger, &runner, false);
        assert!(started.allowed);
        assert!(started.warnings.is_empty(), "{:?}", started.warnings);

        // The start is recorded, and Central is asked for no NOW: this
        // session could not close one, so it would stay open for good.
        let session = harness_session(REF).unwrap();
        assert_eq!(home.session_lifecycle_history(&session).unwrap().len(), 1);
        for action in ["central.now.allocate", "central.now.lifecycle"] {
            assert!(calls_of(&runner, action).is_empty(), "{action} was asked");
        }
        assert!(ledger.session_now("sess-1").is_none());
        let text = started.injected_text();
        assert!(!text.contains("(this session's own)"), "{text}");
        assert!(
            text.contains("Day: central:day:control:root:2026-09-23"),
            "{text}"
        );
        // One plain line says so, in the NOW's place, and no other
        // session's NOW items ride it.
        assert_eq!(text.matches(SESSION_NOW_NOT_OPENED).count(), 1, "{text}");
        assert!(!text.contains("another session's handoff"), "{text}");

        // The prompt turn after it is quiet: nothing changed.
        let prompt = hook_at(
            PROJECT,
            HookEventKind::UserPromptSubmit,
            json!({"session_id": "sess-1"}),
        );
        let turn = dispatch_holding(&prompt, Some(&world), &home, &ledger, &runner, false);
        assert!(turn.injected.is_empty(), "{}", turn.injected_text());

        // Its end is recorded, and nothing of its own is looked for or closed.
        let end = hook_at(
            PROJECT,
            HookEventKind::SessionEnd,
            json!({"session_id": "sess-1", "reason": "exit"}),
        );
        let ended = dispatch_holding(&end, Some(&world), &home, &ledger, &runner, false);
        assert!(ended.warnings.is_empty(), "{:?}", ended.warnings);
        assert_eq!(
            home.session_lifecycle_read_model(&session).unwrap().state,
            SessionLifecycleState::Ended
        );
        for action in [
            "central.now.allocate",
            "central.now.list",
            "central.now.lifecycle",
        ] {
            assert!(calls_of(&runner, action).is_empty(), "{action} was asked");
        }
    }

    #[test]
    fn a_session_end_records_the_end_and_closes_its_own_now() {
        let temp = tempfile::tempdir().unwrap();
        let home = AikitHome::at(temp.path());
        let ledger = FloorLedger::in_state(&home.state());
        let runner = owner_runner();
        let world = example_world();
        let payload = json!({"session_id": "sess-1"});
        dispatch(
            &hook_at(PROJECT, HookEventKind::SessionStart, payload.clone()),
            Some(&world),
            &home,
            &ledger,
            &runner,
        );
        assert!(ledger.session_now("sess-1").is_some());

        let end = hook_at(
            PROJECT,
            HookEventKind::SessionEnd,
            json!({"session_id": "sess-1", "reason": "exit"}),
        );
        let ended = dispatch(&end, Some(&world), &home, &ledger, &runner);
        assert!(ended.allowed);
        assert!(ended.warnings.is_empty(), "{:?}", ended.warnings);
        assert!(ended.injected.is_empty(), "an end carries no ground");

        let model = home
            .session_lifecycle_read_model(&harness_session(REF).unwrap())
            .unwrap();
        assert_eq!(model.state, SessionLifecycleState::Ended);
        assert_eq!(model.event_count, 2);

        // Found among the NOWs naming this session, closed at the revision
        // just listed.
        let listing = calls_of(&runner, "central.now.list");
        assert_eq!(listing.len(), 1);
        assert!(listing[0].contains("\"participant_refs\":[\"aikit:session:claude:sess-1\"]"));
        let closes = calls_of(&runner, "central.now.lifecycle");
        assert_eq!(closes.len(), 1);
        for part in [
            "\"lifecycle\":\"closed\"",
            "\"expected_revision\":\"rev-now\"",
            "\"expected_policy_revision\":\"rev-policy\"",
            "\"project\":\"example\"",
            NOW_REF,
        ] {
            assert!(closes[0].contains(part), "{part} missing: {}", closes[0]);
        }
        // Kept, closed, for a resume to re-enter; no turn names it now.
        assert!(ledger.session_now("sess-1").is_some_and(|held| held.closed));

        // An end replayed for an ended session records nothing more.
        let replay = dispatch(&end, Some(&world), &home, &ledger, &runner);
        assert!(replay.warnings.is_empty(), "{:?}", replay.warnings);
        assert_eq!(
            home.session_lifecycle_history(&harness_session(REF).unwrap())
                .unwrap()
                .len(),
            2
        );
    }

    /// The hooks of one AIKit home, each run as `aikit hook dispatch claude <event>` runs the
    /// session boundary in a process handed `handed`, and the record kinds they leave.
    struct HandedHooks {
        _temp: tempfile::TempDir,
        home: AikitHome,
        ledger: FloorLedger,
    }

    impl HandedHooks {
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let home = AikitHome::at(temp.path());
            let ledger = FloorLedger::in_state(&home.state());
            Self {
                _temp: temp,
                home,
                ledger,
            }
        }

        fn kinds(&self, session_ref: &str) -> Vec<SessionLifecycleEventKind> {
            self.home
                .session_lifecycle_history(&harness_session(session_ref).unwrap())
                .map(|history| history.iter().map(|event| event.kind).collect())
                .unwrap_or_default()
        }

        /// One hook of `client` for its native session `session`, in a claude body handed the
        /// record `OWNED` under the open `open`.
        fn hook(&self, client: &str, kind: HookEventKind, session: &str, open: &str) {
            self.hook_handed(
                client,
                kind,
                session,
                &SessionLifecycleHandoff {
                    session_ref: OWNED.into(),
                    client: "claude".into(),
                    open: open.into(),
                },
            );
        }

        /// One hook of `client` for its native session `session`, in a body handed `handed`.
        fn hook_handed(
            &self,
            client: &str,
            kind: HookEventKind,
            session: &str,
            handed: &SessionLifecycleHandoff,
        ) {
            let event = HookEvent::new(client, kind.clone(), json!({"session_id": session}))
                .in_cwd(PROJECT);
            let mut result = decision(kind);
            session_boundary(
                &mut result,
                &event,
                None,
                Some(&self.home),
                Some(&self.ledger),
                &ScriptedRunner::new(),
                false,
                Some(handed),
            );
            assert!(result.allowed);
            assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        }
    }

    /// The record an AIKit conversation's session is kept under, handed to the bodies its opens launch.
    const OWNED: &str = "aikit:session:claude:agent-session/talk";

    fn own(session: &str) -> String {
        format!("aikit:session:claude:{session}")
    }

    /// ai-kit#640 (R640-2): a body its owner handed the session's record keeps no record under the
    /// harness's own id. Its start is the owner's to record, once its open has answered; its end
    /// lands on the record, and after the owner's End it is already recorded, answered with no
    /// warning. A record handed for another client was inherited by a harness the body launched in
    /// turn, and is not its own.
    #[test]
    fn a_body_handed_its_sessions_record_ends_on_it() {
        use HookEventKind::{SessionEnd, SessionStart};
        use SessionLifecycleEventKind::{SessionEnded as Ended, SessionStarted as Started};
        let hooks = HandedHooks::new();
        hooks.ledger.open_handed(OWNED, "open-1");
        hooks.hook("claude", SessionStart, "sess-1", "open-1");
        assert!(hooks.kinds(OWNED).is_empty(), "the owner records the start");
        // The open answered, and the owner recorded its start; its End ended it.
        assert_eq!(
            record_harness_start(&hooks.home, OWNED).unwrap(),
            HarnessStart::First
        );
        assert!(record_harness_end(&hooks.home, OWNED).unwrap());
        // The body's end comes after.
        hooks.hook("claude", SessionEnd, "sess-1", "open-1");
        assert_eq!(hooks.kinds(OWNED), [Started, Ended]);
        assert!(
            hooks.kinds(&own("sess-1")).is_empty(),
            "no record under the harness's own id"
        );

        // Another client's hook keeps its own record.
        hooks.hook("codex", SessionStart, "sess-2", "open-1");
        assert_eq!(hooks.kinds("aikit:session:codex:sess-2"), [Started]);
        assert_eq!(hooks.kinds(OWNED), [Started, Ended]);
        let handed = |session_ref: &str| SessionLifecycleHandoff {
            session_ref: session_ref.into(),
            client: "claude".into(),
            open: "open-1".into(),
        };
        assert_eq!(handed_session_ref(Some(&handed(OWNED)), "codex"), None);
        assert_eq!(
            handed_session_ref(Some(&handed(OWNED)), "claude").as_deref(),
            Some(OWNED)
        );
        assert_eq!(
            handed_session_ref(Some(&handed("aikit:session:claude:")), "claude"),
            None
        );
        assert_eq!(
            handed_session_ref(Some(&handed("aikit:session::agent-session/talk")), "claude"),
            None
        );
        assert_eq!(handed_session_ref(None, "claude"), None);
    }

    /// ai-kit#640 (re-read F1): a claude the body launches in turn inherits what the body was
    /// handed. The record is the body's — the first session to start under the open that launched
    /// it — so the nested sessions' starts and ends land on their own records and the body's
    /// session stands running until its own end. The owner's next open launches a body under
    /// another id, which claims the record for that open; a session of the earlier open holds it
    /// no more, so the first body ending late leaves the next one running.
    #[test]
    fn a_session_the_body_launches_in_turn_keeps_its_own_record() {
        use HookEventKind::{SessionEnd, SessionStart};
        use SessionLifecycleEventKind::{SessionEnded as Ended, SessionStarted as Started};
        let hooks = HandedHooks::new();
        hooks.ledger.open_handed(OWNED, "open-1");
        hooks.hook("claude", SessionStart, "sess-body", "open-1");
        assert_eq!(
            record_harness_start(&hooks.home, OWNED).unwrap(),
            HarnessStart::First
        );
        // Two claudes the body launched in turn start and end while the body runs.
        for nested in ["sess-nested-1", "sess-nested-2"] {
            hooks.hook("claude", SessionStart, nested, "open-1");
            hooks.hook("claude", SessionEnd, nested, "open-1");
            assert_eq!(
                hooks.kinds(OWNED),
                [Started],
                "the body's session still runs"
            );
            assert_eq!(hooks.kinds(&own(nested)), [Started, Ended]);
        }
        // The body's own end ends its record; nothing lands under the body's own id.
        hooks.hook("claude", SessionEnd, "sess-body", "open-1");
        assert_eq!(hooks.kinds(OWNED), [Started, Ended]);
        assert!(hooks.kinds(&own("sess-body")).is_empty());

        // The owner's next open: its body claims the record under that open, and a claude it
        // launches keeps its own record again.
        hooks.ledger.open_handed(OWNED, "open-2");
        hooks.hook("claude", SessionStart, "sess-next", "open-2");
        assert_eq!(
            record_harness_start(&hooks.home, OWNED).unwrap(),
            HarnessStart::Resumed
        );
        hooks.hook("claude", SessionStart, "sess-nested-3", "open-2");
        hooks.hook("claude", SessionEnd, "sess-nested-3", "open-2");
        assert_eq!(hooks.kinds(OWNED), [Started, Ended, Started]);
        assert_eq!(hooks.kinds(&own("sess-nested-3")), [Started, Ended]);
        // The first body, ending late, holds the record no more.
        hooks.hook("claude", SessionEnd, "sess-body", "open-1");
        assert_eq!(hooks.kinds(OWNED), [Started, Ended, Started]);
        hooks.hook("claude", SessionEnd, "sess-next", "open-2");
        assert_eq!(hooks.kinds(OWNED), [Started, Ended, Started, Ended]);
        assert!(hooks.kinds(&own("sess-next")).is_empty());
    }

    /// ai-kit#640 (re-read F3): a body records no start on the record it was handed; its start is
    /// the owner's, recorded once the open has answered. So a body whose open then fails (a model
    /// or mode refused, the startup deadline, its basis changed) leaves the record as it stood: an
    /// ended session stays ended, with no body. The next open's body claims the record under its
    /// own open, though the failed one claimed it under the open before. Unfenced, the failed
    /// body's start resumed the ended record, which then read running with no body and no open
    /// that answered.
    #[test]
    fn a_body_whose_open_fails_leaves_its_sessions_record_as_it_stood() {
        use HookEventKind::{SessionEnd, SessionStart};
        use SessionLifecycleEventKind::{SessionEnded as Ended, SessionStarted as Started};
        let hooks = HandedHooks::new();
        // The session ran, and its End ended it.
        assert_eq!(
            record_harness_start(&hooks.home, OWNED).unwrap(),
            HarnessStart::First
        );
        assert!(record_harness_end(&hooks.home, OWNED).unwrap());
        // Replace model: its open launches a body, whose start hook runs; then the open fails.
        hooks.ledger.open_handed(OWNED, "open-2");
        hooks.hook("claude", SessionStart, "sess-failed", "open-2");
        assert_eq!(hooks.kinds(OWNED), [Started, Ended], "the End stands");
        assert!(hooks.kinds(&own("sess-failed")).is_empty());

        // The next open: its body claims the record, and the open answers with the start.
        hooks.ledger.open_handed(OWNED, "open-3");
        hooks.hook("claude", SessionStart, "sess-next", "open-3");
        assert_eq!(hooks.kinds(OWNED), [Started, Ended]);
        assert_eq!(
            record_harness_start(&hooks.home, OWNED).unwrap(),
            HarnessStart::Resumed
        );
        hooks.hook("claude", SessionStart, "sess-nested", "open-3");
        hooks.hook("claude", SessionEnd, "sess-nested", "open-3");
        assert_eq!(hooks.kinds(OWNED), [Started, Ended, Started]);
        hooks.hook("claude", SessionEnd, "sess-next", "open-3");
        assert_eq!(hooks.kinds(OWNED), [Started, Ended, Started, Ended]);
        assert!(hooks.kinds(&own("sess-next")).is_empty());
    }

    /// ai-kit#640 (re-read F2): a session keeps the record its first start chose, spelt for the
    /// harness that first ran it. A successor body on another harness (Replace model onto a
    /// claude-code provider after a codex one) is handed that record with its own harness's
    /// spelling of the client, so its hooks hold the record: its end lands there, and no second
    /// record opens under the harness's own id. A harness of the record's spelling that the body
    /// launches in turn keeps its own record. Unfenced, the claude body's hooks refused the
    /// codex-spelt record and opened `aikit:session:claude:<its own id>`, which no End reached.
    #[test]
    fn a_successor_on_another_harness_keeps_the_record_its_session_chose() {
        use HookEventKind::{SessionEnd, SessionStart};
        use SessionLifecycleEventKind::{SessionEnded as Ended, SessionStarted as Started};
        const KEPT: &str = "aikit:session:codex:agent-session/talk";
        let hooks = HandedHooks::new();
        // The session's first open ran codex; its body was released, and the record runs on.
        assert_eq!(
            record_harness_start(&hooks.home, KEPT).unwrap(),
            HarnessStart::First
        );
        // Replace model launches a claude body, handed the kept record.
        let claude = SessionLifecycleHandoff {
            session_ref: KEPT.into(),
            client: "claude".into(),
            open: "open-2".into(),
        };
        hooks.ledger.open_handed(KEPT, "open-2");
        hooks.hook_handed("claude", SessionStart, "sess-claude", &claude);
        // A codex the claude body launches in turn keeps its own record.
        hooks.hook_handed("codex", SessionStart, "sess-codex", &claude);
        hooks.hook_handed("codex", SessionEnd, "sess-codex", &claude);
        assert_eq!(
            hooks.kinds("aikit:session:codex:sess-codex"),
            [Started, Ended]
        );
        assert_eq!(hooks.kinds(KEPT), [Started]);
        // The claude body's end lands on the kept record.
        hooks.hook_handed("claude", SessionEnd, "sess-claude", &claude);
        assert_eq!(hooks.kinds(KEPT), [Started, Ended]);
        assert!(
            hooks.kinds(&own("sess-claude")).is_empty(),
            "no second record under the harness's own id"
        );
    }

    #[test]
    fn a_refused_close_is_a_warning_and_the_end_is_still_recorded() {
        let temp = tempfile::tempdir().unwrap();
        let home = AikitHome::at(temp.path());
        let ledger = FloorLedger::in_state(&home.state());
        let world = example_world();
        let runner = ScriptedRunner::new()
            .on(
                "central.work.policy",
                &success(json!({"revision": "rev-policy"})),
            )
            .on(
                "central.now.allocate",
                &success(json!({"now_ref": NOW_REF, "writable_destination": SCRATCH})),
            )
            .on("central.now.list", &listed("active"))
            .on(
                "central.now.lifecycle",
                &json!({"ok": false, "status": "denied",
                    "error": {"code": "denied", "message": "native credential is required"}})
                .to_string(),
            );
        let payload = json!({"session_id": "sess-1"});
        let mut start = decision(HookEventKind::SessionStart);
        session_boundary(
            &mut start,
            &hook_at(PROJECT, HookEventKind::SessionStart, payload.clone()),
            Some(&world),
            Some(&home),
            Some(&ledger),
            &runner,
            true,
            None,
        );
        let mut end = decision(HookEventKind::SessionEnd);
        session_boundary(
            &mut end,
            &hook_at(PROJECT, HookEventKind::SessionEnd, payload),
            Some(&world),
            Some(&home),
            Some(&ledger),
            &runner,
            true,
            None,
        );
        assert!(end.allowed, "a refusal never changes the verdict");
        assert_eq!(end.warnings.len(), 1, "{:?}", end.warnings);
        assert!(end.warnings[0].contains("not closed in Central: native credential is required"));
        assert_eq!(
            home.session_lifecycle_read_model(&harness_session(REF).unwrap())
                .unwrap()
                .state,
            SessionLifecycleState::Ended
        );
        assert_eq!(
            calls_of(&runner, "central.now.lifecycle").len(),
            1,
            "no retry"
        );

        // The NOW stayed active, so a resume keeps it: the session runs
        // again and nothing is allocated or moved.
        let mut resumed = decision(HookEventKind::SessionStart);
        session_boundary(
            &mut resumed,
            &hook_at(
                PROJECT,
                HookEventKind::SessionStart,
                json!({"session_id": "sess-1", "source": "resume"}),
            ),
            Some(&world),
            Some(&home),
            Some(&ledger),
            &runner,
            true,
            None,
        );
        assert!(resumed.warnings.is_empty(), "{:?}", resumed.warnings);
        assert_eq!(calls_of(&runner, "central.now.allocate").len(), 1);
        assert_eq!(calls_of(&runner, "central.now.lifecycle").len(), 1);
        let held = ledger.session_now("sess-1").unwrap();
        assert!(!held.closed);
        assert_eq!(held.now.now_ref, NOW_REF);
        assert_eq!(
            home.session_lifecycle_read_model(&harness_session(REF).unwrap())
                .unwrap()
                .state,
            SessionLifecycleState::Running
        );
    }

    #[test]
    fn a_session_resumed_after_its_end_runs_again_and_re_enters_its_own_now() {
        let temp = tempfile::tempdir().unwrap();
        let home = AikitHome::at(temp.path());
        let ledger = FloorLedger::in_state(&home.state());
        let (active, closed) = (listed("active"), listed("closed"));
        let runner = owner_runner_listing(&[&active, &closed]);
        let world = example_world();
        dispatch(
            &hook_at(
                PROJECT,
                HookEventKind::SessionStart,
                json!({"session_id": "sess-1", "source": "startup"}),
            ),
            Some(&world),
            &home,
            &ledger,
            &runner,
        );
        dispatch(
            &hook_at(
                PROJECT,
                HookEventKind::SessionEnd,
                json!({"session_id": "sess-1", "reason": "exit"}),
            ),
            Some(&world),
            &home,
            &ledger,
            &runner,
        );

        let resume = hook_at(
            PROJECT,
            HookEventKind::SessionStart,
            json!({"session_id": "sess-1", "source": "resume"}),
        );
        let resumed = dispatch(&resume, Some(&world), &home, &ledger, &runner);
        assert!(resumed.warnings.is_empty(), "{:?}", resumed.warnings);

        // The lifecycle runs again on the session's own history.
        let model = home
            .session_lifecycle_read_model(&harness_session(REF).unwrap())
            .unwrap();
        assert_eq!(model.state, SessionLifecycleState::Running);
        assert_eq!(model.event_count, 3);

        // Its closed NOW is re-entered — the same NOW, not another — and
        // the resumed start names it and its scratch place again.
        let moves = calls_of(&runner, "central.now.lifecycle");
        assert_eq!(moves.len(), 2, "{moves:?}");
        assert!(moves[0].contains("\"lifecycle\":\"closed\""));
        assert!(moves[1].contains("\"lifecycle\":\"active\""));
        assert!(moves[1].contains(NOW_REF));
        let allocations = calls_of(&runner, "central.now.allocate");
        assert!(allocations
            .iter()
            .all(|line| line.contains("\"task_ref\":\"aikit:session:claude:sess-1\"")));
        let text = resumed.injected_text();
        assert!(text.contains(&format!("NOW: {NOW_REF} (this session's own)")));
        assert!(text.contains(&format!("scratch: {SCRATCH}")));
        assert!(ledger
            .session_now("sess-1")
            .is_some_and(|held| !held.closed));
    }

    #[test]
    fn a_live_session_replayed_in_another_world_keeps_its_one_now() {
        let temp = tempfile::tempdir().unwrap();
        let home = AikitHome::at(temp.path());
        let ledger = FloorLedger::in_state(&home.state());
        let runner = owner_runner();
        let payload = json!({"session_id": "sess-1"});
        dispatch(
            &hook_at(PROJECT, HookEventKind::SessionStart, payload.clone()),
            Some(&example_world()),
            &home,
            &ledger,
            &runner,
        );
        // Resumed from the root: no second NOW for the same session.
        let root = SessionWorld {
            central_root: CENTRAL.into(),
            project: None,
        };
        let replayed = dispatch(
            &hook_at(CENTRAL, HookEventKind::SessionStart, payload),
            Some(&root),
            &home,
            &ledger,
            &runner,
        );
        assert!(replayed.warnings.is_empty(), "{:?}", replayed.warnings);
        assert_eq!(calls_of(&runner, "central.now.allocate").len(), 1);
        assert!(replayed
            .injected_text()
            .contains(&format!("NOW: {NOW_REF} (this session's own)")));
        assert_eq!(ledger.session_now("sess-1").unwrap().world, example_world());
    }

    #[test]
    fn a_failed_allocation_is_a_warning_and_the_start_is_still_recorded() {
        let temp = tempfile::tempdir().unwrap();
        let home = AikitHome::at(temp.path());
        let ledger = FloorLedger::in_state(&home.state());
        let runner = ScriptedRunner::new()
            .on(
                "central.work.policy",
                &success(json!({"revision": "rev-policy"})),
            )
            .failing("central.now.allocate", 9, "owner unavailable");
        let mut start = decision(HookEventKind::SessionStart);
        session_boundary(
            &mut start,
            &hook_at(
                PROJECT,
                HookEventKind::SessionStart,
                json!({"session_id": "sess-1"}),
            ),
            Some(&example_world()),
            Some(&home),
            Some(&ledger),
            &runner,
            true,
            None,
        );
        assert!(start.allowed);
        assert!(start.warnings[0].contains("this session's NOW was not opened in Central"));
        assert_eq!(
            home.session_lifecycle_history(&harness_session(REF).unwrap())
                .unwrap()
                .len(),
            1
        );
        assert!(ledger.session_now("sess-1").is_none());
    }

    #[test]
    fn outside_any_central_world_the_lifecycle_is_recorded_and_central_is_not_asked() {
        let temp = tempfile::tempdir().unwrap();
        let home = AikitHome::at(temp.path());
        let ledger = FloorLedger::in_state(&home.state());
        let runner = ScriptedRunner::new();
        for kind in [HookEventKind::SessionStart, HookEventKind::SessionEnd] {
            let mut result = decision(kind.clone());
            session_boundary(
                &mut result,
                &hook_at("/elsewhere/project", kind, json!({"session_id": "sess-1"})),
                None,
                Some(&home),
                Some(&ledger),
                &runner,
                true,
                None,
            );
            assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        }
        assert!(runner.calls().is_empty());
        let model = home
            .session_lifecycle_read_model(&harness_session(REF).unwrap())
            .unwrap();
        assert_eq!(model.state, SessionLifecycleState::Ended);
    }

    #[test]
    fn an_event_without_a_session_id_records_nothing() {
        let temp = tempfile::tempdir().unwrap();
        let home = AikitHome::at(temp.path());
        let runner = ScriptedRunner::new();
        let mut result = decision(HookEventKind::SessionStart);
        session_boundary(
            &mut result,
            &hook_at(PROJECT, HookEventKind::SessionStart, json!({})),
            Some(&example_world()),
            Some(&home),
            None,
            &runner,
            true,
            None,
        );
        assert!(home.session_lifecycle_list().unwrap().is_empty());
        assert!(runner.calls().is_empty());
    }

    #[test]
    fn a_root_session_holds_a_root_now_and_hears_it_with_the_day() {
        let temp = tempfile::tempdir().unwrap();
        let home = AikitHome::at(temp.path());
        let ledger = FloorLedger::in_state(&home.state());
        let runner = ScriptedRunner::new()
            .on(
                "central.work.policy",
                &success(json!({"revision": "rev-root"})),
            )
            .on(
                "central.now.allocate",
                &success(json!({"now_ref": "central:now:control:root:k2",
                    "writable_destination": "/home/me/Central/Control/agents/now/clearings/k2/T"})),
            )
            .on("central.day.read", &day());
        let world = SessionWorld {
            central_root: CENTRAL.into(),
            project: None,
        };
        let start = hook_at(
            CENTRAL,
            HookEventKind::SessionStart,
            json!({"session_id": "sess-1"}),
        );
        let result = dispatch(&start, Some(&world), &home, &ledger, &runner);
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        let allocation = &calls_of(&runner, "central.now.allocate")[0];
        assert!(!allocation.contains("\"project\""), "{allocation}");
        assert!(allocation.contains("claude session in the root"));
        let text = result.injected_text();
        assert!(text.contains("register: the Central root (no Project)"));
        assert!(text.contains("NOW: central:now:control:root:k2"));
        assert!(text.contains("Day: central:day:control:root:2026-09-23"));
    }

    // ---- The ground of a session that opened no NOW (O-I #1016) -----------

    use aikit_adapters::central_temporal::SESSION_NOW_NOT_OPENED;

    /// A session that could not close a NOW and is resumed by a caller that
    /// can opens its NOW then, and its ground stops saying it was not opened.
    /// One that can from its first start allocates as before, and its ground
    /// never says it.
    #[test]
    fn a_session_that_can_close_a_now_opens_one_and_its_ground_names_it() {
        let temp = tempfile::tempdir().unwrap();
        let home = AikitHome::at(temp.path());
        let ledger = FloorLedger::in_state(&home.state());
        let runner = owner_runner_listing(&[&success(json!({"records": []}))]);
        let world = example_world();
        let at = |session: &str, kind: HookEventKind, source: &str| {
            hook_at(
                PROJECT,
                kind,
                json!({"session_id": session, "source": source}),
            )
        };

        let opened = dispatch_holding(
            &at("sess-1", HookEventKind::SessionStart, "startup"),
            Some(&world),
            &home,
            &ledger,
            &runner,
            true,
        );
        assert!(opened.warnings.is_empty(), "{:?}", opened.warnings);
        let allocations = calls_of(&runner, "central.now.allocate");
        assert_eq!(allocations.len(), 1);
        assert!(allocations[0].contains("\"task_ref\":\"aikit:session:claude:sess-1\""));
        let text = opened.injected_text();
        assert!(text.contains(&format!("NOW: {NOW_REF} (this session's own)")));
        assert!(!text.contains(SESSION_NOW_NOT_OPENED), "{text}");

        let unopened = dispatch_holding(
            &at("sess-2", HookEventKind::SessionStart, "startup"),
            Some(&world),
            &home,
            &ledger,
            &runner,
            false,
        );
        assert!(unopened.injected_text().contains(SESSION_NOW_NOT_OPENED));
        assert!(ledger.now_not_opened("sess-2"));
        assert_eq!(calls_of(&runner, "central.now.allocate").len(), 1);
        dispatch_holding(
            &at("sess-2", HookEventKind::SessionEnd, "exit"),
            Some(&world),
            &home,
            &ledger,
            &runner,
            false,
        );
        let resumed = dispatch_holding(
            &at("sess-2", HookEventKind::SessionStart, "resume"),
            Some(&world),
            &home,
            &ledger,
            &runner,
            true,
        );
        assert!(resumed.warnings.is_empty(), "{:?}", resumed.warnings);
        let allocations = calls_of(&runner, "central.now.allocate");
        assert_eq!(allocations.len(), 2);
        assert!(allocations[1].contains("\"task_ref\":\"aikit:session:claude:sess-2\""));
        assert!(ledger.session_now("sess-2").is_some());
        assert!(!ledger.now_not_opened("sess-2"));
        let text = resumed.injected_text();
        assert!(text.contains(&format!("NOW: {NOW_REF} (this session's own)")));
        assert!(!text.contains(SESSION_NOW_NOT_OPENED), "{text}");
    }

    /// A root session that cannot close a NOW hears the root ground with the
    /// Day and the line that says its NOW was not opened; no NOW Action is
    /// asked of Central.
    #[test]
    fn a_root_session_that_cannot_close_a_now_hears_it_was_not_opened() {
        let temp = tempfile::tempdir().unwrap();
        let home = AikitHome::at(temp.path());
        let ledger = FloorLedger::in_state(&home.state());
        let runner = ScriptedRunner::new().on("central.day.read", &day());
        let world = SessionWorld {
            central_root: CENTRAL.into(),
            project: None,
        };
        let start = hook_at(
            CENTRAL,
            HookEventKind::SessionStart,
            json!({"session_id": "sess-1"}),
        );
        let result = dispatch_holding(&start, Some(&world), &home, &ledger, &runner, false);
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        for action in [
            "central.now.allocate",
            "central.now.list",
            "central.now.lifecycle",
        ] {
            assert!(calls_of(&runner, action).is_empty(), "{action} was asked");
        }
        let text = result.injected_text();
        assert!(text.contains("register: the Central root (no Project)"));
        assert_eq!(text.matches(SESSION_NOW_NOT_OPENED).count(), 1, "{text}");
        assert!(text.contains("Day: central:day:control:root:2026-09-23"));
    }
}
