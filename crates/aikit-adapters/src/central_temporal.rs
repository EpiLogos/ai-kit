//! Central temporal-ground adapter.
//!
//! Central owns ProjectCentral NOW and the Day. AIKit consumes that owner surface
//! through `ctrl --json action run ...`; it does not parse Central's files or mint
//! a parallel temporal model. The provider is deliberately a normal external
//! process adapter so scripted argv tests and real-binary conformance use the same
//! call path.
//!
//! The legacy Flow store (`projectcentral.flow.*`) was retired by Central #177;
//! the Day is the integrated writing medium that replaced it, and it is named
//! here by reference (`central.day.read` → day ref + revision), never inlined.
//!
//! A session makes its own NOW (O-I #1016): its start allocates one clearing
//! for the session (`central.now.allocate`, idempotent per task) and its end
//! closes it (`central.now.lifecycle`); a resumed session re-enters it. The
//! session's ground names that NOW and its scratch place, never other
//! sessions' NOW items.

use std::path::{Component, Path};

use aikit_core::{AikitError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::runner::{CommandRunner, Output};

pub const NOW_INSPECT_ACTION: &str = "projectcentral.now.inspect";
pub const DAY_READ_ACTION: &str = "central.day.read";
pub const NOW_ALLOCATE_ACTION: &str = "central.now.allocate";
pub const NOW_LIFECYCLE_ACTION: &str = "central.now.lifecycle";
pub const NOW_LIST_ACTION: &str = "central.now.list";
/// The placement policy whose `revision` every NOW write is checked against.
pub const WORK_POLICY_ACTION: &str = "central.work.policy";
pub const CENTRAL_TEMPORAL_PROVIDER_VERSION: &str = "aikit.central-temporal/v1";

const MAX_HUMAN_REFS: usize = 8;
const MAX_DAY_REFS: usize = 3;
const MAX_RENDERED_CHARS: usize = 9_000;

/// The ref one harness session is known by in Central, as its NOW's task and
/// participant: `aikit:session:<client>:<session id>`. The session's lifecycle
/// identity in AIKit is derived from the same ref, so the two owners' records
/// join on it.
pub fn session_ref(client: &str, session_id: &str) -> String {
    format!("aikit:session:{client}:{session_id}")
}

/// The NOW Central allocated for one session, as its allocation answered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionNow {
    pub now_ref: String,
    /// The NOW's writable T destination — the session's scratch place. `None`
    /// when the allocation named none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scratch: Option<String>,
}

/// What a session's ground says in place of its own NOW when its start opened
/// none because its caller cannot close one (O-I #1016): `central.now.lifecycle`
/// is an authenticated Action, and the hook holds no authority for it yet, so a
/// NOW it opened would stay active for good.
pub const SESSION_NOW_NOT_OPENED: &str =
    "This session's NOW: not opened, because the hook holds no authority to close it yet (O-I #1016)";

impl SessionNow {
    fn lines(&self) -> Vec<String> {
        let mut lines = vec![format!("NOW: {} (this session's own)", self.now_ref)];
        if let Some(scratch) = &self.scratch {
            lines.push(format!("scratch: {scratch}"));
        }
        lines
    }
}

/// The current root Day, by reference. `None` when Central has no open Day or
/// the read failed: the floor says so rather than inventing one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CentralDayPointer {
    pub day_ref: String,
    /// `None` when the Day is open but its document is not yet written (an
    /// environmental rollover opens the Day; the person writes it).
    pub revision: Option<String>,
}

/// What Control is compared against. The remote-tracking ref as last fetched:
/// a session start never waits on the network, so the reading can lag the
/// remote but cannot stall a turn.
const CONTROL_UPSTREAM: &str = "origin/main";

/// The oldest of the commits Control holds that its remote does not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnpushedCommit {
    /// Commit date, `YYYY-MM-DD`.
    pub date: String,
    pub short_hash: String,
}

/// Control's checked-out branch is ahead of `origin/main`. Control is the
/// owner's own ground and not a Project repository, so nothing else carries it
/// to its remote: commits that stay unpushed exist on this machine alone, and
/// a push that cannot succeed (it once died on size for twelve days) fails
/// silently unless a session is told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlUnpushed {
    pub ahead: usize,
    /// `None` when the date read failed; the count alone is still a warning.
    pub oldest: Option<UnpushedCommit>,
}

impl ControlUnpushed {
    pub fn warning(&self) -> String {
        let plural = if self.ahead == 1 { "" } else { "s" };
        let since = self
            .oldest
            .as_ref()
            .map(|oldest| {
                format!(
                    ", unpushed since {} (oldest {})",
                    oldest.date, oldest.short_hash
                )
            })
            .unwrap_or_default();
        format!(
            "WARNING: Control is {} commit{plural} ahead of {CONTROL_UPSTREAM}{since}. Until they are pushed the owner's ground exists only on this machine. Say so to the owner in your first reply.",
            self.ahead
        )
    }

    /// The whole fragment for a session in the Central world that is no
    /// Project (the Central root, Control itself) and holds no NOW of its own:
    /// the warning is all the ground there is.
    pub fn render_standalone(&self) -> String {
        [
            "[Central temporal ground]".to_owned(),
            owner_line(),
            STANDING_LINE.to_owned(),
            self.warning(),
        ]
        .join("\n")
    }
}

/// The ground of a session in the Central world that is no Project (the root
/// itself, `Control`). With a NOW of its own the session hears that NOW, its
/// scratch place and the Day; when its start opened none because it could not
/// close one (`not_opened`), it hears that in the NOW's place, with the Day;
/// otherwise only an unpushed Control, and otherwise nothing at all.
pub fn render_root_ground(
    control_unpushed: Option<&ControlUnpushed>,
    session_now: Option<&SessionNow>,
    not_opened: bool,
    day: Option<&CentralDayPointer>,
) -> Option<String> {
    if session_now.is_none() && !not_opened {
        return control_unpushed.map(ControlUnpushed::render_standalone);
    }
    let mut lines = vec![
        "[Central temporal ground]".to_owned(),
        owner_line(),
        "register: the Central root (no Project)".to_owned(),
        STANDING_LINE.to_owned(),
    ];
    if let Some(unpushed) = control_unpushed {
        lines.push(unpushed.warning());
    }
    match session_now {
        Some(session_now) => lines.extend(session_now.lines()),
        None => lines.push(SESSION_NOW_NOT_OPENED.to_owned()),
    }
    lines.push(day_line(day));
    Some(bounded(&lines.join("\n"), MAX_RENDERED_CHARS))
}

fn day_line(day: Option<&CentralDayPointer>) -> String {
    match day {
        Some(CentralDayPointer {
            day_ref,
            revision: Some(revision),
        }) => format!(
            "Day: {day_ref} @ {revision} (read it with `ctrl --json action run {DAY_READ_ACTION} '{{}}'`)"
        ),
        Some(CentralDayPointer {
            day_ref,
            revision: None,
        }) => format!("Day: {day_ref} (open; its document is not yet written)"),
        None => "Day: no open root Day".to_owned(),
    }
}

const STANDING_LINE: &str = "standing: current orientation only; this does not grant authority, invoke an Agent/model, or activate a capability.";

fn owner_line() -> String {
    format!("owner: Central ({CENTRAL_TEMPORAL_PROVIDER_VERSION})")
}

/// Ask git, locally, whether Control holds commits `origin/main` does not.
///
/// Fail-soft in every direction: no git, no `Control` checkout, no remote-
/// tracking ref, an unreadable count — all read as silence, never as a guess
/// and never as an error that could cost a turn. In step with the remote also
/// reads as silence.
pub fn read_control_unpushed<R: CommandRunner>(
    runner: &R,
    central_root: &Path,
) -> Option<ControlUnpushed> {
    let control = central_root.join("Control");
    let range = format!("{CONTROL_UPSTREAM}..HEAD");
    let git = |args: &[&str]| -> Option<String> {
        let mut argv = vec![
            "git".to_owned(),
            "-C".to_owned(),
            control.display().to_string(),
        ];
        argv.extend(args.iter().map(|arg| (*arg).to_owned()));
        let output = runner.run(&argv).ok()?;
        output.ok().then(|| output.stdout.trim().to_owned())
    };

    let ahead: usize = git(&["rev-list", "--count", &range])?.parse().ok()?;
    if ahead == 0 {
        return None;
    }
    // The walk lists newest first, so the oldest unpushed commit is the last.
    let skip = format!("--skip={}", ahead - 1);
    let oldest = git(&[
        "log",
        "--no-show-signature",
        "--format=%cs %h",
        &skip,
        "-n",
        "1",
        &range,
    ])
    .and_then(|line| {
        let (date, short_hash) = line.split_once(' ')?;
        Some(UnpushedCommit {
            date: date.to_owned(),
            short_hash: short_hash.to_owned(),
        })
    });
    Some(ControlUnpushed { ahead, oldest })
}

#[derive(Debug, Clone, PartialEq)]
pub struct CentralTemporalGround {
    pub project: String,
    /// Central's NOW inspection of the Project. The rendered ground carries
    /// its status, the human's current-source refs and the recent DAY refs,
    /// never its active items: other sessions' handoffs do not ride a
    /// session's turns.
    pub now: Value,
    pub day: Option<CentralDayPointer>,
    /// Control is ahead of its remote. `None` when it is in step or unreadable.
    pub control_unpushed: Option<ControlUnpushed>,
    /// This session's own NOW, when its start allocated one.
    pub session_now: Option<SessionNow>,
    /// The session's start opened no NOW because its caller cannot close one;
    /// the ground says so in the NOW's place.
    pub session_now_not_opened: bool,
}

impl CentralTemporalGround {
    /// Render an orientation fragment, not an authority or activation grant:
    /// the Project and its NOW's status, this session's own NOW and scratch
    /// place, the human's current-source refs, the recent DAY refs, and
    /// today's Day.
    pub fn render(&self) -> String {
        let mut lines = vec![
            "[Central temporal ground]".to_owned(),
            owner_line(),
            format!("project: {}", self.project),
            STANDING_LINE.to_owned(),
        ];
        // First, after the standing line: the bound below cuts from the end.
        if let Some(unpushed) = &self.control_unpushed {
            lines.push(unpushed.warning());
        }

        let exists = self
            .now
            .get("exists")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        lines.push(format!(
            "NOW: {}",
            if exists { "present" } else { "not initialized" }
        ));
        if let Some(session_now) = &self.session_now {
            lines.extend(session_now.lines());
        } else if self.session_now_not_opened {
            lines.push(SESSION_NOW_NOT_OPENED.to_owned());
        }

        if let Some(refs) = self.now.get("human_scratch").and_then(Value::as_array) {
            let refs = refs
                .iter()
                .filter_map(Value::as_str)
                .take(MAX_HUMAN_REFS)
                .collect::<Vec<_>>();
            if !refs.is_empty() {
                lines.push(format!("human-current-source refs: {}", refs.join(", ")));
            }
        }

        if let Some(days) = self.now.get("day_records").and_then(Value::as_array) {
            let mut refs = days.iter().filter_map(Value::as_str).collect::<Vec<_>>();
            refs.sort();
            let start = refs.len().saturating_sub(MAX_DAY_REFS);
            if !refs[start..].is_empty() {
                lines.push(format!("recent DAY refs: {}", refs[start..].join(", ")));
            }
        }

        lines.push(day_line(self.day.as_ref()));
        bounded(&lines.join("\n"), MAX_RENDERED_CHARS)
    }
}

/// Consume Central's public Action surface for one Project physically resident in
/// `Central/Work`. Returns `Ok(None)` when this Project is not in that world.
pub fn read_central_temporal_ground<R: CommandRunner>(
    runner: &R,
    central_root: &Path,
    project_root: &Path,
) -> Result<Option<CentralTemporalGround>> {
    let Some(project) = project_member(central_root, project_root) else {
        return Ok(None);
    };

    let now = action(
        runner,
        central_root,
        NOW_INSPECT_ACTION,
        json!({"project": project}),
    )?;
    let day = read_day_pointer(runner, central_root);
    let control_unpushed = read_control_unpushed(runner, central_root);

    Ok(Some(CentralTemporalGround {
        project,
        now,
        day,
        control_unpushed,
        session_now: None,
        session_now_not_opened: false,
    }))
}

/// Today's root Day, by reference. Optional: a world with no open Day (or an
/// older Central) reads as `None`, and the ground says so.
pub fn read_day_pointer<R: CommandRunner>(
    runner: &R,
    central_root: &Path,
) -> Option<CentralDayPointer> {
    let day = action(runner, central_root, DAY_READ_ACTION, json!({})).ok()?;
    Some(CentralDayPointer {
        day_ref: day.get("day_ref")?.as_str()?.to_owned(),
        revision: day
            .get("revision")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

/// Open the session's own NOW: read the placement policy's revision, then
/// allocate one clearing whose task and only participant is the session's
/// ref. Allocation is idempotent per task while that NOW is active, so a
/// replay of a live session answers the same NOW. A NOW the session's end
/// closed is not allocated again: [`reopen_session_now`] re-enters it.
/// `project` is `None` for a session in the root register.
pub fn open_session_now<R: CommandRunner>(
    runner: &R,
    central_root: &Path,
    project: Option<&str>,
    client: &str,
    session_ref: &str,
) -> Result<SessionNow> {
    let policy = policy_revision(runner, central_root, project)?;
    allocate_session_now(runner, central_root, project, client, session_ref, &policy)
}

/// Open the NOW of a session that may have held one before: a session
/// resumed after its end, or one whose start this record never saw. Central
/// refuses another allocation of a NOW that is no longer active and asks for
/// explicit lifecycle re-entry, which keeps the NOW's identity: the session's
/// NOW is found among the clearings naming it, moved back to `active` at the
/// revision just listed, and answered by its allocation with its scratch
/// place. An active NOW is answered as it stands; a session with no NOW in
/// this scope gets one. Central's refusal comes back in its own words.
pub fn reopen_session_now<R: CommandRunner>(
    runner: &R,
    central_root: &Path,
    project: Option<&str>,
    client: &str,
    session_ref: &str,
) -> Result<SessionNow> {
    let listed = listed_session_now(runner, central_root, project, session_ref)?;
    let policy = policy_revision(runner, central_root, project)?;
    if let Some(listed) = listed.filter(|listed| listed.lifecycle != "active") {
        move_session_now(runner, central_root, project, &listed, &policy, "active")?;
    }
    allocate_session_now(runner, central_root, project, client, session_ref, &policy)
}

fn allocate_session_now<R: CommandRunner>(
    runner: &R,
    central_root: &Path,
    project: Option<&str>,
    client: &str,
    session_ref: &str,
    policy: &str,
) -> Result<SessionNow> {
    let purpose = format!("{client} session in {}", project.unwrap_or("the root"));
    let allocation = action(
        runner,
        central_root,
        NOW_ALLOCATE_ACTION,
        scoped(
            project,
            json!({
                "task_ref": session_ref,
                "purpose": purpose,
                "participant_refs": [session_ref],
                "expected_policy_revision": policy,
            }),
        ),
    )?;
    let now_ref = allocation
        .get("now_ref")
        .or_else(|| allocation.pointer("/record/now_ref"))
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_output(NOW_ALLOCATE_ACTION, "names no now_ref"))?
        .to_owned();
    let scratch = allocation
        .get("writable_destination")
        .and_then(|destination| {
            destination
                .as_str()
                .or_else(|| destination.get("path")?.as_str())
        })
        .map(str::to_owned);
    Ok(SessionNow { now_ref, scratch })
}

/// Close the session's own NOW: find it among the clearings naming the
/// session as participant, then move it to `closed` at the revision just
/// read. `Ok(None)` when the session holds no active NOW in this scope.
/// Central's refusal (an authority it does not grant, T fixtures not yet
/// distilled) comes back as Central's own error; nothing retries it.
pub fn close_session_now<R: CommandRunner>(
    runner: &R,
    central_root: &Path,
    project: Option<&str>,
    session_ref: &str,
) -> Result<Option<String>> {
    let Some(listed) = listed_session_now(runner, central_root, project, session_ref)?
        .filter(|listed| listed.lifecycle == "active")
    else {
        return Ok(None);
    };
    let policy = policy_revision(runner, central_root, project)?;
    move_session_now(runner, central_root, project, &listed, &policy, "closed")?;
    Ok(Some(listed.now_ref))
}

/// The session's NOW as `central.now.list` lists it in one scope.
struct ListedNow {
    now_ref: String,
    revision: String,
    lifecycle: String,
}

/// The NOW whose task is the session's ref, among the clearings naming the
/// session as participant, whatever its lifecycle. One task has one NOW in a
/// scope.
fn listed_session_now<R: CommandRunner>(
    runner: &R,
    central_root: &Path,
    project: Option<&str>,
    session_ref: &str,
) -> Result<Option<ListedNow>> {
    let listing = action(
        runner,
        central_root,
        NOW_LIST_ACTION,
        scoped(project, json!({"participant_refs": [session_ref]})),
    )?;
    let Some(record) = listing
        .get("records")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|record| record.get("task_ref").and_then(Value::as_str) == Some(session_ref))
    else {
        return Ok(None);
    };
    let now_ref = record
        .get("now_ref")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_output(NOW_LIST_ACTION, "lists a record with no now_ref"))?;
    let revision = record
        .get("revision")
        .and_then(|revision| revision.get("revision").unwrap_or(revision).as_str())
        .ok_or_else(|| invalid_output(NOW_LIST_ACTION, "lists a record with no revision"))?;
    Ok(Some(ListedNow {
        now_ref: now_ref.to_owned(),
        revision: revision.to_owned(),
        lifecycle: record
            .get("lifecycle")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
    }))
}

fn move_session_now<R: CommandRunner>(
    runner: &R,
    central_root: &Path,
    project: Option<&str>,
    listed: &ListedNow,
    policy: &str,
    lifecycle: &str,
) -> Result<Value> {
    action(
        runner,
        central_root,
        NOW_LIFECYCLE_ACTION,
        scoped(
            project,
            json!({
                "now_ref": listed.now_ref,
                "expected_revision": listed.revision,
                "expected_policy_revision": policy,
                "lifecycle": lifecycle,
            }),
        ),
    )
}

fn policy_revision<R: CommandRunner>(
    runner: &R,
    central_root: &Path,
    project: Option<&str>,
) -> Result<String> {
    action(
        runner,
        central_root,
        WORK_POLICY_ACTION,
        scoped(project, json!({})),
    )?
    .get("revision")
    .and_then(Value::as_str)
    .map(str::to_owned)
    .ok_or_else(|| invalid_output(WORK_POLICY_ACTION, "names no revision"))
}

/// An Action input at a Project's scope, or at the root's when `project` is
/// `None` (the root register takes no `project`).
fn scoped(project: Option<&str>, mut input: Value) -> Value {
    if let Some(project) = project {
        input["project"] = json!(project);
    }
    input
}

fn invalid_output(id: &str, what: &str) -> AikitError {
    AikitError::new(
        "central.action_invalid_output",
        format!("Central Action {id} {what}"),
    )
    .with("action", id)
}

fn action<R: CommandRunner>(
    runner: &R,
    central_root: &Path,
    id: &str,
    input: Value,
) -> Result<Value> {
    let argv = vec![
        "ctrl".to_owned(),
        "--json".to_owned(),
        "--root".to_owned(),
        central_root.display().to_string(),
        "action".to_owned(),
        "run".to_owned(),
        id.to_owned(),
        input.to_string(),
    ];
    let output = runner.run(&argv)?;
    decode_action(output, &argv, id)
}

fn decode_action(output: Output, argv: &[String], id: &str) -> Result<Value> {
    // A refused Action exits non-zero with its envelope on stdout; that
    // envelope carries Central's own reason, which the exit status does not.
    let refusal = (!output.ok())
        .then(|| serde_json::from_str::<Value>(output.stdout.trim()).ok())
        .flatten()
        .filter(|envelope| envelope.get("ok").and_then(Value::as_bool) == Some(false));
    if !output.ok() && refusal.is_none() {
        return Err(AikitError::new(
            "central.action_failed",
            format!("Central Action {id} exited with status {}", output.status),
        )
        .with("action", id)
        .with("command", argv.join(" "))
        .with("stderr", output.stderr.trim().to_owned()));
    }
    let result: Value = match refusal {
        Some(envelope) => envelope,
        None => serde_json::from_str(output.stdout.trim()).map_err(|error| {
            AikitError::new(
                "central.action_invalid_output",
                format!("Central Action {id} returned invalid JSON: {error}"),
            )
            .with("action", id)
        })?,
    };
    if result.get("ok").and_then(Value::as_bool) != Some(true) {
        let message = result
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or("Central Action failed");
        return Err(AikitError::new("central.action_failed", message)
            .with("action", id)
            .with(
                "status",
                result
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown"),
            ));
    }
    result.get("data").cloned().ok_or_else(|| {
        AikitError::new(
            "central.action_invalid_output",
            format!("Central Action {id} succeeded without data"),
        )
        .with("action", id)
    })
}

/// The Project a path stands in, as Central names it (`ai-kit` for
/// `Work/ai-kit/crates` or the seat `worktrees/env-2/ai-kit`); `None` for the
/// root, `Control`, or anything outside this world.
pub fn project_member(central_root: &Path, project_root: &Path) -> Option<String> {
    let primary = project_ground_root(central_root, project_root)?;
    let relative = primary.strip_prefix(central_root.join("Work")).ok()?;
    Some(relative.to_string_lossy().replace('\\', "/"))
}

/// The Project a path belongs to in this Central world, as its primary
/// checkout `Work/<Name>`.
///
/// A session stands at any depth inside a Project (`Work/ai-kit/crates` is
/// Project `ai-kit`, not `ai-kit/crates`), and lane work stands in workcell
/// seats outside `Work/` (`worktrees/env-2/ai-kit`), which are linked
/// checkouts of the primary: their `.git` file names the primary's git
/// directory. Both resolve to the same Project and therefore the same NOW.
/// Anything else — the Central root itself, a folder beneath `Work/` that is
/// not a Project root's descendant, a checkout of a repository outside this
/// world — is no Project.
pub fn project_ground_root(central_root: &Path, path: &Path) -> Option<std::path::PathBuf> {
    let work = central_root.join("Work");
    if let Ok(relative) = path.strip_prefix(&work) {
        let first = relative.components().next()?;
        let Component::Normal(name) = first else {
            return None;
        };
        return Some(work.join(name));
    }
    if !path.starts_with(central_root) {
        return None;
    }
    // A linked checkout: walk up to the directory carrying `.git`, read the
    // `gitdir:` pointer, and accept it only when it points into a primary
    // checkout's own `.git/worktrees/` beneath `Work/`.
    let mut dir = Some(path);
    while let Some(current) = dir {
        if current == central_root {
            return None;
        }
        let marker = current.join(".git");
        if marker.is_file() {
            let text = std::fs::read_to_string(&marker).ok()?;
            let gitdir = text
                .lines()
                .find_map(|line| line.strip_prefix("gitdir:"))?
                .trim();
            let gitdir = Path::new(gitdir);
            let primary = gitdir
                .ancestors()
                .find(|ancestor| ancestor.file_name().is_some_and(|name| name == ".git"))?;
            let primary = primary.parent()?;
            let relative = primary.strip_prefix(&work).ok()?;
            let mut parts = relative.components();
            let (Some(Component::Normal(name)), None) = (parts.next(), parts.next()) else {
                return None;
            };
            return Some(work.join(name));
        }
        if marker.is_dir() {
            return None;
        }
        dir = current.parent();
    }
    None
}

fn bounded(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let mut out = text
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::ScriptedRunner;

    fn success(data: Value) -> String {
        json!({"ok": true, "status": "success", "action": "fixture", "data": data}).to_string()
    }

    #[test]
    fn non_central_project_is_not_invented_into_central() {
        let runner = ScriptedRunner::new();
        let value = read_central_temporal_ground(
            &runner,
            Path::new("/home/me/Central"),
            Path::new("/tmp/project"),
        )
        .unwrap();
        assert!(value.is_none());
        assert!(runner.calls().is_empty());
    }

    #[test]
    fn owner_actions_supply_now_and_the_day_by_reference_only() {
        let now = success(json!({
            "exists": true,
            "human_scratch": ["ProjectCentral/now/user/current.md"],
            "active_items": [{"id":"h1","kind":"handoff","actor":"agent:Epii","status":"active","subject":"Current work","result":"continue from live state"}],
            "day_records": ["ProjectCentral/now/day/2026-09-01.md"]
        }));
        let day = success(json!({
            "day_ref": "central:day:control:root:2026-09-23",
            "revision": "central.content-fnv1a64/v1:1:abc",
            "content": "THE WHOLE DAY DOCUMENT",
            "automatic_agent_or_model_invocation": false
        }));
        let runner = ScriptedRunner::new()
            .on(NOW_INSPECT_ACTION, &now)
            .on(DAY_READ_ACTION, &day);

        let ground = read_central_temporal_ground(
            &runner,
            Path::new("/home/me/Central"),
            Path::new("/home/me/Central/Work/example"),
        )
        .unwrap()
        .unwrap();

        assert_eq!(ground.project, "example");
        let rendered = ground.render();
        assert!(rendered.contains("central:day:control:root:2026-09-23"));
        assert!(
            !rendered.contains("THE WHOLE DAY DOCUMENT"),
            "a pointer, never the body"
        );
        // The NOW's status, the human's current-source refs and the recent
        // DAY refs still orient the session…
        for kept in [
            "NOW: present",
            "human-current-source refs: ProjectCentral/now/user/current.md",
            "recent DAY refs: ProjectCentral/now/day/2026-09-01.md",
        ] {
            assert!(rendered.contains(kept), "{kept} missing: {rendered}");
        }
        // …while another session's handoff never rides it.
        for other in ["Current work", "continue from live state", "agent:Epii"] {
            assert!(!rendered.contains(other), "{other} rendered: {rendered}");
        }
        assert!(
            !runner
                .call_lines()
                .iter()
                .any(|line| line.contains("projectcentral.flow.")),
            "the retired Flow store is never called"
        );
        // Every Central read goes through the public Action surface; the only
        // other process is the read-only `git` question about Control's remote.
        assert!(runner.call_lines().iter().all(|line| {
            line.contains("--json --root /home/me/Central action run")
                || line.starts_with("git -C /home/me/Central/Control ")
        }));
    }

    #[test]
    fn a_missing_day_is_said_not_fatal() {
        let now = success(json!({"exists": true}));
        let runner = ScriptedRunner::new().on(NOW_INSPECT_ACTION, &now).failing(
            DAY_READ_ACTION,
            1,
            "no DayRef or current today pointer",
        );
        let ground = read_central_temporal_ground(
            &runner,
            Path::new("/home/me/Central"),
            Path::new("/home/me/Central/Work/example"),
        )
        .unwrap()
        .unwrap();
        assert!(ground.day.is_none());
        assert!(ground.render().contains("Day: no open root Day"));
    }

    #[test]
    fn an_open_day_without_a_written_document_is_still_open() {
        let now = success(json!({"exists": true}));
        let day = success(json!({
            "day_ref": "central:day:control:root:2026-09-24",
            "document": null,
            "document_state": "uninitialised"
        }));
        let runner = ScriptedRunner::new()
            .on(NOW_INSPECT_ACTION, &now)
            .on(DAY_READ_ACTION, &day);
        let ground = read_central_temporal_ground(
            &runner,
            Path::new("/home/me/Central"),
            Path::new("/home/me/Central/Work/example"),
        )
        .unwrap()
        .unwrap();
        let rendered = ground.render();
        assert!(rendered.contains(
            "Day: central:day:control:root:2026-09-24 (open; its document is not yet written)"
        ));
        assert!(!rendered.contains("no open root Day"));
    }

    #[test]
    fn rendered_ground_is_bounded_and_non_authorising() {
        let ground = CentralTemporalGround {
            project: "example".into(),
            now: json!({"exists": true, "active_items": (0..20).map(|i| json!({
                "id": format!("h{i}"), "kind": "handoff", "actor": "agent", "status": "active",
                "subject": "s", "result": "x".repeat(2_000)
            })).collect::<Vec<_>>()}),
            day: None,
            control_unpushed: None,
            session_now: None,
            session_now_not_opened: false,
        };
        let rendered = ground.render();
        assert!(rendered.chars().count() <= MAX_RENDERED_CHARS);
        assert!(rendered.contains("does not grant authority"));
        assert!(
            !rendered.contains("handoff"),
            "no NOW item rides: {rendered}"
        );
        assert!(!rendered.contains(&"x".repeat(100)));
    }

    fn control_runner(ahead: &str, oldest: &str) -> ScriptedRunner {
        ScriptedRunner::new()
            .on("rev-list --count origin/main..HEAD", ahead)
            .on("--skip=", oldest)
    }

    #[test]
    fn control_ahead_of_its_remote_is_counted_and_dated_from_the_oldest_commit() {
        // Six commits ahead: the oldest is the sixth in the default walk (skip 5).
        let runner = control_runner("6\n", "2026-10-08 14194ff\n");
        let found = read_control_unpushed(&runner, Path::new("/home/me/Central")).unwrap();
        assert_eq!(found.ahead, 6);
        assert_eq!(
            found.oldest,
            Some(UnpushedCommit {
                date: "2026-10-08".into(),
                short_hash: "14194ff".into()
            })
        );
        let lines = runner.call_lines();
        assert!(lines
            .iter()
            .all(|line| line.starts_with("git -C /home/me/Central/Control ")));
        assert!(lines.iter().any(|line| line.contains("--skip=5")));
        // Local refs only: a session start never waits on the network.
        assert!(!lines
            .iter()
            .any(|line| line.contains("fetch") || line.contains("push")));
    }

    #[test]
    fn control_in_step_with_its_remote_says_nothing() {
        let runner = control_runner("0\n", "");
        assert!(read_control_unpushed(&runner, Path::new("/home/me/Central")).is_none());
        // A count of zero never asks for a date.
        assert!(!runner.call_lines().iter().any(|l| l.contains("--skip=")));
    }

    #[test]
    fn control_without_git_or_a_remote_ref_says_nothing_rather_than_guessing() {
        let central = Path::new("/home/me/Central");
        let failing = ScriptedRunner::new().failing(
            "rev-list",
            128,
            "fatal: ambiguous argument 'origin/main..HEAD'",
        );
        assert!(read_control_unpushed(&failing, central).is_none());
        // Nothing scripted at all: the runner errors, and that is silence too.
        assert!(read_control_unpushed(&ScriptedRunner::new(), central).is_none());
        let garbled = ScriptedRunner::new().on("rev-list", "not a number\n");
        assert!(read_control_unpushed(&garbled, central).is_none());
    }

    #[test]
    fn a_failed_date_read_still_reports_the_count() {
        let runner = ScriptedRunner::new().on("rev-list --count", "3\n").failing(
            "--skip=",
            128,
            "fatal: bad revision",
        );
        let found = read_control_unpushed(&runner, Path::new("/home/me/Central")).unwrap();
        assert_eq!(found.ahead, 3);
        assert!(found.oldest.is_none());
        let warning = found.warning();
        assert!(warning.contains("3 commits ahead of origin/main"));
        assert!(!warning.contains("since"));
    }

    #[test]
    fn the_warning_names_count_date_and_oldest_commit_in_plain_words() {
        let many = ControlUnpushed {
            ahead: 6,
            oldest: Some(UnpushedCommit {
                date: "2026-10-08".into(),
                short_hash: "14194ff".into(),
            }),
        };
        let warning = many.warning();
        assert!(warning.starts_with("WARNING: Control is 6 commits ahead of origin/main"));
        assert!(warning.contains("unpushed since 2026-10-08 (oldest 14194ff)"));
        assert!(warning.contains("only on this machine"));
        let one = ControlUnpushed {
            ahead: 1,
            oldest: None,
        };
        assert!(one.warning().contains("1 commit ahead of origin/main"));
        assert!(!one.warning().contains("1 commits"));
    }

    #[test]
    fn the_unpushed_warning_leads_the_ground_and_survives_its_bound() {
        let mut ground = CentralTemporalGround {
            project: "example".into(),
            now: json!({"exists": true, "active_items": (0..20).map(|i| json!({
                "id": format!("h{i}"), "kind": "handoff", "actor": "agent", "status": "active",
                "subject": "s", "result": "x".repeat(2_000)
            })).collect::<Vec<_>>()}),
            day: None,
            control_unpushed: Some(ControlUnpushed {
                ahead: 6,
                oldest: Some(UnpushedCommit {
                    date: "2026-10-08".into(),
                    short_hash: "14194ff".into(),
                }),
            }),
            session_now: Some(SessionNow {
                now_ref: "central:now:project:example:abc".into(),
                scratch: None,
            }),
            session_now_not_opened: false,
        };
        let rendered = ground.render();
        assert!(rendered.chars().count() <= MAX_RENDERED_CHARS);
        let warning_at = rendered.find("WARNING: Control is 6 commits").unwrap();
        assert!(warning_at < rendered.find("NOW: present").unwrap());
        assert!(
            warning_at
                < rendered
                    .find("NOW: central:now:project:example:abc")
                    .unwrap()
        );
        assert!(warning_at < rendered.find("Day: no open root Day").unwrap());
        assert!(rendered.contains("does not grant authority"));

        ground.control_unpushed = None;
        assert!(!ground.render().contains("WARNING"));
    }

    #[test]
    fn the_read_ground_carries_the_unpushed_warning_when_control_is_ahead() {
        let now = success(json!({"exists": true}));
        let day = success(json!({"day_ref": "central:day:control:root:2026-10-20"}));
        let runner = control_runner("6\n", "2026-10-08 14194ff\n")
            .on(NOW_INSPECT_ACTION, &now)
            .on(DAY_READ_ACTION, &day);
        let ground = read_central_temporal_ground(
            &runner,
            Path::new("/home/me/Central"),
            Path::new("/home/me/Central/Work/example"),
        )
        .unwrap()
        .unwrap();
        assert_eq!(ground.control_unpushed.as_ref().map(|c| c.ahead), Some(6));
        assert!(ground
            .render()
            .contains("6 commits ahead of origin/main, unpushed since 2026-10-08"));
    }

    /// Real git, a real bare remote: the count and the oldest date must be what
    /// git itself says, not what the scripted argv assumes.
    #[test]
    fn real_git_reports_what_control_holds_that_its_remote_does_not() {
        use std::process::Command;

        fn git(dir: &Path, date: &str, args: &[&str]) {
            let output = Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
                .args(["-c", "commit.gpgsign=false"])
                .args(args)
                .env("GIT_AUTHOR_DATE", date)
                .env("GIT_COMMITTER_DATE", date)
                .output()
                .expect("git runs");
            assert!(
                output.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let temp = tempfile::tempdir().unwrap();
        let central = temp.path();
        let remote = temp.path().join("remote.git");
        let control = central.join("Control");
        std::fs::create_dir_all(&control).unwrap();
        git(
            central,
            "2026-10-01T12:00:00+00:00",
            &["init", "--bare", "-b", "main", remote.to_str().unwrap()],
        );
        git(
            &control,
            "2026-10-01T12:00:00+00:00",
            &["init", "-b", "master"],
        );
        git(
            &control,
            "2026-10-01T12:00:00+00:00",
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        std::fs::write(control.join("a"), "a").unwrap();
        git(&control, "2026-10-01T12:00:00+00:00", &["add", "a"]);
        git(
            &control,
            "2026-10-01T12:00:00+00:00",
            &["commit", "-m", "pushed"],
        );
        git(
            &control,
            "2026-10-01T12:00:00+00:00",
            &["push", "origin", "master:main"],
        );

        // In step: silent.
        let runner = crate::runner::SystemRunner::new();
        assert!(read_control_unpushed(&runner, central).is_none());

        for (day, name) in [
            ("2026-10-08", "b"),
            ("2026-10-12", "c"),
            ("2026-10-19", "d"),
        ] {
            std::fs::write(control.join(name), name).unwrap();
            git(&control, &format!("{day}T12:00:00+00:00"), &["add", name]);
            git(
                &control,
                &format!("{day}T12:00:00+00:00"),
                &["commit", "-m", name],
            );
        }
        let found = read_control_unpushed(&runner, central).unwrap();
        assert_eq!(found.ahead, 3);
        assert_eq!(found.oldest.as_ref().unwrap().date, "2026-10-08");
        assert!(found
            .warning()
            .contains("3 commits ahead of origin/main, unpushed since 2026-10-08"));

        // Pushing it clears the warning.
        git(
            &control,
            "2026-10-20T12:00:00+00:00",
            &["push", "origin", "master:main"],
        );
        assert!(read_control_unpushed(&runner, central).is_none());
    }

    #[test]
    fn a_session_below_a_project_root_or_in_its_seat_is_that_project() {
        let temp = tempfile::tempdir().unwrap();
        let central = temp.path();
        std::fs::create_dir_all(central.join("Work/ai-kit/.git/worktrees/ai-kit1")).unwrap();
        std::fs::create_dir_all(central.join("Work/ai-kit/crates/aikit-cli")).unwrap();
        let seat = central.join("worktrees/env-2/ai-kit");
        std::fs::create_dir_all(seat.join("crates")).unwrap();
        std::fs::write(
            seat.join(".git"),
            format!(
                "gitdir: {}\n",
                central.join("Work/ai-kit/.git/worktrees/ai-kit1").display()
            ),
        )
        .unwrap();
        let primary = central.join("Work/ai-kit");
        assert_eq!(
            project_ground_root(central, &primary),
            Some(primary.clone())
        );
        assert_eq!(
            project_ground_root(central, &primary.join("crates/aikit-cli")),
            Some(primary.clone())
        );
        assert_eq!(project_ground_root(central, &seat), Some(primary.clone()));
        assert_eq!(
            project_ground_root(central, &seat.join("crates")),
            Some(primary.clone())
        );
        assert_eq!(
            project_member(central, &seat.join("crates")).as_deref(),
            Some("ai-kit")
        );
        // The Central root itself and an unrelated linked checkout are no Project.
        assert_eq!(project_ground_root(central, central), None);
        let stray = central.join("worktrees/env-2/elsewhere");
        std::fs::create_dir_all(&stray).unwrap();
        std::fs::write(
            stray.join(".git"),
            "gitdir: /somewhere/else/.git/worktrees/x\n",
        )
        .unwrap();
        assert_eq!(project_ground_root(central, &stray), None);
    }

    const ROOT: &str = "/home/me/Central";
    const REF: &str = "aikit:session:claude:sess-1";
    const SCRATCH: &str = "/home/me/Central/Work/example/ProjectCentral/agents/now/clearings/k1/T";

    fn policy(revision: &str) -> String {
        success(json!({"revision": revision, "scope_ref": "project:example"}))
    }

    fn allocation() -> String {
        success(json!({
            "schema": "central.now-allocation/v1",
            "created": true,
            "now_ref": "central:now:project:example:k1",
            "writable_destination": SCRATCH,
            "artifact_namespace": "T",
        }))
    }

    /// One `ctrl --json --root <root> action run <id> <input>` call, split into
    /// its Action id and its parsed input.
    fn ctrl_call(call: &[String]) -> (String, Value) {
        assert_eq!(call.len(), 8, "{call:?}");
        assert_eq!(
            call[..6],
            ["ctrl", "--json", "--root", ROOT, "action", "run"]
        );
        (call[6].clone(), serde_json::from_str(&call[7]).unwrap())
    }

    /// Answers one Action as the real `ctrl` refuses it: exit 4, the refusal
    /// envelope on stdout and nothing on stderr. Every form refuses that
    /// Action and passes the rest on to the runner it wraps.
    struct Refusing {
        inner: ScriptedRunner,
        action: &'static str,
        envelope: String,
    }

    impl Refusing {
        fn refusal(&self, argv: &[String]) -> Option<Output> {
            argv.iter().any(|arg| arg == self.action).then(|| Output {
                status: 4,
                stdout: self.envelope.clone(),
                stderr: String::new(),
            })
        }
    }

    impl CommandRunner for Refusing {
        fn run(&self, argv: &[String]) -> Result<Output> {
            match self.refusal(argv) {
                Some(refused) => Ok(refused),
                None => self.inner.run(argv),
            }
        }

        fn starts_in(&self) -> Option<std::path::PathBuf> {
            self.inner.starts_in()
        }

        fn run_in(
            &self,
            dir: &Path,
            argv: &[String],
            timeout: Option<std::time::Duration>,
        ) -> Result<Output> {
            match self.refusal(argv) {
                Some(refused) => Ok(refused),
                None => self.inner.run_in(dir, argv, timeout),
            }
        }

        fn run_in_with_limits(
            &self,
            dir: &Path,
            argv: &[String],
            remaining_timeout: std::time::Duration,
            remaining_aggregate_bytes: usize,
            strict_utf8: bool,
        ) -> Result<Output> {
            match self.refusal(argv) {
                Some(refused) => Ok(refused),
                None => self.inner.run_in_with_limits(
                    dir,
                    argv,
                    remaining_timeout,
                    remaining_aggregate_bytes,
                    strict_utf8,
                ),
            }
        }
    }

    #[test]
    fn a_session_is_named_by_its_client_and_harness_session() {
        assert_eq!(
            session_ref("claude", "0b6f-41c2"),
            "aikit:session:claude:0b6f-41c2"
        );
    }

    #[test]
    fn a_session_start_allocates_its_own_now_at_the_policy_revision_just_read() {
        let runner = ScriptedRunner::new()
            .on(WORK_POLICY_ACTION, &policy("rev-policy-1"))
            .on(NOW_ALLOCATE_ACTION, &allocation());
        let now =
            open_session_now(&runner, Path::new(ROOT), Some("example"), "claude", REF).unwrap();
        assert_eq!(
            now,
            SessionNow {
                now_ref: "central:now:project:example:k1".into(),
                scratch: Some(SCRATCH.into()),
            }
        );
        let calls = runner.calls();
        assert_eq!(calls.len(), 2, "{calls:?}");
        assert_eq!(
            ctrl_call(&calls[0]),
            (WORK_POLICY_ACTION.into(), json!({"project": "example"}))
        );
        assert_eq!(
            ctrl_call(&calls[1]),
            (
                NOW_ALLOCATE_ACTION.into(),
                json!({
                    "project": "example",
                    "task_ref": REF,
                    "purpose": "claude session in example",
                    "participant_refs": [REF],
                    "expected_policy_revision": "rev-policy-1",
                })
            )
        );
    }

    #[test]
    fn a_root_session_allocates_in_the_root_register_without_a_project() {
        let runner = ScriptedRunner::new()
            .on(WORK_POLICY_ACTION, &policy("rev-root"))
            .on(NOW_ALLOCATE_ACTION, &allocation());
        open_session_now(&runner, Path::new(ROOT), None, "codex", REF).unwrap();
        let calls = runner.calls();
        assert_eq!(ctrl_call(&calls[0]).1, json!({}));
        let (_, input) = ctrl_call(&calls[1]);
        assert!(input.get("project").is_none(), "{input}");
        assert_eq!(input["purpose"], "codex session in the root");
        assert_eq!(input["expected_policy_revision"], "rev-root");
    }

    #[test]
    fn an_allocation_without_a_now_ref_is_named_not_guessed() {
        let runner = ScriptedRunner::new()
            .on(WORK_POLICY_ACTION, &policy("rev"))
            .on(NOW_ALLOCATE_ACTION, &success(json!({"created": true})));
        let error =
            open_session_now(&runner, Path::new(ROOT), Some("example"), "claude", REF).unwrap_err();
        assert_eq!(error.code(), "central.action_invalid_output");
    }

    fn listing() -> String {
        success(json!({"schema": "central.now-listing/v1", "records": [
            {"now_ref": "central:now:project:example:other", "task_ref": "aikit:session:claude:sess-0",
             "lifecycle": "active", "participant_refs": [REF],
             "revision": {"byte_len": 1, "revision": "rev-other"}},
            {"now_ref": "central:now:project:example:k1", "task_ref": REF,
             "lifecycle": "active", "participant_refs": [REF],
             "revision": {"byte_len": 9, "revision": "rev-now-1"}}
        ]}))
    }

    #[test]
    fn a_session_end_closes_its_own_now_at_the_revision_just_listed() {
        let runner = ScriptedRunner::new()
            .on(NOW_LIST_ACTION, &listing())
            .on(WORK_POLICY_ACTION, &policy("rev-policy-2"))
            .on(
                NOW_LIFECYCLE_ACTION,
                &success(json!({"lifecycle": "closed"})),
            );
        let closed = close_session_now(&runner, Path::new(ROOT), Some("example"), REF).unwrap();
        assert_eq!(closed.as_deref(), Some("central:now:project:example:k1"));
        let calls = runner.calls();
        assert_eq!(calls.len(), 3, "{calls:?}");
        assert_eq!(
            ctrl_call(&calls[0]),
            (
                NOW_LIST_ACTION.into(),
                json!({"project": "example", "participant_refs": [REF]})
            )
        );
        assert_eq!(ctrl_call(&calls[1]).0, WORK_POLICY_ACTION);
        assert_eq!(
            ctrl_call(&calls[2]),
            (
                NOW_LIFECYCLE_ACTION.into(),
                json!({
                    "project": "example",
                    "now_ref": "central:now:project:example:k1",
                    "expected_revision": "rev-now-1",
                    "expected_policy_revision": "rev-policy-2",
                    "lifecycle": "closed",
                })
            )
        );
    }

    #[test]
    fn a_session_holding_no_active_now_closes_nothing() {
        let closed_already = success(json!({"records": [
            {"now_ref": "central:now:project:example:k1", "task_ref": REF, "lifecycle": "closed",
             "participant_refs": [REF], "revision": {"revision": "rev"}}
        ]}));
        let runner = ScriptedRunner::new().on(NOW_LIST_ACTION, &closed_already);
        assert_eq!(
            close_session_now(&runner, Path::new(ROOT), Some("example"), REF).unwrap(),
            None
        );
        assert_eq!(runner.calls().len(), 1, "only the listing is read");
    }

    fn listed_as(lifecycle: &str) -> String {
        success(json!({"records": [
            {"now_ref": "central:now:project:example:k1", "task_ref": REF, "lifecycle": lifecycle,
             "participant_refs": [REF], "revision": {"byte_len": 9, "revision": "rev-closed"}}
        ]}))
    }

    #[test]
    fn a_resumed_session_re_enters_its_closed_now_rather_than_allocating_again() {
        let runner = ScriptedRunner::new()
            .on(NOW_LIST_ACTION, &listed_as("closed"))
            .on(WORK_POLICY_ACTION, &policy("rev-policy-3"))
            .on(
                NOW_LIFECYCLE_ACTION,
                &success(json!({"lifecycle": "active"})),
            )
            .on(NOW_ALLOCATE_ACTION, &allocation());
        let now =
            reopen_session_now(&runner, Path::new(ROOT), Some("example"), "claude", REF).unwrap();
        assert_eq!(
            now,
            SessionNow {
                now_ref: "central:now:project:example:k1".into(),
                scratch: Some(SCRATCH.into()),
            }
        );
        let calls = runner.calls();
        assert_eq!(calls.len(), 4, "{calls:?}");
        assert_eq!(
            ctrl_call(&calls[0]),
            (
                NOW_LIST_ACTION.into(),
                json!({"project": "example", "participant_refs": [REF]})
            )
        );
        assert_eq!(ctrl_call(&calls[1]).0, WORK_POLICY_ACTION);
        // Re-entry keeps the NOW's identity, at the revision just listed…
        assert_eq!(
            ctrl_call(&calls[2]),
            (
                NOW_LIFECYCLE_ACTION.into(),
                json!({
                    "project": "example",
                    "now_ref": "central:now:project:example:k1",
                    "expected_revision": "rev-closed",
                    "expected_policy_revision": "rev-policy-3",
                    "lifecycle": "active",
                })
            )
        );
        // …and the allocation, idempotent again, answers it with its scratch.
        let (id, input) = ctrl_call(&calls[3]);
        assert_eq!(id, NOW_ALLOCATE_ACTION);
        assert_eq!(input["task_ref"], REF);
        assert_eq!(input["expected_policy_revision"], "rev-policy-3");
    }

    #[test]
    fn reopening_an_active_now_or_none_moves_no_lifecycle() {
        for listing in [listed_as("active"), success(json!({"records": []}))] {
            let runner = ScriptedRunner::new()
                .on(NOW_LIST_ACTION, &listing)
                .on(WORK_POLICY_ACTION, &policy("rev"))
                .on(NOW_ALLOCATE_ACTION, &allocation());
            reopen_session_now(&runner, Path::new(ROOT), Some("example"), "claude", REF).unwrap();
            let ids = runner
                .calls()
                .iter()
                .map(|call| ctrl_call(call).0)
                .collect::<Vec<_>>();
            assert_eq!(
                ids,
                [NOW_LIST_ACTION, WORK_POLICY_ACTION, NOW_ALLOCATE_ACTION]
            );
        }
    }

    #[test]
    fn a_refused_re_entry_is_centrals_refusal_and_allocates_nothing() {
        let runner = Refusing {
            inner: ScriptedRunner::new()
                .on(NOW_LIST_ACTION, &listed_as("closed"))
                .on(WORK_POLICY_ACTION, &policy("rev"))
                .on(NOW_ALLOCATE_ACTION, &allocation()),
            action: NOW_LIFECYCLE_ACTION,
            envelope: json!({"ok": false, "status": "denied", "action": NOW_LIFECYCLE_ACTION,
                "error": {"code": "denied", "message": "missing bounded CENTRAL_NATIVE_TOKEN"}})
            .to_string(),
        };
        let error = reopen_session_now(&runner, Path::new(ROOT), Some("example"), "claude", REF)
            .unwrap_err();
        assert!(
            error.message().contains("CENTRAL_NATIVE_TOKEN"),
            "{}",
            error.message()
        );
        assert!(!runner
            .inner
            .call_lines()
            .iter()
            .any(|line| line.contains(NOW_ALLOCATE_ACTION)));
    }

    #[test]
    fn centrals_refusal_reaches_the_caller_in_centrals_own_words() {
        let envelope = json!({"ok": false, "status": "conflict", "action": NOW_LIFECYCLE_ACTION,
            "error": {"code": "source_conflict",
                      "message": "NOW has 1 T fixture(s) not distilled into learnings: T/a.md."}})
        .to_string();
        let runner = Refusing {
            inner: ScriptedRunner::new()
                .on(NOW_LIST_ACTION, &listing())
                .on(WORK_POLICY_ACTION, &policy("rev")),
            action: NOW_LIFECYCLE_ACTION,
            envelope,
        };
        let error = close_session_now(&runner, Path::new(ROOT), Some("example"), REF).unwrap_err();
        assert_eq!(error.code(), "central.action_failed");
        assert!(
            error.message().contains("not distilled into learnings"),
            "{}",
            error.message()
        );
    }

    #[test]
    fn a_failed_exit_without_an_envelope_still_says_the_status() {
        let runner = ScriptedRunner::new().failing(WORK_POLICY_ACTION, 7, "ctrl: broken");
        let error =
            open_session_now(&runner, Path::new(ROOT), Some("example"), "claude", REF).unwrap_err();
        assert!(error.message().contains("exited with status 7"));
    }

    #[test]
    fn the_ground_names_this_sessions_now_and_scratch_place() {
        let ground = CentralTemporalGround {
            project: "example".into(),
            now: json!({"exists": true}),
            day: Some(CentralDayPointer {
                day_ref: "central:day:control:root:2026-11-13".into(),
                revision: None,
            }),
            control_unpushed: None,
            session_now: Some(SessionNow {
                now_ref: "central:now:project:example:k1".into(),
                scratch: Some(SCRATCH.into()),
            }),
            session_now_not_opened: false,
        };
        let rendered = ground.render();
        assert!(rendered.contains("project: example"));
        assert!(rendered.contains("NOW: central:now:project:example:k1 (this session's own)"));
        assert!(rendered.contains(&format!("scratch: {SCRATCH}")));
        assert!(rendered.contains("Day: central:day:control:root:2026-11-13"));
        assert!(!rendered.contains(SESSION_NOW_NOT_OPENED));
    }

    /// O-I #1016: a session whose start opened no NOW, because it could not
    /// close one, hears that once in the NOW's place, in the
    /// Project and in the root, and its ground is otherwise the same.
    #[test]
    fn a_session_whose_now_was_not_opened_hears_that_in_its_place() {
        let day = CentralDayPointer {
            day_ref: "central:day:control:root:2026-11-13".into(),
            revision: None,
        };
        let ground = CentralTemporalGround {
            project: "example".into(),
            now: json!({"exists": true}),
            day: Some(day.clone()),
            control_unpushed: None,
            session_now: None,
            session_now_not_opened: true,
        };
        let rendered = ground.render();
        assert!(rendered.contains("project: example"));
        assert_eq!(rendered.matches(SESSION_NOW_NOT_OPENED).count(), 1);
        assert!(!rendered.contains("(this session's own)"));
        assert!(!rendered.contains("scratch:"));
        assert!(rendered.contains("Day: central:day:control:root:2026-11-13"));

        let root = render_root_ground(None, None, true, Some(&day)).unwrap();
        assert!(root.contains("register: the Central root (no Project)"));
        assert_eq!(root.matches(SESSION_NOW_NOT_OPENED).count(), 1);
        assert!(root.contains("Day: central:day:control:root:2026-11-13"));
    }

    #[test]
    fn a_root_session_with_its_own_now_hears_it_and_the_day() {
        let now = SessionNow {
            now_ref: "central:now:control:root:k2".into(),
            scratch: None,
        };
        let day = CentralDayPointer {
            day_ref: "central:day:control:root:2026-11-13".into(),
            revision: Some("rev-day".into()),
        };
        let rendered = render_root_ground(None, Some(&now), false, Some(&day)).unwrap();
        assert!(rendered.starts_with("[Central temporal ground]"));
        assert!(rendered.contains("NOW: central:now:control:root:k2"));
        assert!(rendered.contains("Day: central:day:control:root:2026-11-13 @ rev-day"));
        assert!(!rendered.contains("project:"), "the root is no Project");
        assert!(!rendered.contains("scratch:"));
        // Without a NOW the root ground is what it was: the warning alone, or nothing.
        assert_eq!(render_root_ground(None, None, false, Some(&day)), None);
        let unpushed = ControlUnpushed {
            ahead: 1,
            oldest: None,
        };
        assert_eq!(
            render_root_ground(Some(&unpushed), None, false, None),
            Some(unpushed.render_standalone())
        );
    }
}
