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

use std::path::{Component, Path};

use aikit_core::{AikitError, Result};
use serde_json::{json, Value};

use crate::runner::{CommandRunner, Output};

pub const NOW_INSPECT_ACTION: &str = "projectcentral.now.inspect";
pub const DAY_READ_ACTION: &str = "central.day.read";
pub const CENTRAL_TEMPORAL_PROVIDER_VERSION: &str = "aikit.central-temporal/v1";

const MAX_ACTIVE_ITEMS: usize = 8;
const MAX_HUMAN_REFS: usize = 8;
const MAX_DAY_REFS: usize = 3;
const MAX_ITEM_RESULT_CHARS: usize = 600;
const MAX_RENDERED_CHARS: usize = 9_000;

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
    /// Project (the Central root, Control itself): it has no NOW to report, so
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
    pub now: Value,
    pub day: Option<CentralDayPointer>,
    /// Control is ahead of its remote. `None` when it is in step or unreadable.
    pub control_unpushed: Option<ControlUnpushed>,
}

impl CentralTemporalGround {
    /// Render an orientation fragment, not an authority or activation grant.
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

        if let Some(items) = self.now.get("active_items").and_then(Value::as_array) {
            for item in items.iter().take(MAX_ACTIVE_ITEMS) {
                let id = item.get("id").and_then(Value::as_str).unwrap_or("unknown");
                let kind = item.get("kind").and_then(Value::as_str).unwrap_or("item");
                let actor = item
                    .get("actor")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                let status = item
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                let subject = item.get("subject").and_then(Value::as_str).unwrap_or("");
                let result = item
                    .get("result")
                    .and_then(Value::as_str)
                    .map(|text| bounded(text, MAX_ITEM_RESULT_CHARS))
                    .unwrap_or_default();
                lines.push(format!(
                    "NOW {kind} {id} [{status}] by {actor}: {subject}{}",
                    if result.is_empty() {
                        String::new()
                    } else {
                        format!(" — {result}")
                    }
                ));
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

        lines.push(match &self.day {
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
        });

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
    // The Day is a pointer, and optional: a world with no open Day (or an
    // older Central) still gets its NOW ground.
    let day = action(runner, central_root, DAY_READ_ACTION, json!({}))
        .ok()
        .and_then(|day| {
            Some(CentralDayPointer {
                day_ref: day.get("day_ref")?.as_str()?.to_owned(),
                revision: day
                    .get("revision")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            })
        });

    let control_unpushed = read_control_unpushed(runner, central_root);

    Ok(Some(CentralTemporalGround {
        project,
        now,
        day,
        control_unpushed,
    }))
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
    if !output.ok() {
        return Err(AikitError::new(
            "central.action_failed",
            format!("Central Action {id} exited with status {}", output.status),
        )
        .with("action", id)
        .with("command", argv.join(" "))
        .with("stderr", output.stderr.trim().to_owned()));
    }
    let result: Value = serde_json::from_str(output.stdout.trim()).map_err(|error| {
        AikitError::new(
            "central.action_invalid_output",
            format!("Central Action {id} returned invalid JSON: {error}"),
        )
        .with("action", id)
    })?;
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

fn project_member(central_root: &Path, project_root: &Path) -> Option<String> {
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
        };
        let rendered = ground.render();
        assert!(rendered.chars().count() <= MAX_RENDERED_CHARS);
        assert!(rendered.contains("does not grant authority"));
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
        };
        let rendered = ground.render();
        assert!(rendered.chars().count() <= MAX_RENDERED_CHARS);
        let warning_at = rendered.find("WARNING: Control is 6 commits").unwrap();
        assert!(warning_at < rendered.find("NOW: present").unwrap());
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
}
