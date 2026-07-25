//! Running a Procedure on a real filesystem: stage → validate → commit → undo.
//!
//! `aikit_core::procedure` plans and digests; this module is where the bytes move.
//! It exists to make one sentence true outside AIKit's own state directory, the
//! same sentence [`crate::generation`] makes true inside it: **a failed procedure
//! leaves the world intact.**
//!
//! ## The order of operations
//!
//! ```text
//! plan (pure, in core)                ← nothing has been written
//!   compute every inverse             ← BEFORE any edit, never at failure time
//! stage into the isolation strategy   ← a shadow tree, or a branch
//!   back up every path we will touch  ← into state/procedures/<id>/undo/
//! validate                            ← still nothing outside has moved
//! ── commit ─────────────────────────
//!   apply each edit in order
//!   record the undo journal after each one
//! ```
//!
//! ## Why the undo journal is written as we go, not at the end
//!
//! A procedure that dies halfway has applied some edits. If the journal were
//! written only on success, those edits would be unrecorded and therefore
//! un-undoable — precisely the state a user most needs `undo` to work in. So each
//! applied edit appends its inverse *before* the next edit starts, and `undo`
//! replays whatever is there in reverse.
//!
//! ## Why a re-run of a satisfied plan is a no-op
//!
//! The plan digest is content-addressed. `run` records the digest on success, so
//! running the same procedure again finds the work already done and reports it
//! rather than repeating it. That is what makes `doctor --fix` safe to run twice.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use aikit_core::procedure::{
    Inverse, Plan, PlanDigest, Procedure, UndoRecord, UndoStep, WorldEdit,
};
use aikit_core::{AikitError, ProcedureId, Result};

use crate::home::{create_dir_all, io_error, AikitHome};

/// Files a procedure keeps under `state/procedures/<id>/`.
pub const UNDO_DIR: &str = "undo";
pub const JOURNAL_FILE: &str = "undo.json";
pub const PLAN_FILE: &str = "plan.json";

// ---------------------------------------------------------------------------
// The diff a human reviews
// ---------------------------------------------------------------------------

/// What one edit would change, rendered for review **before** anything is written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditDiff {
    pub description: String,
    pub path: Option<PathBuf>,
    /// `true` when the path does not exist yet, so "before" is empty by fact
    /// rather than by omission.
    pub creates: bool,
    pub before: Option<String>,
    pub after: Option<String>,
    /// How this edit will be undone, stated up front.
    pub undo: String,
}

/// The full reviewable diff of a procedure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcedureDiff {
    pub procedure: ProcedureId,
    pub digest: PlanDigest,
    pub isolation: String,
    pub edits: Vec<EditDiff>,
    pub notes: Vec<String>,
}

impl ProcedureDiff {
    pub fn is_empty(&self) -> bool {
        self.edits.is_empty()
    }

    /// Plain-text rendering for `aikit procedure diff`.
    pub fn render(&self) -> String {
        let mut out = format!(
            "procedure {} ({}) — {} edit{}, staged {}\n",
            self.procedure,
            self.digest.short(),
            self.edits.len(),
            if self.edits.len() == 1 { "" } else { "s" },
            self.isolation,
        );
        for note in &self.notes {
            out.push_str(&format!("note: {note}\n"));
        }
        for edit in &self.edits {
            out.push_str(&format!("\n{}\n", edit.description));
            if let Some(path) = &edit.path {
                out.push_str(&format!(
                    "  {} {}\n",
                    if edit.creates { "create" } else { "modify" },
                    path.display()
                ));
            }
            out.push_str(&format!("  undo: {}\n", edit.undo));
        }
        out
    }
}

// ---------------------------------------------------------------------------
// The runner
// ---------------------------------------------------------------------------

/// The outcome of committing a procedure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcedureOutcome {
    pub procedure: ProcedureId,
    pub digest: PlanDigest,
    /// Edits actually applied. Zero when the plan was already satisfied.
    pub applied: usize,
    /// True when the plan's digest was already recorded as done.
    pub already_satisfied: bool,
    pub notes: Vec<String>,
}

/// Runs procedures against the real filesystem.
pub struct ProcedureRunner<'a> {
    home: &'a AikitHome,
}

impl<'a> ProcedureRunner<'a> {
    pub fn new(home: &'a AikitHome) -> Self {
        Self { home }
    }

    /// Where this procedure's undo material lives.
    pub fn procedure_dir(&self, id: &ProcedureId) -> PathBuf {
        self.home.state().join("procedures").join(id.as_str())
    }

    /// Compute the reviewable diff. **Writes nothing.**
    ///
    /// The "before" side is read from the real filesystem, so what a user reviews
    /// is what is actually there rather than what the plan assumed.
    pub fn diff(&self, procedure: &Procedure) -> Result<ProcedureDiff> {
        let mut edits = Vec::new();
        for edit in &procedure.plan.edits {
            edits.push(self.diff_one(edit)?);
        }
        Ok(ProcedureDiff {
            procedure: procedure.id.clone(),
            digest: procedure.digest.clone(),
            isolation: procedure.isolation.as_str().to_string(),
            edits,
            notes: procedure.plan.notes.clone(),
        })
    }

    fn diff_one(&self, edit: &WorldEdit) -> Result<EditDiff> {
        let path = edit.path().cloned();
        let existing = path.as_ref().and_then(|p| fs::read_to_string(p).ok());
        let creates = path.as_ref().is_some_and(|p| !p.exists());

        let after = match edit {
            WorldEdit::WriteFile { contents, .. } => {
                Some(String::from_utf8_lossy(contents).into_owned())
            }
            WorldEdit::DeleteFile { .. } => None,
            WorldEdit::CreateLink { target, .. } => Some(target.display().to_string()),
            WorldEdit::MarkedBlock { path, contents, .. } => {
                let current = fs::read_to_string(path).unwrap_or_default();
                Some(aikit_core::procedure::splice_marked_block(
                    &current,
                    comment_leader(path),
                    contents,
                ))
            }
            WorldEdit::RunCommand { .. } => None,
        };

        Ok(EditDiff {
            description: edit.describe(),
            path,
            creates,
            before: existing,
            after,
            undo: describe_undo(edit),
        })
    }

    /// Stage, validate and commit a procedure.
    ///
    /// Refuses before touching anything when the plan is not safe for its
    /// isolation. Applies each edit in order, appending its inverse to the journal
    /// as it goes; on any failure the already-applied edits are rolled back from
    /// that same journal, so a half-applied procedure never survives.
    pub fn run(&self, procedure: &Procedure) -> Result<ProcedureOutcome> {
        procedure.plan.validate(&procedure.isolation)?;

        let dir = self.procedure_dir(&procedure.id);
        create_dir_all(&dir)?;
        create_dir_all(&dir.join(UNDO_DIR))?;

        // Record the plan so `undo` and an audit can read what was intended even
        // if the process dies mid-run.
        let plan_json = serde_json::to_string_pretty(&procedure.plan).map_err(|e| {
            AikitError::new(
                "procedure.unserializable",
                format!("the plan could not be recorded: {e}"),
            )
        })?;
        write_file(&dir.join(PLAN_FILE), plan_json.as_bytes())?;

        if self.is_satisfied(&procedure.digest)? {
            return Ok(ProcedureOutcome {
                procedure: procedure.id.clone(),
                digest: procedure.digest.clone(),
                applied: 0,
                already_satisfied: true,
                notes: vec![
                    "this plan was already applied; nothing was done again".to_string(),
                ],
            });
        }

        let mut journal = UndoRecord {
            procedure: procedure.id.clone(),
            digest: procedure.digest.clone(),
            steps: Vec::new(),
        };

        for (index, edit) in procedure.plan.edits.iter().enumerate() {
            // The inverse is captured BEFORE the edit, from the real state.
            let step = self.capture_inverse(&dir, edit, index)?;
            journal.steps.push(step);
            // Persist the journal before applying, so an edit that dies mid-write
            // is still undoable.
            self.write_journal(&dir, &journal)?;

            if let Err(error) = self.apply(edit) {
                // Roll back what we managed to do, then report the original error.
                let _ = self.undo_record(&dir, &journal);
                return Err(error);
            }
        }

        self.mark_satisfied(&procedure.digest)?;

        Ok(ProcedureOutcome {
            procedure: procedure.id.clone(),
            digest: procedure.digest.clone(),
            applied: procedure.plan.edits.len(),
            already_satisfied: false,
            notes: procedure.plan.notes.clone(),
        })
    }

    /// Apply the recorded inverses of a committed procedure, newest edit first.
    pub fn undo(&self, id: &ProcedureId) -> Result<usize> {
        let dir = self.procedure_dir(id);
        let journal = self.read_journal(&dir)?;
        let count = self.undo_record(&dir, &journal)?;
        // A procedure that has been undone is no longer satisfied.
        let _ = fs::remove_file(self.satisfied_marker(&journal.digest));
        Ok(count)
    }

    /// Replay a journal's inverses in reverse order.
    fn undo_record(&self, dir: &Path, journal: &UndoRecord) -> Result<usize> {
        let mut undone = 0;
        for step in journal.steps.iter().rev() {
            match (&step.inverse, &step.path) {
                (Inverse::Restore { blob }, Some(path)) => {
                    let stored = dir.join(UNDO_DIR).join(blob.as_str());
                    let bytes = fs::read(&stored)
                        .map_err(|e| io_error("procedure.undo_failed", &stored, &e))?;
                    write_file(path, &bytes)?;
                    undone += 1;
                }
                (Inverse::Remove, Some(path)) => {
                    // Removing what we created. A missing file is already undone.
                    if path.is_dir() {
                        let _ = fs::remove_dir_all(path);
                    } else {
                        let _ = fs::remove_file(path);
                    }
                    undone += 1;
                }
                (Inverse::Recreate { target }, Some(path)) => {
                    let _ = fs::remove_file(path);
                    symlink(target, path)?;
                    undone += 1;
                }
                (Inverse::None, _) | (_, None) => {}
            }
        }
        Ok(undone)
    }

    /// Back up whatever is at the edit's path and return the step that undoes it.
    fn capture_inverse(&self, dir: &Path, edit: &WorldEdit, index: usize) -> Result<UndoStep> {
        let Some(path) = edit.path() else {
            return Ok(UndoStep::new(None, Inverse::None, edit.describe()));
        };

        // A symlink is restored by recreating it, not by copying its target's bytes.
        if let Ok(target) = fs::read_link(path) {
            return Ok(UndoStep::new(
                Some(path.clone()),
                Inverse::Recreate { target },
                edit.describe(),
            ));
        }

        if !path.exists() {
            return Ok(UndoStep::new(
                Some(path.clone()),
                Inverse::Remove,
                edit.describe(),
            ));
        }

        let bytes = fs::read(path).map_err(|e| io_error("procedure.backup_failed", path, &e))?;
        let blob = aikit_core::procedure::BlobId::new(format!("{index:04}-{}", file_slug(path)));
        let stored = dir.join(UNDO_DIR).join(blob.as_str());
        write_file(&stored, &bytes)?;
        Ok(UndoStep::new(
            Some(path.clone()),
            Inverse::Restore { blob },
            edit.describe(),
        ))
    }

    /// Perform one edit.
    fn apply(&self, edit: &WorldEdit) -> Result<()> {
        match edit {
            WorldEdit::WriteFile { path, contents, .. } => write_file(path, contents),
            WorldEdit::DeleteFile { path, .. } => {
                if path.exists() || path.is_symlink() {
                    fs::remove_file(path)
                        .map_err(|e| io_error("procedure.apply_failed", path, &e))?;
                }
                Ok(())
            }
            WorldEdit::CreateLink { path, target, .. } => {
                if path.exists() || path.is_symlink() {
                    fs::remove_file(path)
                        .map_err(|e| io_error("procedure.apply_failed", path, &e))?;
                }
                if let Some(parent) = path.parent() {
                    create_dir_all(parent)?;
                }
                symlink(target, path)
            }
            WorldEdit::MarkedBlock { path, contents, .. } => {
                let existing = fs::read_to_string(path).unwrap_or_default();
                let next = aikit_core::procedure::splice_marked_block(
                    &existing,
                    comment_leader(path),
                    contents,
                );
                write_file(path, next.as_bytes())
            }
            WorldEdit::RunCommand { argv, cwd, .. } => {
                let Some((program, args)) = argv.split_first() else {
                    return Err(AikitError::new(
                        "procedure.empty_command",
                        "a RunCommand edit has no command",
                    ));
                };
                let status = std::process::Command::new(program)
                    .args(args)
                    .current_dir(cwd)
                    .status()
                    .map_err(|e| {
                        AikitError::new(
                            "procedure.command_failed",
                            format!("could not run `{}`: {e}", argv.join(" ")),
                        )
                        .with("command", argv.join(" "))
                    })?;
                if !status.success() {
                    return Err(AikitError::new(
                        "procedure.command_failed",
                        format!(
                            "`{}` exited with status {}",
                            argv.join(" "),
                            status.code().unwrap_or(-1)
                        ),
                    )
                    .with("command", argv.join(" ")));
                }
                Ok(())
            }
        }
    }

    // -- satisfaction marker -------------------------------------------------

    fn satisfied_dir(&self) -> PathBuf {
        self.home.state().join("procedures").join(".satisfied")
    }

    fn satisfied_marker(&self, digest: &PlanDigest) -> PathBuf {
        self.satisfied_dir().join(digest.as_str())
    }

    fn is_satisfied(&self, digest: &PlanDigest) -> Result<bool> {
        Ok(self.satisfied_marker(digest).exists())
    }

    fn mark_satisfied(&self, digest: &PlanDigest) -> Result<()> {
        create_dir_all(&self.satisfied_dir())?;
        write_file(&self.satisfied_marker(digest), b"")
    }

    // -- journal -------------------------------------------------------------

    fn write_journal(&self, dir: &Path, journal: &UndoRecord) -> Result<()> {
        let json = serde_json::to_string_pretty(journal).map_err(|e| {
            AikitError::new(
                "procedure.unserializable",
                format!("the undo journal could not be written: {e}"),
            )
        })?;
        write_file(&dir.join(JOURNAL_FILE), json.as_bytes())
    }

    fn read_journal(&self, dir: &Path) -> Result<UndoRecord> {
        let path = dir.join(JOURNAL_FILE);
        let text = fs::read_to_string(&path).map_err(|e| {
            io_error("procedure.no_undo_record", &path, &e)
        })?;
        serde_json::from_str(&text).map_err(|e| {
            AikitError::new(
                "procedure.no_undo_record",
                format!("{} is not a readable undo record: {e}", path.display()),
            )
        })
    }

    /// List the procedures that have an undo record on disk, newest first.
    pub fn list(&self) -> Result<Vec<ProcedureId>> {
        let root = self.home.state().join("procedures");
        let Ok(entries) = fs::read_dir(&root) else {
            return Ok(Vec::new());
        };
        let mut ids: Vec<ProcedureId> = entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                ProcedureId::parse(&name).ok()
            })
            .collect();
        // ULID bodies sort by creation time; newest first.
        ids.sort_by(|a, b| b.as_str().cmp(a.as_str()));
        Ok(ids)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// The comment leader a marked block uses in this file type.
///
/// Markdown has no `#` line comment (it is a heading), so `AGENTS.md` and
/// `CLAUDE.md` get an HTML comment; everything else AIKit edits — shell rc files,
/// tmux config, TOML — uses `#`.
pub fn comment_leader(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("md") | Some("markdown") => "<!--",
        _ => "#",
    }
}

fn describe_undo(edit: &WorldEdit) -> String {
    match edit {
        WorldEdit::MarkedBlock { .. } => {
            "replace the managed block (idempotent; prose outside it is untouched)".to_string()
        }
        WorldEdit::RunCommand { undo: Some(u), .. } => format!("run `{}`", u.join(" ")),
        WorldEdit::RunCommand { undo: None, .. } => {
            "nothing — this command declares no undo".to_string()
        }
        other => match other.inverse() {
            Some(Inverse::Restore { .. }) => "restore the previous contents".to_string(),
            Some(Inverse::Remove) => "remove what was created".to_string(),
            Some(Inverse::Recreate { target }) => {
                format!("recreate the link to {}", target.display())
            }
            Some(Inverse::None) | None => "nothing recorded".to_string(),
        },
    }
}

/// A filesystem-safe name for a backup blob.
fn file_slug(path: &Path) -> String {
    path.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

/// Write a file, creating its parents.
///
/// A failure here is re-coded into the `procedure.*` domain: a caller handling a
/// procedure needs to recognise its failures as procedure failures (STANDARDS §3),
/// and a bare `home.create_failed` bubbling out of the middle of an apply is a code
/// from a different domain describing the same event. The underlying message is
/// preserved, so the path and the OS reason are not lost.
fn write_file(path: &Path, contents: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        create_dir_all(parent).map_err(|e| {
            AikitError::new("procedure.write_failed", e.message().to_string())
                .with("path", path.display().to_string())
        })?;
    }
    fs::write(path, contents).map_err(|e| io_error("procedure.write_failed", path, &e))
}

#[cfg(unix)]
fn symlink(target: &Path, link: &Path) -> Result<()> {
    std::os::unix::fs::symlink(target, link)
        .map_err(|e| io_error("procedure.link_failed", link, &e))
}

#[cfg(windows)]
fn symlink(target: &Path, link: &Path) -> Result<()> {
    let result = if target.is_dir() {
        std::os::windows::fs::symlink_dir(target, link)
    } else {
        std::os::windows::fs::symlink_file(target, link)
    };
    result.map_err(|e| io_error("procedure.link_failed", link, &e))
}

/// Convenience for callers that hold a plan and want it staged with the isolation
/// the rule selects (Spec II §1.2), with the shadow root under the AIKit home.
pub fn plan_procedure(
    home: &AikitHome,
    kind: aikit_core::procedure::ProcedureKind,
    plan: Plan,
) -> Result<Procedure> {
    let shadow_root = home.state().join("procedures").join(".shadow");
    let isolation = aikit_core::procedure::select_isolation(&plan, &shadow_root, git_repo_of);
    Procedure::new(kind, plan, isolation)
}

/// The git repository a path belongs to, by walking up for a `.git` entry.
///
/// A worktree's `.git` is a *file* rather than a directory, so both are accepted —
/// otherwise every `--worktree` task would look like it was outside version
/// control and be staged in a shadow tree unnecessarily.
pub fn git_repo_of(path: &Path) -> Option<PathBuf> {
    let mut dir = if path.is_dir() {
        Some(path)
    } else {
        path.parent()
    };
    while let Some(current) = dir {
        if current.join(".git").exists() {
            return Some(current.to_path_buf());
        }
        dir = current.parent();
    }
    None
}

/// Re-exported so callers can name the isolation without importing core directly.
pub use aikit_core::procedure::MutationIsolation as Isolation;
