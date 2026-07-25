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

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use aikit_core::procedure::{
    Inverse, Plan, PlanDigest, Procedure, UndoRecord, UndoStep, WorldEdit, WorldPrecondition,
};
use aikit_core::{AikitError, ProcedureId, Result};

use crate::home::{create_dir_all, io_error, AikitHome};

/// Files a procedure keeps under `state/procedures/<id>/`.
pub const UNDO_DIR: &str = "undo";
pub const JOURNAL_FILE: &str = "undo.json";
pub const PLAN_FILE: &str = "plan.json";
pub const PROCEDURE_FILE: &str = "procedure.json";
pub const GIT_FILE: &str = "git.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GitCommitRecord {
    repo: PathBuf,
    branch: String,
    commit: String,
}

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
            if let Some(before) = &edit.before {
                out.push_str("  before:\n");
                render_indented(&mut out, before);
            } else if edit.creates {
                out.push_str("  before: <absent>\n");
            }
            if let Some(after) = &edit.after {
                out.push_str("  after:\n");
                render_indented(&mut out, after);
            } else if edit.path.is_some() {
                out.push_str("  after: <absent>\n");
            }
            out.push_str(&format!("  undo: {}\n", edit.undo));
        }
        out
    }
}

fn render_indented(out: &mut String, contents: &str) {
    if contents.is_empty() {
        out.push_str("    <empty>\n");
        return;
    }
    for line in contents.split_inclusive('\n') {
        out.push_str("    ");
        out.push_str(line);
        if !line.ends_with('\n') {
            out.push('\n');
        }
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

    /// Persist an immutable planned Procedure before it is applied.
    pub fn save(&self, procedure: &Procedure) -> Result<()> {
        let dir = self.procedure_dir(&procedure.id);
        create_dir_all(&dir)?;
        let metadata_path = dir.join(PROCEDURE_FILE);
        if metadata_path.exists() {
            let existing: Procedure =
                serde_json::from_slice(&fs::read(&metadata_path).map_err(|error| {
                    io_error("procedure.invalid_record", &metadata_path, &error)
                })?)
                .map_err(|error| {
                    AikitError::new(
                        "procedure.invalid_record",
                        format!(
                            "{} is not valid Procedure metadata: {error}",
                            metadata_path.display()
                        ),
                    )
                })?;
            let existing_plan = self.read_plan(&dir)?;
            if existing != *procedure || existing_plan != procedure.plan {
                return Err(AikitError::new(
                    "procedure.id_collision",
                    format!(
                        "Procedure id {} is already bound to a different immutable plan",
                        procedure.id
                    ),
                ));
            }
            return Ok(());
        }
        let plan_json = serde_json::to_string_pretty(&procedure.plan).map_err(|error| {
            AikitError::new(
                "procedure.unserializable",
                format!("the plan could not be recorded: {error}"),
            )
        })?;
        let procedure_json = serde_json::to_string_pretty(procedure).map_err(|error| {
            AikitError::new(
                "procedure.unserializable",
                format!("the Procedure metadata could not be recorded: {error}"),
            )
        })?;
        write_file(&dir.join(PLAN_FILE), plan_json.as_bytes())?;
        write_file(&metadata_path, procedure_json.as_bytes())
    }

    /// Load and structurally validate one durable planned Procedure.
    pub fn load(&self, id: &ProcedureId) -> Result<Procedure> {
        let dir = self.procedure_dir(id);
        let path = dir.join(PROCEDURE_FILE);
        let bytes =
            fs::read(&path).map_err(|error| io_error("procedure.not_found", &path, &error))?;
        let procedure: Procedure = serde_json::from_slice(&bytes).map_err(|error| {
            AikitError::new(
                "procedure.invalid_record",
                format!(
                    "{} is not valid Procedure metadata: {error}",
                    path.display()
                ),
            )
        })?;
        let recorded_plan = self.read_plan(&dir)?;
        if procedure.id != *id
            || procedure.plan != recorded_plan
            || procedure.digest != recorded_plan.digest()
        {
            return Err(AikitError::new(
                "procedure.invalid_record",
                format!(
                    "{} does not match its recorded plan and digest",
                    path.display()
                ),
            )
            .with("procedure", id.to_string()));
        }
        Ok(procedure)
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
            WorldEdit::CreateDir { .. } => Some("<directory>".to_string()),
            WorldEdit::MovePath { to, .. } => Some(format!("moved to {}", to.display())),
            WorldEdit::WriteFile { contents, .. } | WorldEdit::WriteFileMode { contents, .. } => {
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
        self.save(procedure)?;
        create_dir_all(&dir.join(UNDO_DIR))?;

        if self.is_satisfied(&procedure.digest)? {
            verify_satisfied_result(&procedure.plan)?;
            return Ok(ProcedureOutcome {
                procedure: procedure.id.clone(),
                digest: procedure.digest.clone(),
                applied: 0,
                already_satisfied: true,
                notes: vec!["this plan was already applied; nothing was done again".to_string()],
            });
        }

        self.stage_and_validate(procedure, &dir)?;
        let git_branch = self.begin_git_branch(procedure)?;

        let mut journal = UndoRecord {
            procedure: procedure.id.clone(),
            digest: procedure.digest.clone(),
            steps: Vec::new(),
        };
        let preconditions: BTreeMap<&Path, &WorldPrecondition> = procedure
            .plan
            .preconditions
            .iter()
            .map(|precondition| (precondition.path().as_path(), precondition))
            .collect();
        let mut verified = BTreeSet::new();
        let edit_paths: BTreeSet<PathBuf> = procedure.plan.touched_paths().into_iter().collect();
        for precondition in &procedure.plan.preconditions {
            if !edit_paths.contains(precondition.path()) {
                if let Err(error) = verify_forward_precondition(precondition) {
                    if let Err(rollback) =
                        self.rollback_uncommitted(&dir, &journal, git_branch.as_ref())
                    {
                        return Err(rollback_failure(procedure, &dir, &error, &rollback));
                    }
                    return Err(error);
                }
                verified.insert(precondition.path().clone());
            }
        }

        for (index, edit) in procedure.plan.edits.iter().enumerate() {
            for path in edit.paths() {
                if verified.insert(path.clone()) {
                    if let Some(precondition) = preconditions.get(path.as_path()) {
                        if let Err(error) = verify_forward_precondition(precondition) {
                            if let Err(rollback) =
                                self.rollback_uncommitted(&dir, &journal, git_branch.as_ref())
                            {
                                return Err(rollback_failure(procedure, &dir, &error, &rollback));
                            }
                            return Err(error);
                        }
                    }
                }
            }
            // The inverse is captured BEFORE the edit, from the real state.
            let step = match self.capture_inverse(&dir, edit, index) {
                Ok(step) => step,
                Err(error) => {
                    if let Err(rollback) =
                        self.rollback_uncommitted(&dir, &journal, git_branch.as_ref())
                    {
                        return Err(rollback_failure(procedure, &dir, &error, &rollback));
                    }
                    return Err(error);
                }
            };
            journal.steps.push(step);
            // Persist the journal before applying, so an edit that dies mid-write
            // is still undoable.
            if let Err(error) = self.write_journal(&dir, &journal) {
                // This edit has not started. Its inverse must not run (especially
                // for RunCommand, whose undo command would otherwise run alone).
                journal.steps.pop();
                if let Err(rollback) =
                    self.rollback_uncommitted(&dir, &journal, git_branch.as_ref())
                {
                    return Err(rollback_failure(procedure, &dir, &error, &rollback));
                }
                return Err(error);
            }

            if let Err(error) = self.apply(edit) {
                // Roll back what we managed to do, then report the original error.
                if let Err(rollback) = self.undo_record(&dir, &journal) {
                    let combined = AikitError::new(
                        "procedure.rollback_failed",
                        format!(
                            "the Procedure failed ({}) and rollback also failed ({}); inspect {}",
                            error.message(),
                            rollback.message(),
                            dir.display()
                        ),
                    )
                    .with("original_code", error.code())
                    .with("rollback_code", rollback.code())
                    .with("procedure", procedure.id.to_string());
                    if let Some((repo, original, branch)) = &git_branch {
                        let _ = cleanup_failed_branch(repo, original, branch);
                    }
                    return Err(combined);
                }
                if let Some((repo, original, branch)) = &git_branch {
                    cleanup_failed_branch(repo, original, branch)?;
                }
                discard_journal(&dir);
                return Err(error);
            }
        }

        if let Some((repo, original, branch)) = &git_branch {
            match self.commit_git_branch(repo, branch, procedure) {
                Err(error) => {
                    if let Err(rollback) = self.undo_record(&dir, &journal) {
                        return Err(AikitError::new(
                            "procedure.rollback_failed",
                            format!(
                                "git commit failed ({}) and rollback failed ({}); inspect {}",
                                error.message(),
                                rollback.message(),
                                dir.display()
                            ),
                        ));
                    }
                    self.unstage_git_paths(repo, procedure)?;
                    ensure_git_clean(repo)?;
                    cleanup_failed_branch(repo, original, branch)?;
                    discard_journal(&dir);
                    return Err(error);
                }
                Ok(record) => {
                    if let Err(error) = self.write_git_record(&dir, &record) {
                        // The commit exists and the working tree is clean. Do not
                        // replay byte inverses over that commit: switch back to
                        // the untouched original branch and drop the isolated
                        // commit as a whole.
                        if let Err(rollback) = ensure_git_clean(repo)
                            .and_then(|()| cleanup_failed_branch(repo, original, branch))
                        {
                            return Err(rollback_failure(procedure, &dir, &error, &rollback));
                        }
                        discard_journal(&dir);
                        return Err(error);
                    }
                }
            }
        }
        if let Err(error) = self.mark_satisfied(&procedure.digest) {
            let rollback = if let Some((repo, original, branch)) = &git_branch {
                ensure_git_clean(repo)
                    .and_then(|()| cleanup_failed_branch(repo, original, branch))
                    .and_then(|()| {
                        let git_record = dir.join(GIT_FILE);
                        if git_record.exists() {
                            fs::remove_file(&git_record).map_err(|remove_error| {
                                io_error("procedure.rollback_failed", &git_record, &remove_error)
                            })?;
                        }
                        Ok(())
                    })
            } else {
                self.undo_record(&dir, &journal).map(|_| ())
            };
            if let Err(rollback) = rollback {
                return Err(AikitError::new(
                    "procedure.rollback_failed",
                    format!(
                        "the Procedure was applied but its satisfaction record failed ({}), and rollback also failed ({}); inspect {}",
                        error.message(),
                        rollback.message(),
                        dir.display()
                    ),
                )
                .with("original_code", error.code())
                .with("rollback_code", rollback.code())
                .with("procedure", procedure.id.to_string()));
            }
            discard_journal(&dir);
            return Err(error);
        }

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
        let plan = self.read_plan(&dir)?;
        self.verify_undo_preconditions(&dir, &plan, &journal)?;
        let git_path = dir.join(GIT_FILE);
        let count = if git_path.is_file() {
            let record: GitCommitRecord = serde_json::from_str(
                &fs::read_to_string(&git_path)
                    .map_err(|error| io_error("procedure.undo_failed", &git_path, &error))?,
            )
            .map_err(|error| {
                AikitError::new(
                    "procedure.undo_failed",
                    format!("{} is not valid git metadata: {error}", git_path.display()),
                )
            })?;
            ensure_git_clean(&record.repo)?;
            let current_branch = git_output(
                &record.repo,
                &["branch", "--show-current"],
                "procedure.undo_failed",
            )?;
            let current_head = git_output(
                &record.repo,
                &["rev-parse", "HEAD"],
                "procedure.undo_failed",
            )?;
            if current_branch.trim() != record.branch || current_head.trim() != record.commit {
                return Err(AikitError::new(
                    "procedure.undo_drift",
                    "the Git branch has advanced or changed since this Procedure; refusing to revert newer committed work",
                )
                .with("expected_branch", record.branch)
                .with("actual_branch", current_branch.trim())
                .with("expected_head", record.commit)
                .with("actual_head", current_head.trim()));
            }
            if let Err(error) = git(
                &record.repo,
                &[
                    "-c",
                    "user.name=AIKit",
                    "-c",
                    "user.email=aikit@localhost",
                    "revert",
                    "--no-edit",
                    &record.commit,
                ],
                "procedure.undo_failed",
            ) {
                let abort = git(
                    &record.repo,
                    &["revert", "--abort"],
                    "procedure.undo_failed",
                );
                let clean = ensure_git_clean(&record.repo);
                if abort.is_err() || clean.is_err() {
                    return Err(AikitError::new(
                        "procedure.rollback_failed",
                        format!(
                            "Git undo failed ({}) and the revert could not be cleanly aborted",
                            error.message()
                        ),
                    )
                    .with("repo", record.repo.display().to_string()));
                }
                return Err(error);
            }
            journal.steps.len()
        } else {
            self.undo_record(&dir, &journal)?
        };
        // A procedure that has been undone is no longer satisfied.
        let _ = fs::remove_file(self.satisfied_marker(&journal.digest));
        Ok(count)
    }

    fn stage_and_validate(&self, procedure: &Procedure, dir: &Path) -> Result<()> {
        let shadow = match &procedure.isolation {
            aikit_core::procedure::MutationIsolation::Direct => return Ok(()),
            aikit_core::procedure::MutationIsolation::Staged { shadow } => shadow.clone(),
            aikit_core::procedure::MutationIsolation::GitBranch { .. } => dir.join("stage"),
            aikit_core::procedure::MutationIsolation::GitWorktree { .. } => {
                return Err(AikitError::new(
                    "procedure.isolation_unsupported",
                    "git-worktree isolation is not implemented; refusing to imply that the plan was isolated",
                ));
            }
        };
        let allowed = self.home.state().join("procedures");
        if !shadow.starts_with(&allowed) {
            return Err(AikitError::new(
                "procedure.invalid_shadow",
                format!("refusing a shadow outside {}", allowed.display()),
            )
            .with("shadow", shadow.display().to_string()));
        }
        if shadow.exists() {
            fs::remove_dir_all(&shadow)
                .map_err(|error| io_error("procedure.stage_failed", &shadow, &error))?;
        }
        create_dir_all(&shadow)?;
        for (index, edit) in procedure.plan.edits.iter().enumerate() {
            let staged = shadow.join(format!("{index:04}"));
            match edit {
                WorldEdit::CreateDir { .. } => write_file(&staged, b"create-directory")?,
                WorldEdit::MovePath { to, .. } => {
                    write_file(&staged, format!("move to {}", to.display()).as_bytes())?
                }
                WorldEdit::WriteFile { contents, .. } => write_file(&staged, contents)?,
                WorldEdit::WriteFileMode { contents, mode, .. } => {
                    write_file(&staged, contents)?;
                    set_file_mode(&staged, *mode)?;
                }
                WorldEdit::CreateLink { target, .. } => symlink(target, &staged)?,
                WorldEdit::DeleteFile { .. } => write_file(&staged, b"delete")?,
                WorldEdit::MarkedBlock { path, contents, .. } => {
                    let current = fs::read_to_string(path).unwrap_or_default();
                    let next = aikit_core::procedure::splice_marked_block(
                        &current,
                        comment_leader(path),
                        contents,
                    );
                    write_file(&staged, next.as_bytes())?;
                }
                WorldEdit::RunCommand { .. } => {
                    return Err(AikitError::new(
                        "procedure.command_not_staged",
                        "commands must use explicitly confirmed Direct isolation",
                    ));
                }
            }
        }
        Ok(())
    }

    fn begin_git_branch(&self, procedure: &Procedure) -> Result<Option<(PathBuf, String, String)>> {
        let aikit_core::procedure::MutationIsolation::GitBranch { repo, branch } =
            &procedure.isolation
        else {
            return Ok(None);
        };
        ensure_git_clean(repo)?;
        let original = git_output(
            repo,
            &["symbolic-ref", "--quiet", "--short", "HEAD"],
            "procedure.git_setup_failed",
        )?;
        git(
            repo,
            &["switch", "-c", branch],
            "procedure.git_setup_failed",
        )?;
        Ok(Some((
            repo.clone(),
            original.trim().to_string(),
            branch.clone(),
        )))
    }

    fn commit_git_branch(
        &self,
        repo: &Path,
        branch: &str,
        procedure: &Procedure,
    ) -> Result<GitCommitRecord> {
        let relative = git_relative_paths(repo, procedure)?;
        let mut add = vec!["add".to_string(), "--".to_string()];
        add.extend(relative);
        git_owned(repo, &add, "procedure.git_commit_failed")?;
        git(
            repo,
            &[
                "-c",
                "user.name=AIKit",
                "-c",
                "user.email=aikit@localhost",
                "commit",
                "-m",
                &format!("aikit procedure {}", procedure.id),
            ],
            "procedure.git_commit_failed",
        )?;
        let commit = git_output(repo, &["rev-parse", "HEAD"], "procedure.git_commit_failed")?;
        Ok(GitCommitRecord {
            repo: repo.to_path_buf(),
            branch: branch.to_string(),
            commit: commit.trim().to_string(),
        })
    }

    fn write_git_record(&self, dir: &Path, record: &GitCommitRecord) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(&record).map_err(|error| {
            AikitError::new(
                "procedure.git_commit_failed",
                format!("could not record git commit: {error}"),
            )
        })?;
        write_file(&dir.join(GIT_FILE), &bytes)
    }

    fn unstage_git_paths(&self, repo: &Path, procedure: &Procedure) -> Result<()> {
        let mut reset = vec!["reset".to_string(), "HEAD".to_string(), "--".to_string()];
        reset.extend(git_relative_paths(repo, procedure)?);
        git_owned(repo, &reset, "procedure.git_cleanup_failed")
    }

    /// Refuse an undo if any path no longer contains the result this Procedure
    /// wrote. This is a preflight over the complete plan: one drifted path means
    /// no inverse is applied, so undo cannot both erase later user work and leave
    /// the rest half-restored.
    fn verify_undo_preconditions(
        &self,
        dir: &Path,
        plan: &Plan,
        journal: &UndoRecord,
    ) -> Result<()> {
        let mut final_edits: BTreeMap<&Path, &WorldEdit> = BTreeMap::new();
        for edit in &plan.edits {
            if let Some(path) = edit.path() {
                final_edits.insert(path.as_path(), edit);
            }
        }
        for (path, edit) in final_edits {
            let matches = match edit {
                WorldEdit::CreateDir { .. } => path.is_dir() && !path.is_symlink(),
                WorldEdit::MovePath { to, .. } => {
                    !path.exists() && !path.is_symlink() && (to.exists() || to.is_symlink())
                }
                WorldEdit::WriteFile { contents, .. } => {
                    !path.is_symlink() && fs::read(path).is_ok_and(|bytes| bytes == *contents)
                }
                WorldEdit::WriteFileMode { contents, mode, .. } => {
                    !path.is_symlink()
                        && fs::read(path).is_ok_and(|bytes| bytes == *contents)
                        && file_mode(path).is_some_and(|current| current == *mode)
                }
                WorldEdit::DeleteFile { .. } => !path.exists() && !path.is_symlink(),
                WorldEdit::CreateLink { target, .. } => {
                    fs::read_link(path).is_ok_and(|current| current == *target)
                }
                WorldEdit::MarkedBlock { contents, .. } => {
                    let original = journal
                        .steps
                        .iter()
                        .find(|step| step.path.as_deref() == Some(path))
                        .and_then(|step| match &step.inverse {
                            Inverse::Restore { blob } => {
                                fs::read_to_string(dir.join(UNDO_DIR).join(blob.as_str())).ok()
                            }
                            Inverse::Remove => Some(String::new()),
                            Inverse::Recreate { .. } | Inverse::None => None,
                        });
                    original.is_some_and(|original| {
                        let expected = aikit_core::procedure::splice_marked_block(
                            &original,
                            comment_leader(path),
                            contents,
                        );
                        fs::read_to_string(path).is_ok_and(|current| current == expected)
                    })
                }
                WorldEdit::RunCommand { .. } => true,
            };
            if !matches {
                return Err(AikitError::new(
                    "procedure.undo_drift",
                    format!(
                        "refusing to undo because {} changed after the Procedure was applied",
                        path.display()
                    ),
                )
                .with("path", path.display().to_string())
                .with("edit", edit.describe()));
            }
        }
        Ok(())
    }

    /// Replay a journal's inverses in reverse order.
    fn undo_record(&self, dir: &Path, journal: &UndoRecord) -> Result<usize> {
        let mut undone = 0;
        for step in journal.steps.iter().rev() {
            if let (Some(from), Some(to)) = (&step.move_from, &step.move_to) {
                if !from.exists() && !from.is_symlink() {
                    return Err(AikitError::new(
                        "procedure.undo_failed",
                        format!("cannot restore move because {} is missing", from.display()),
                    )
                    .with("path", from.display().to_string()));
                }
                if to.exists() || to.is_symlink() {
                    return Err(AikitError::new(
                        "procedure.undo_drift",
                        format!("refusing to restore move over {}", to.display()),
                    )
                    .with("path", to.display().to_string()));
                }
                if let Some(parent) = to.parent() {
                    create_dir_all(parent)?;
                }
                fs::rename(from, to)
                    .map_err(|error| io_error("procedure.undo_failed", from, &error))?;
                undone += 1;
                continue;
            }
            if let Some(argv) = &step.undo_command {
                let cwd = step.undo_cwd.as_deref().ok_or_else(|| {
                    AikitError::new(
                        "procedure.undo_failed",
                        "a recorded undo command has no working directory",
                    )
                })?;
                run_command(argv, cwd, "procedure.undo_failed")?;
                undone += 1;
                continue;
            }
            match (&step.inverse, &step.path) {
                (Inverse::Restore { blob }, Some(path)) => {
                    let stored = dir.join(UNDO_DIR).join(blob.as_str());
                    let bytes = fs::read(&stored)
                        .map_err(|e| io_error("procedure.undo_failed", &stored, &e))?;
                    // `fs::write` follows a symlink. When the forward edit
                    // replaced a regular file with a projection link, writing
                    // the backup directly would corrupt the projection target
                    // and leave the link in place instead of restoring the
                    // original file.
                    if path.is_symlink() {
                        fs::remove_file(path)
                            .map_err(|e| io_error("procedure.undo_failed", path, &e))?;
                    }
                    write_file(path, &bytes)?;
                    if let Some(mode) = step.original_mode {
                        set_file_mode(path, mode)?;
                    }
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
        if let WorldEdit::MovePath { from, to } = edit {
            return Ok(UndoStep::move_path(
                to.clone(),
                from.clone(),
                edit.describe(),
            ));
        }
        if let WorldEdit::RunCommand { undo, cwd, .. } = edit {
            return Ok(UndoStep::command(
                undo.clone(),
                cwd.clone(),
                edit.describe(),
            ));
        }
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
        let mut step = UndoStep::new(
            Some(path.clone()),
            Inverse::Restore { blob },
            edit.describe(),
        );
        step.original_mode = file_mode(path);
        Ok(step)
    }

    /// Perform one edit.
    fn apply(&self, edit: &WorldEdit) -> Result<()> {
        match edit {
            WorldEdit::CreateDir { path, .. } => create_dir_all(path).map_err(|error| {
                AikitError::new("procedure.apply_failed", error.message().to_string())
                    .with("path", path.display().to_string())
            }),
            WorldEdit::MovePath { from, to } => {
                if let Some(parent) = to.parent() {
                    create_dir_all(parent)?;
                }
                fs::rename(from, to)
                    .map_err(|error| io_error("procedure.apply_failed", from, &error))
            }
            WorldEdit::WriteFile { path, contents, .. } => write_replacing_symlink(path, contents),
            WorldEdit::WriteFileMode {
                path,
                contents,
                mode,
                ..
            } => {
                write_replacing_symlink(path, contents)?;
                set_file_mode(path, *mode)
            }
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
                write_replacing_symlink(path, next.as_bytes())
            }
            WorldEdit::RunCommand { argv, cwd, .. } => {
                run_command(argv, cwd, "procedure.command_failed")
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

    fn rollback_uncommitted(
        &self,
        dir: &Path,
        journal: &UndoRecord,
        git_branch: Option<&(PathBuf, String, String)>,
    ) -> Result<()> {
        self.undo_record(dir, journal)?;
        if let Some((repo, original, branch)) = git_branch {
            ensure_git_clean(repo)?;
            cleanup_failed_branch(repo, original, branch)?;
        }
        discard_journal(dir);
        Ok(())
    }

    fn read_journal(&self, dir: &Path) -> Result<UndoRecord> {
        let path = dir.join(JOURNAL_FILE);
        let text = fs::read_to_string(&path)
            .map_err(|e| io_error("procedure.no_undo_record", &path, &e))?;
        serde_json::from_str(&text).map_err(|e| {
            AikitError::new(
                "procedure.no_undo_record",
                format!("{} is not a readable undo record: {e}", path.display()),
            )
        })
    }

    fn read_plan(&self, dir: &Path) -> Result<Plan> {
        let path = dir.join(PLAN_FILE);
        let text = fs::read_to_string(&path)
            .map_err(|e| io_error("procedure.no_plan_record", &path, &e))?;
        serde_json::from_str(&text).map_err(|e| {
            AikitError::new(
                "procedure.no_plan_record",
                format!("{} is not a readable plan record: {e}", path.display()),
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
                if !e.path().join(JOURNAL_FILE).is_file() {
                    return None;
                }
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
        WorldEdit::MovePath { from, to } => {
            format!("move {} back to {}", to.display(), from.display())
        }
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
fn file_mode(path: &Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    fs::symlink_metadata(path)
        .ok()
        .map(|metadata| metadata.permissions().mode() & 0o7777)
}

#[cfg(not(unix))]
fn file_mode(_path: &Path) -> Option<u32> {
    None
}

#[cfg(unix)]
fn set_file_mode(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|error| io_error("procedure.write_failed", path, &error))
}

#[cfg(not(unix))]
fn set_file_mode(_path: &Path, _mode: u32) -> Result<()> {
    Ok(())
}

fn run_command(argv: &[String], cwd: &Path, code: &'static str) -> Result<()> {
    let Some((program, args)) = argv.split_first() else {
        return Err(AikitError::new(code, "a Procedure command has no program"));
    };
    let status = std::process::Command::new(program)
        .args(args)
        .current_dir(cwd)
        .status()
        .map_err(|error| {
            AikitError::new(code, format!("could not run `{}`: {error}", argv.join(" ")))
                .with("command", argv.join(" "))
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(AikitError::new(
            code,
            format!(
                "`{}` exited with status {}",
                argv.join(" "),
                status.code().unwrap_or(-1)
            ),
        )
        .with("command", argv.join(" ")))
    }
}

fn write_replacing_symlink(path: &Path, contents: &[u8]) -> Result<()> {
    // `fs::write` follows a symlink. A WorldEdit names the directory entry it is
    // allowed to replace; it never grants authority over an arbitrary target
    // outside that path. Capture records Recreate(link), so replacing the link
    // also makes apply and undo exact inverses.
    if path.is_symlink() {
        fs::remove_file(path).map_err(|error| io_error("procedure.apply_failed", path, &error))?;
    }
    write_file(path, contents)
}

fn ensure_git_clean(repo: &Path) -> Result<()> {
    let status = git_output(
        repo,
        &["status", "--porcelain"],
        "procedure.git_setup_failed",
    )?;
    if status.trim().is_empty() {
        Ok(())
    } else {
        Err(AikitError::new(
            "procedure.git_dirty",
            format!(
                "refusing to stage a Procedure on a dirty repository at {}",
                repo.display()
            ),
        )
        .with("repo", repo.display().to_string())
        .with("status", status))
    }
}

fn cleanup_failed_branch(repo: &Path, original: &str, branch: &str) -> Result<()> {
    git(repo, &["switch", original], "procedure.git_cleanup_failed")?;
    git(
        repo,
        &["branch", "-D", branch],
        "procedure.git_cleanup_failed",
    )
}

fn discard_journal(dir: &Path) {
    let _ = fs::remove_file(dir.join(JOURNAL_FILE));
}

fn rollback_failure(
    procedure: &Procedure,
    dir: &Path,
    original: &AikitError,
    rollback: &AikitError,
) -> AikitError {
    AikitError::new(
        "procedure.rollback_failed",
        format!(
            "the Procedure failed ({}) and rollback also failed ({}); inspect {}",
            original.message(),
            rollback.message(),
            dir.display()
        ),
    )
    .with("original_code", original.code())
    .with("rollback_code", rollback.code())
    .with("procedure", procedure.id.to_string())
}

fn git_relative_paths(repo: &Path, procedure: &Procedure) -> Result<Vec<String>> {
    procedure
        .plan
        .touched_paths()
        .iter()
        .map(|path| {
            path.strip_prefix(repo)
                .map(|value| value.to_string_lossy().to_string())
                .map_err(|_| {
                    AikitError::new(
                        "procedure.git_setup_failed",
                        format!("{} is outside {}", path.display(), repo.display()),
                    )
                })
        })
        .collect()
}

fn git(repo: &Path, args: &[&str], code: &'static str) -> Result<()> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .map_err(|error| {
            AikitError::new(
                code,
                format!("could not run `git {}`: {error}", args.join(" ")),
            )
        })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(AikitError::new(
            code,
            format!(
                "`git {}` exited with status {}: {}",
                args.join(" "),
                output.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        ))
    }
}

fn git_owned(repo: &Path, args: &[String], code: &'static str) -> Result<()> {
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    git(repo, &borrowed, code)
}

fn git_output(repo: &Path, args: &[&str], code: &'static str) -> Result<String> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .map_err(|error| {
            AikitError::new(
                code,
                format!("could not run `git {}`: {error}", args.join(" ")),
            )
        })?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(AikitError::new(
            code,
            format!(
                "`git {}` failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        ))
    }
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

/// Bind every filesystem edit to the exact state observed during planning.
///
/// Existing entries are retained so a caller can bind a partial plan before
/// computing a review digest, append audit edits, and call this again for only
/// the newly-added paths.
pub fn bind_current_preconditions(mut plan: Plan) -> Result<Plan> {
    let mut bound: BTreeSet<PathBuf> = plan
        .preconditions
        .iter()
        .map(|precondition| precondition.path().clone())
        .collect();
    for path in plan.touched_paths() {
        if bound.insert(path.clone()) {
            plan.preconditions.push(observe_precondition(&path)?);
        }
    }
    plan.preconditions
        .sort_by(|left, right| left.path().cmp(right.path()));
    Ok(plan)
}

/// Bind a read-only dependency whose bytes affect the safety of a plan even
/// though the plan does not write that path.
pub fn bind_read_precondition(mut plan: Plan, path: &Path) -> Result<Plan> {
    if !plan
        .preconditions
        .iter()
        .any(|precondition| precondition.path() == path)
    {
        plan.preconditions.push(observe_precondition(path)?);
        plan.preconditions
            .sort_by(|left, right| left.path().cmp(right.path()));
    }
    Ok(plan)
}

fn observe_precondition(path: &Path) -> Result<WorldPrecondition> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(WorldPrecondition::Absent {
                path: path.to_path_buf(),
            })
        }
        Err(error) => return Err(io_error("procedure.precondition_unreadable", path, &error)),
    };
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        let target = fs::read_link(path)
            .map_err(|error| io_error("procedure.precondition_unreadable", path, &error))?;
        return Ok(WorldPrecondition::Link {
            path: path.to_path_buf(),
            target,
        });
    }
    if file_type.is_file() {
        let bytes = fs::read(path)
            .map_err(|error| io_error("procedure.precondition_unreadable", path, &error))?;
        return Ok(WorldPrecondition::File {
            path: path.to_path_buf(),
            hash: blake3::hash(&bytes).to_hex().to_string(),
            mode: file_mode(path).unwrap_or(0),
        });
    }
    if file_type.is_dir() {
        return Ok(WorldPrecondition::Directory {
            path: path.to_path_buf(),
            hash: hash_directory(path)?,
            mode: file_mode(path).unwrap_or(0),
        });
    }
    Err(AikitError::new(
        "procedure.precondition_unsupported",
        format!(
            "refusing to plan a mutation of unsupported filesystem object {}",
            path.display()
        ),
    )
    .with("path", path.display().to_string()))
}

fn hash_directory(root: &Path) -> Result<String> {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"aikit-directory-state-v1\n");
    let walker = walkdir::WalkDir::new(root)
        .follow_links(false)
        .sort_by_file_name();
    for entry in walker {
        let entry = entry.map_err(|error| {
            AikitError::new(
                "procedure.precondition_unreadable",
                format!("could not survey {}: {error}", root.display()),
            )
        })?;
        let path = entry.path();
        let relative = path.strip_prefix(root).unwrap_or(path);
        let metadata = fs::symlink_metadata(path)
            .map_err(|error| io_error("procedure.precondition_unreadable", path, &error))?;
        let mode = file_mode(path).unwrap_or(0);
        let kind = if metadata.file_type().is_symlink() {
            "link"
        } else if metadata.is_dir() {
            "directory"
        } else if metadata.is_file() {
            "file"
        } else {
            return Err(AikitError::new(
                "procedure.precondition_unsupported",
                format!("unsupported filesystem object inside {}", root.display()),
            )
            .with("path", path.display().to_string()));
        };
        let header = format!("{kind}|{}|{mode:04o}", relative.display());
        hasher.update(&(header.len() as u64).to_le_bytes());
        hasher.update(header.as_bytes());
        if metadata.file_type().is_symlink() {
            let target = fs::read_link(path)
                .map_err(|error| io_error("procedure.precondition_unreadable", path, &error))?;
            let target = target.as_os_str().as_encoded_bytes();
            hasher.update(&(target.len() as u64).to_le_bytes());
            hasher.update(target);
        } else if metadata.is_file() {
            let bytes = fs::read(path)
                .map_err(|error| io_error("procedure.precondition_unreadable", path, &error))?;
            hasher.update(&(bytes.len() as u64).to_le_bytes());
            hasher.update(&bytes);
        }
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn verify_forward_precondition(expected: &WorldPrecondition) -> Result<()> {
    let actual = observe_precondition(expected.path())?;
    if actual == *expected {
        return Ok(());
    }
    Err(AikitError::new(
        "procedure.precondition_failed",
        format!(
            "{} changed after this Procedure was planned; refusing to overwrite concurrent work",
            expected.path().display()
        ),
    )
    .with("path", expected.path().display().to_string())
    .with("expected", expected.digest_line())
    .with("actual", actual.digest_line()))
}

fn verify_satisfied_result(plan: &Plan) -> Result<()> {
    let preconditions: BTreeMap<&Path, &WorldPrecondition> = plan
        .preconditions
        .iter()
        .map(|precondition| (precondition.path().as_path(), precondition))
        .collect();
    let mut checked = BTreeSet::new();
    for edit in plan.edits.iter().rev() {
        if let Some(path) = edit.path() {
            if !checked.insert(path.clone()) {
                continue;
            }
        }
        let matches = match edit {
            WorldEdit::CreateDir { path, .. } => path.is_dir() && !path.is_symlink(),
            WorldEdit::MovePath { from, to } => {
                !from.exists()
                    && !from.is_symlink()
                    && preconditions
                        .get(from.as_path())
                        .is_some_and(|expected| state_matches_at(expected, to))
            }
            WorldEdit::WriteFile { path, contents, .. } => {
                !path.is_symlink() && fs::read(path).is_ok_and(|bytes| bytes == *contents)
            }
            WorldEdit::WriteFileMode {
                path,
                contents,
                mode,
                ..
            } => {
                !path.is_symlink()
                    && fs::read(path).is_ok_and(|bytes| bytes == *contents)
                    && file_mode(path).is_some_and(|actual| actual == *mode)
            }
            WorldEdit::DeleteFile { path, .. } => !path.exists() && !path.is_symlink(),
            WorldEdit::CreateLink { path, target, .. } => {
                fs::read_link(path).is_ok_and(|actual| actual == *target)
            }
            WorldEdit::MarkedBlock { path, contents, .. } => {
                let block =
                    aikit_core::procedure::render_marked_block(comment_leader(path), contents);
                fs::read_to_string(path).is_ok_and(|actual| actual.contains(&block))
            }
            WorldEdit::RunCommand { .. } => true,
        };
        if !matches {
            return Err(AikitError::new(
                "procedure.satisfied_drift",
                format!(
                    "a satisfied Procedure no longer owns its expected result for `{}`",
                    edit.describe()
                ),
            )
            .with("edit", edit.describe()));
        }
    }
    Ok(())
}

fn state_matches_at(expected: &WorldPrecondition, path: &Path) -> bool {
    let Ok(actual) = observe_precondition(path) else {
        return false;
    };
    match (expected, actual) {
        (WorldPrecondition::Absent { .. }, WorldPrecondition::Absent { .. }) => true,
        (
            WorldPrecondition::File {
                hash: expected_hash,
                mode: expected_mode,
                ..
            },
            WorldPrecondition::File {
                hash: actual_hash,
                mode: actual_mode,
                ..
            },
        ) => expected_hash == &actual_hash && expected_mode == &actual_mode,
        (
            WorldPrecondition::Link {
                target: expected_target,
                ..
            },
            WorldPrecondition::Link {
                target: actual_target,
                ..
            },
        ) => expected_target == &actual_target,
        (
            WorldPrecondition::Directory {
                hash: expected_hash,
                mode: expected_mode,
                ..
            },
            WorldPrecondition::Directory {
                hash: actual_hash,
                mode: actual_mode,
                ..
            },
        ) => expected_hash == &actual_hash && expected_mode == &actual_mode,
        _ => false,
    }
}

/// Convenience for callers that hold a plan and want it staged with the isolation
/// the rule selects (Spec II §1.2), with the shadow root under the AIKit home.
pub fn plan_procedure(
    home: &AikitHome,
    kind: aikit_core::procedure::ProcedureKind,
    plan: Plan,
) -> Result<Procedure> {
    let plan = bind_current_preconditions(plan)?;
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
