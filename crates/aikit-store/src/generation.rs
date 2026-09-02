//! Immutable, content-addressed generations.
//!
//! This is the file `ARCHITECTURE.md` §6 is about, and it exists to make one
//! sentence true on a real filesystem: **a failed build never replaces the
//! existing view.** Everything below is arranged around that.
//!
//! ## The order of operations
//!
//! ```text
//! stage into generations/.staging-<ulid>/   ← nothing outside the temp dir moves
//!   write resolution.lock.toml
//!   create bin/ hooks/ guidance/ projections/
//!   execute every projection item
//! validate the built tree                    ← still nothing outside has moved
//! hash the tree → gen_<hash>
//! ── commit ──────────────────────────────
//! take the context lock
//! compare `current` against the expected base ← compare-and-swap
//! rename staging → generations/gen_<hash>
//! point `previous` at the old target
//! atomically replace `current`
//! ```
//!
//! Every step before the commit line is reversible by deleting one directory,
//! which is what [`StagedGeneration`]'s `Drop` does. Every step after it is a
//! `rename(2)`, which is atomic: a reader following `current` at any instant sees
//! either the old generation or the new one, never a partially written tree and
//! never a dangling link.
//!
//! ## Why `current` is replaced by rename and not by remove-then-create
//!
//! `remove_file(current)` followed by `symlink(new, current)` has a window in
//! which `AIKIT_VIEW` points at nothing. Every shell, hook dispatcher and client
//! that resolves the path during that window fails. Writing `current.tmp` and
//! renaming it over `current` has no such window.
//!
//! ## Why `previous` is written before `current`
//!
//! If the process dies between the two, `previous` and `current` briefly name the
//! same generation: a rollback is then a no-op, which is safe. The other order
//! would leave `previous` naming a generation two steps back, and a rollback
//! would take the user somewhere they never were.
//!
//! ## Compare-and-swap rather than a lock alone
//!
//! The lock serializes two commits; it does not make the second one *correct*.
//! A pane that resolved against generation A and then committed after another
//! pane replaced A with B would silently discard B's change. So the commit states
//! the base it believes it is replacing, and a mismatch is
//! `generation.stale_base` — refused, with both ids in the error.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use aikit_core::error::err;
use aikit_core::projection::{MaterializationMode, ProjectionItem, ProjectionPlan};
use aikit_core::{
    ActivationEffect, AikitError, GenerationId, Isolation, ResolvedView, Result, TargetId,
};

use crate::events::Timestamp;
use crate::home::{create_dir_all, io_error};
use crate::locks::{ContextLock, LockOptions};

/// The names §6 fixes. Consumers depend on them, so they are constants.
pub const CURRENT: &str = "current";
pub const PREVIOUS: &str = "previous";
pub const GENERATIONS: &str = "generations";
pub const LOCK_FILE: &str = "resolution.lock.toml";
pub const METADATA_FILE: &str = "metadata.json";
/// The environment a context exports, one `NAME=value` per line. Read by the
/// shell integration through the stable `AIKIT_VIEW/env` path.
pub const ENV_FILE: &str = "env";

/// The on-disk generation format. Its **presence** in `metadata.json` is the test
/// for "this directory is a generation" (PRIOR-ART-ACTIONS #8, from Guix's
/// `%manifest-format-version` and home-manager's `gen-version`). Distinct from
/// [`GenerationMetadata::schema`], which versions the metadata *document*: the
/// format stamp changes only when the on-disk generation layout does.
pub const GENERATION_FORMAT: u32 = 1;

/// Staging directories are hidden so a `generations/*` listing never mistakes an
/// in-progress build for a real generation.
const STAGING_PREFIX: &str = ".staging-";

// ---------------------------------------------------------------------------
// Metadata
// ---------------------------------------------------------------------------

/// What one target got, recorded so `aikit explain` can answer "why does Codex
/// behave differently" without re-planning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetRecord {
    pub target: String,
    pub digest: String,
    pub items: usize,
    pub effect: String,
    #[serde(default)]
    pub notes: Vec<String>,
}

/// `metadata.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenerationMetadata {
    pub schema: u32,
    /// The on-disk generation format stamp; its presence marks the directory as a
    /// generation. See [`GENERATION_FORMAT`].
    #[serde(default)]
    pub generation_format: u32,
    pub generation_id: GenerationId,
    pub context_id: String,
    pub created_at: Timestamp,
    pub resolution_hash: String,
    pub catalog_revision: String,
    #[serde(default)]
    pub base_generation: Option<GenerationId>,
    pub isolation: Isolation,
    pub materialization: MaterializationMode,
    pub targets: Vec<TargetRecord>,
    /// Honest notes: degradations, fallbacks and the reasons for them.
    #[serde(default)]
    pub notes: Vec<String>,
}

/// Whether `dir` is a generation, decided by the presence of the
/// [`GENERATION_FORMAT`] stamp in a readable `metadata.json`.
///
/// This is deliberately the *stamp*, not "does a `metadata.json` exist": a
/// half-written directory or a foreign tree that happens to hold a `metadata.json`
/// is not a generation. `gc` and any future audit can trust this predicate.
pub fn is_generation(dir: &Path) -> bool {
    read_metadata(dir).is_ok_and(|m| m.generation_format >= 1)
}

/// Read a generation's metadata.
pub fn read_metadata(generation_dir: &Path) -> Result<GenerationMetadata> {
    let path = generation_dir.join(METADATA_FILE);
    let text =
        fs::read_to_string(&path).map_err(|e| io_error("generation.unreadable", &path, &e))?;
    serde_json::from_str(&text).map_err(|e| {
        AikitError::new(
            "generation.unreadable",
            format!("{} is not readable metadata: {e}", path.display()),
        )
        .with("path", path.display().to_string())
    })
}

fn write_metadata(generation_dir: &Path, metadata: &GenerationMetadata) -> Result<()> {
    let encoded = serde_json::to_string_pretty(metadata).map_err(|e| {
        AikitError::new(
            "generation.metadata_unserializable",
            format!("could not encode generation metadata: {e}"),
        )
    })?;
    write_file(&generation_dir.join(METADATA_FILE), encoded.as_bytes())
}

/// Attach cosmetic `[properties]` (a label, a note) to a committed generation by
/// rewriting only that table in its lock.
///
/// Safe on an "immutable" generation precisely because [`hash_tree`] excludes
/// `[properties]` from identity: the directory name still equals the content hash
/// afterwards, `current`/`previous` are untouched, and no new generation is minted
/// (PRIOR-ART-ACTIONS #9). A no-op when the properties are already what is asked.
pub fn relabel(
    generation_dir: &Path,
    properties: &std::collections::BTreeMap<String, String>,
) -> Result<()> {
    let mut view = read_lock(generation_dir)?;
    if &view.properties == properties {
        return Ok(());
    }
    view.properties = properties.clone();
    let text = toml::to_string_pretty(&view).map_err(|e| {
        AikitError::new(
            "generation.lock_unserializable",
            format!("could not re-serialize the lock to attach properties: {e}"),
        )
    })?;

    // This is the one write that lands *inside* an already-committed generation,
    // so it has to honour the same no-window discipline as every other write to
    // committed state (see the module header). A bare `fs::write` truncates first:
    // a crash between the truncate and the write would leave the lock of an
    // immutable generation empty, `read_lock` would fail forever, and — because
    // the lock is excluded from `hash_tree` — the directory name would still match
    // its content hash, so nothing could even detect the corruption. Write beside
    // it and rename over: `rename(2)` is atomic, so a reader sees the old lock or
    // the new one, never a truncated one.
    let final_path = generation_dir.join(LOCK_FILE);
    let temporary = generation_dir.join(format!("{LOCK_FILE}.tmp"));
    write_file(&temporary, text.as_bytes())?;
    fs::rename(&temporary, &final_path).map_err(|e| {
        let _ = fs::remove_file(&temporary);
        io_error("generation.write_failed", &final_path, &e)
    })
}

/// On an identical re-apply, carry any label the new build declared onto the
/// generation already on disk.
fn carry_properties(staging: &Path, final_dir: &Path) -> Result<()> {
    let staged = read_lock(staging)?;
    if staged.properties.is_empty() {
        return Ok(());
    }
    relabel(final_dir, &staged.properties)
}

/// Read a generation's resolved view back out of its lock file.
pub fn read_lock(generation_dir: &Path) -> Result<ResolvedView> {
    let path = generation_dir.join(LOCK_FILE);
    let text =
        fs::read_to_string(&path).map_err(|e| io_error("generation.unreadable", &path, &e))?;
    toml::from_str(&text).map_err(|e| {
        AikitError::new(
            "generation.unreadable",
            format!("{} is not a readable resolution lock: {e}", path.display()),
        )
        .with("path", path.display().to_string())
    })
}

// ---------------------------------------------------------------------------
// The builder
// ---------------------------------------------------------------------------

/// Builds a generation into a temporary directory.
#[derive(Debug, Clone)]
pub struct GenerationBuilder {
    mode: MaterializationMode,
    symlinks: bool,
    /// The command a generated shim execs. Configurable so a test — or a user
    /// with AIKit installed under another name — is not forced to have `aikit`
    /// on the PATH for the shim to be truthful.
    aikit_command: String,
    lock_timeout: Duration,
}

impl Default for GenerationBuilder {
    fn default() -> Self {
        Self {
            mode: MaterializationMode::Auto,
            symlinks: cfg!(unix),
            aikit_command: "aikit".to_string(),
            lock_timeout: Duration::from_secs(30),
        }
    }
}

impl GenerationBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_mode(mut self, mode: MaterializationMode) -> Self {
        self.mode = mode;
        self
    }

    /// Declare that this filesystem or configuration cannot use symlinks.
    #[must_use]
    pub fn without_symlinks(mut self) -> Self {
        self.symlinks = false;
        self
    }

    #[must_use]
    pub fn with_aikit_command(mut self, command: impl Into<String>) -> Self {
        self.aikit_command = command.into();
        self
    }

    #[must_use]
    pub fn with_lock_timeout(mut self, timeout: Duration) -> Self {
        self.lock_timeout = timeout;
        self
    }

    /// The concrete mode, and whether honouring the request required a
    /// degradation the user is owed an explanation for.
    fn effective_mode(&self) -> (MaterializationMode, Option<String>) {
        match self.mode {
            MaterializationMode::Copy => (MaterializationMode::Copy, None),
            MaterializationMode::Link if !self.symlinks => (
                MaterializationMode::Copy,
                Some(
                    "link mode was requested but this filesystem or configuration does not \
                     support symlinks, so payloads were copied"
                        .to_string(),
                ),
            ),
            MaterializationMode::Auto if !self.symlinks => (
                MaterializationMode::Copy,
                Some("symlinks are unavailable here, so payloads were copied".to_string()),
            ),
            _ => (MaterializationMode::Link, None),
        }
    }

    /// Materialize `view` and `plans` into a staging directory under
    /// `context_dir/generations/`, validate the result, and name it by content.
    ///
    /// Nothing outside the staging directory is touched. On any error the staging
    /// directory is removed and `current` is exactly as it was.
    pub fn build(
        &self,
        context_dir: &Path,
        view: &ResolvedView,
        plans: &[ProjectionPlan],
    ) -> Result<StagedGeneration> {
        create_dir_all(&context_dir.join(GENERATIONS))?;
        let staging = context_dir
            .join(GENERATIONS)
            .join(format!("{STAGING_PREFIX}{}", ulid::Ulid::generate()));
        create_dir_all(&staging)?;

        // From here on every failure has to take the staging directory with it,
        // so the work is done in a closure and cleaned up in one place.
        let built = self.materialize(&staging, context_dir, view, plans);
        match built {
            Ok(staged) => Ok(staged),
            Err(error) => {
                let _ = fs::remove_dir_all(&staging);
                Err(error)
            }
        }
    }

    fn materialize(
        &self,
        staging: &Path,
        context_dir: &Path,
        view: &ResolvedView,
        plans: &[ProjectionPlan],
    ) -> Result<StagedGeneration> {
        let (mode, degradation) = self.effective_mode();

        // The lock first: it is the artefact everything else is derived from, and
        // a generation without a readable lock is not explainable.
        let lock_text = toml::to_string_pretty(view).map_err(|e| {
            AikitError::new(
                "generation.lock_unserializable",
                format!("the resolved view could not be written as a lock file: {e}"),
            )
        })?;
        write_file(&staging.join(LOCK_FILE), lock_text.as_bytes())?;

        for dir in ["bin", "hooks", "guidance", "projections"] {
            create_dir_all(&staging.join(dir))?;
        }

        let mut targets = Vec::new();
        for plan in plans {
            let root = staging.join(plan_root(&plan.target));
            create_dir_all(&root)?;
            for item in &plan.items {
                self.execute(item, &root, staging, mode)?;
            }
            targets.push(TargetRecord {
                target: plan.target.as_str().to_string(),
                digest: plan.digest(),
                items: plan.items.len(),
                effect: describe_effect(&plan.effect),
                notes: plan.notes.clone(),
            });
        }

        validate(staging, plans)?;

        // Hash after validation and before metadata, because metadata names the
        // hash and cannot be part of what is hashed.
        let id = hash_tree(staging, view)?;

        let mut notes: Vec<String> = degradation.into_iter().collect();
        for plan in plans {
            if let ActivationEffect::Brokered { reason }
            | ActivationEffect::Unsupported { reason } = &plan.effect
            {
                notes.push(format!("{}: {reason}", plan.target));
            }
        }

        let metadata = GenerationMetadata {
            schema: 1,
            generation_format: GENERATION_FORMAT,
            generation_id: id.clone(),
            context_id: view.context.context_id.to_string(),
            created_at: Timestamp::now(),
            resolution_hash: view.hash.to_string(),
            catalog_revision: view.catalog_revision.clone(),
            base_generation: None,
            isolation: view.context.isolation,
            materialization: mode,
            targets,
            notes,
        };

        // Written now rather than at commit time so that a staged generation is
        // a complete, inspectable tree — `apply --dry-run` shows the user the
        // real thing. `base_generation` is filled in at commit, when the base is
        // finally known; the file is excluded from the hash so rewriting it
        // cannot change the generation's identity.
        write_metadata(staging, &metadata)?;

        Ok(StagedGeneration {
            context_dir: context_dir.to_path_buf(),
            staging: staging.to_path_buf(),
            id,
            metadata,
            lock_timeout: self.lock_timeout,
            committed: false,
        })
    }

    fn execute(
        &self,
        item: &ProjectionItem,
        root: &Path,
        staging: &Path,
        mode: MaterializationMode,
    ) -> Result<()> {
        match item {
            ProjectionItem::Link { from, to } => {
                let destination = root.join(to);
                require_source(from)?;
                match mode {
                    MaterializationMode::Copy => copy_tree(from, &destination),
                    _ => {
                        if let Some(parent) = destination.parent() {
                            create_dir_all(parent)?;
                        }
                        symlink(from, &destination)
                    }
                }
            }
            ProjectionItem::Copy { from, to } => {
                require_source(from)?;
                copy_tree(from, &root.join(to))
            }
            ProjectionItem::Write { path, contents } => {
                write_file(&root.join(path), contents.as_bytes())
            }
            // Environment variables are not files. They are collected into the
            // generation's `env` manifest, which the shell integration sources —
            // `AIKIT_VIEW/env` is a stable path, so a shell can read the current
            // context's environment without AIKit running.
            ProjectionItem::Env { name, value } => {
                let path = staging.join(ENV_FILE);
                let mut existing = fs::read_to_string(&path).unwrap_or_default();
                existing.push_str(&format!("{name}={value}\n"));
                write_file(&path, existing.as_bytes())
            }
            ProjectionItem::Shim {
                name,
                capsule,
                export,
            } => {
                // Shims always land in the context's `bin/`, whatever target
                // planned them: `bin/` *is* the contextual PATH, and a shim
                // anywhere else would not be on it.
                let path = staging.join("bin").join(name);
                let body = format!(
                    "#!/bin/sh\n\
                     # Generated by AIKit. This file is part of an immutable generation;\n\
                     # edit the capsule, not the shim.\n\
                     exec {aikit} run '{capsule}' --export '{export}' \"$@\"\n",
                    aikit = self.aikit_command,
                );
                write_file(&path, body.as_bytes())?;
                make_executable(&path)
            }
        }
    }
}

/// Where a plan's items land inside a generation.
///
/// The three well-known surfaces get the top-level directories §6 names, because
/// the hook dispatcher and the shell integration read those paths literally.
/// Everything else is namespaced under `projections/` so a third-party adapter
/// cannot collide with them.
pub fn plan_root(target: &TargetId) -> PathBuf {
    match target.as_str() {
        TargetId::HOOKS => PathBuf::from("hooks"),
        TargetId::GUIDANCE => PathBuf::from("guidance"),
        TargetId::SHELL => PathBuf::from("bin"),
        TargetId::CLAUDE_CODE => PathBuf::from("projections/claude"),
        other => PathBuf::from("projections").join(other),
    }
}

fn describe_effect(effect: &ActivationEffect) -> String {
    effect.describe()
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Check the built tree before anything is allowed to point at it.
///
/// The checks are the failure modes that actually happen: a projection that did
/// not land, a symlink into a payload that has since been removed, a shim that is
/// not executable, and a lock file that cannot be read back.
fn validate(staging: &Path, plans: &[ProjectionPlan]) -> Result<()> {
    read_lock(staging).map_err(|e| {
        AikitError::new(
            "generation.validation_failed",
            format!(
                "the generation's own lock file is not re-readable: {}",
                e.message()
            ),
        )
    })?;

    for plan in plans {
        let root = staging.join(plan_root(&plan.target));
        for item in &plan.items {
            let path = match item {
                // An env var lands in the generation's `env` manifest, not at a
                // destination of its own.
                ProjectionItem::Env { .. } => staging.join(ENV_FILE),
                ProjectionItem::Shim { name, .. } => staging.join("bin").join(name),
                _ => match item.destination() {
                    Some(destination) => root.join(destination),
                    None => continue,
                },
            };
            if fs::symlink_metadata(&path).is_err() {
                return Err(AikitError::new(
                    "generation.validation_failed",
                    format!(
                        "{} was planned but is not present in the built tree",
                        path.display()
                    ),
                )
                .with("path", path.display().to_string())
                .with("target", plan.target.as_str()));
            }
            // Following the link is the point: a link whose target has gone is
            // exactly the dead symlink a user finds months later.
            if fs::metadata(&path).is_err() {
                return Err(AikitError::new(
                    "generation.validation_failed",
                    format!("{} does not resolve to anything", path.display()),
                )
                .with("path", path.display().to_string())
                .with("target", plan.target.as_str()));
            }
            if let ProjectionItem::Shim { name, .. } = item {
                if !is_executable(&path) {
                    return Err(AikitError::new(
                        "generation.validation_failed",
                        format!("the shim `{name}` is not executable"),
                    )
                    .with("path", path.display().to_string()));
                }
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The staged generation
// ---------------------------------------------------------------------------

/// A built, validated generation that nothing points at yet.
///
/// Dropping one without committing removes its staging directory: an abandoned
/// build must not accumulate on disk, and it must never be mistaken for a real
/// generation by `list` or `gc`.
pub struct StagedGeneration {
    context_dir: PathBuf,
    staging: PathBuf,
    id: GenerationId,
    metadata: GenerationMetadata,
    lock_timeout: Duration,
    committed: bool,
}

/// The result of a successful commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommittedGeneration {
    pub id: GenerationId,
    pub path: PathBuf,
    /// What `current` pointed at before, and what `previous` points at now.
    pub replaced: Option<GenerationId>,
}

impl StagedGeneration {
    pub fn id(&self) -> &GenerationId {
        &self.id
    }

    /// The staging directory. Valid only until `commit` or drop.
    pub fn path(&self) -> &Path {
        &self.staging
    }

    pub fn metadata(&self) -> &GenerationMetadata {
        &self.metadata
    }

    /// Promote this generation, refusing if `current` is not `expected_base`.
    pub fn commit(mut self, expected_base: Option<&GenerationId>) -> Result<CommittedGeneration> {
        let context_dir = self.context_dir.clone();
        let _lock = ContextLock::acquire_at(
            &context_dir.join(".lock"),
            &context_dir
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "context".to_string()),
            LockOptions::default()
                .with_timeout(self.lock_timeout)
                .with_purpose("commit a generation"),
        )?;

        let actual = current(&context_dir)?;
        if actual.as_ref() != expected_base {
            // The staging directory goes with the refusal: it was built against
            // a view that is no longer the one in force.
            let error = stale_base(expected_base, actual.as_ref());
            self.discard_staging();
            return Err(error);
        }

        let final_dir = context_dir.join(GENERATIONS).join(self.id.as_str());
        if final_dir.exists() {
            // The same content already exists — an identical re-apply. Keep the
            // one on disk and throw the duplicate away rather than churning it.
            // But a cosmetic label the new apply carried is not part of the
            // identity, so carry it onto the existing generation in place: a label
            // edit must update, never mint (PRIOR-ART-ACTIONS #9).
            carry_properties(&self.staging, &final_dir)?;
            self.discard_staging();
        } else {
            self.metadata.base_generation = actual.clone();
            write_metadata(&self.staging, &self.metadata)?;
            fs::rename(&self.staging, &final_dir)
                .map_err(|e| io_error("generation.commit_failed", &final_dir, &e))?;
            self.committed = true;
        }
        self.committed = true;

        // `previous` first: see the module header for why this order.
        if let Some(old) = &actual {
            set_pointer(&context_dir, PREVIOUS, old)?;
        }
        set_pointer(&context_dir, CURRENT, &self.id)?;

        Ok(CommittedGeneration {
            id: self.id.clone(),
            path: final_dir,
            replaced: actual,
        })
    }

    /// Throw the build away without touching anything else.
    pub fn discard(mut self) -> Result<()> {
        self.discard_staging();
        Ok(())
    }

    fn discard_staging(&mut self) {
        let _ = fs::remove_dir_all(&self.staging);
        self.committed = true;
    }
}

impl Drop for StagedGeneration {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_dir_all(&self.staging);
        }
    }
}

impl std::fmt::Debug for StagedGeneration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StagedGeneration")
            .field("id", &self.id)
            .field("staging", &self.staging)
            .finish()
    }
}

fn stale_base(expected: Option<&GenerationId>, actual: Option<&GenerationId>) -> AikitError {
    let none = "none".to_string();
    AikitError::new(
        "generation.stale_base",
        format!(
            "this apply was built against generation {}, but the context is now at {}; \
             re-resolve and apply again rather than discarding the other change",
            expected
                .map(|g| g.to_string())
                .unwrap_or_else(|| none.clone()),
            actual.map(|g| g.to_string()).unwrap_or(none),
        ),
    )
    .with(
        "expected",
        expected.map(|g| g.to_string()).unwrap_or_default(),
    )
    .with("actual", actual.map(|g| g.to_string()).unwrap_or_default())
}

// ---------------------------------------------------------------------------
// Pointers
// ---------------------------------------------------------------------------

/// The stable path clients and shells hold: `<context>/current`.
pub fn current_path(context_dir: &Path) -> PathBuf {
    context_dir.join(CURRENT)
}

pub fn previous_path(context_dir: &Path) -> PathBuf {
    context_dir.join(PREVIOUS)
}

pub fn current(context_dir: &Path) -> Result<Option<GenerationId>> {
    read_pointer(&current_path(context_dir))
}

pub fn previous(context_dir: &Path) -> Result<Option<GenerationId>> {
    read_pointer(&previous_path(context_dir))
}

fn read_pointer(path: &Path) -> Result<Option<GenerationId>> {
    let target = match fs::read_link(path) {
        Ok(target) => target,
        Err(_) => return Ok(None),
    };
    let Some(name) = target.file_name().and_then(|n| n.to_str()) else {
        return Ok(None);
    };
    Ok(GenerationId::parse(name).ok())
}

/// Point `name` at `generations/<id>` atomically.
///
/// The target is written **relative** to the context directory, so the whole
/// context can be moved or restored from a backup without every pointer becoming
/// a dangling absolute path.
fn set_pointer(context_dir: &Path, name: &str, id: &GenerationId) -> Result<()> {
    let temporary = context_dir.join(format!("{name}.tmp"));
    let _ = fs::remove_file(&temporary);
    let target = PathBuf::from(GENERATIONS).join(id.as_str());
    symlink(&target, &temporary)?;
    fs::rename(&temporary, context_dir.join(name))
        .map_err(|e| io_error("generation.pointer_failed", &context_dir.join(name), &e))
}

// ---------------------------------------------------------------------------
// Rollback, listing and collection
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RollbackOutcome {
    pub was_current: GenerationId,
    pub now_current: GenerationId,
}

/// Swap `current` and `previous`.
///
/// Swapping rather than merely stepping back is what makes a rollback itself
/// undoable: the user who rolls back by mistake rolls forward again.
pub fn rollback(context_dir: &Path) -> Result<RollbackOutcome> {
    let _lock = ContextLock::acquire_at(
        &context_dir.join(".lock"),
        "rollback",
        LockOptions::default().with_purpose("roll back a generation"),
    )?;

    let Some(was_current) = current(context_dir)? else {
        return err(
            "generation.no_previous",
            "this context has no generation to roll back from",
        );
    };
    let Some(now_current) = previous(context_dir)? else {
        return err(
            "generation.no_previous",
            "this context has only one generation, so there is nothing to roll back to",
        );
    };

    set_pointer(context_dir, PREVIOUS, &was_current)?;
    set_pointer(context_dir, CURRENT, &now_current)?;

    Ok(RollbackOutcome {
        was_current,
        now_current,
    })
}

/// Every real generation in a context, newest first.
pub fn list(context_dir: &Path) -> Result<Vec<GenerationId>> {
    Ok(list_with_times(context_dir)?
        .into_iter()
        .map(|(id, _)| id)
        .collect())
}

fn list_with_times(context_dir: &Path) -> Result<Vec<(GenerationId, i64)>> {
    let generations = context_dir.join(GENERATIONS);
    let entries = match fs::read_dir(&generations) {
        Ok(entries) => entries,
        Err(_) => return Ok(vec![]),
    };

    let mut out: Vec<(GenerationId, i64)> = Vec::new();
    for entry in entries.filter_map(std::result::Result::ok) {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with(STAGING_PREFIX) {
            continue;
        }
        let Ok(id) = GenerationId::parse(&name) else {
            continue;
        };
        // Prefer the recorded creation time: directory mtimes are altered by
        // ordinary maintenance and would make `gc` delete the wrong thing.
        let created = read_metadata(&entry.path())
            .map(|m| m.created_at.as_nanos())
            .unwrap_or(0);
        out.push((id, created));
    }
    out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| b.0.cmp(&a.0)));
    Ok(out)
}

/// Delete unreferenced generations, keeping the `keep` most recent.
///
/// `current` and `previous` are never deleted, whatever `keep` says. A `gc` that
/// could unmake the live view would be a different and much worse command.
pub fn gc(context_dir: &Path, keep: usize) -> Result<Vec<GenerationId>> {
    let _lock = ContextLock::acquire_at(
        &context_dir.join(".lock"),
        "gc",
        LockOptions::default().with_purpose("collect old generations"),
    )?;

    let referenced: Vec<GenerationId> = [current(context_dir)?, previous(context_dir)?]
        .into_iter()
        .flatten()
        .collect();

    let all = list_with_times(context_dir)?;
    let mut deleted = Vec::new();
    for (index, (id, _)) in all.iter().enumerate() {
        if referenced.contains(id) || index < keep {
            continue;
        }
        let path = context_dir.join(GENERATIONS).join(id.as_str());
        fs::remove_dir_all(&path).map_err(|e| io_error("generation.gc_failed", &path, &e))?;
        deleted.push(id.clone());
    }

    // Also sweep abandoned staging directories: a process killed mid-build leaves
    // one, and nothing else will ever clean it up.
    if let Ok(entries) = fs::read_dir(context_dir.join(GENERATIONS)) {
        for entry in entries.filter_map(std::result::Result::ok) {
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with(STAGING_PREFIX)
            {
                let _ = fs::remove_dir_all(entry.path());
            }
        }
    }
    Ok(deleted)
}

// ---------------------------------------------------------------------------
// Filesystem primitives
// ---------------------------------------------------------------------------

fn require_source(from: &Path) -> Result<()> {
    if fs::symlink_metadata(from).is_err() {
        return Err(AikitError::new(
            "generation.source_missing",
            format!(
                "{} was planned into this generation but does not exist; the capsule's payload \
                 may have been removed since the catalog was indexed",
                from.display()
            ),
        )
        .with("path", from.display().to_string()));
    }
    Ok(())
}

fn write_file(path: &Path, contents: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        create_dir_all(parent)?;
    }
    fs::write(path, contents).map_err(|e| io_error("generation.write_failed", path, &e))
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    let metadata =
        fs::metadata(from).map_err(|e| io_error("generation.source_missing", from, &e))?;
    if let Some(parent) = to.parent() {
        create_dir_all(parent)?;
    }
    if metadata.is_dir() {
        create_dir_all(to)?;
        let entries =
            fs::read_dir(from).map_err(|e| io_error("generation.source_missing", from, &e))?;
        for entry in entries {
            let entry = entry.map_err(|e| io_error("generation.source_missing", from, &e))?;
            copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        fs::copy(from, to).map_err(|e| io_error("generation.write_failed", to, &e))?;
        // Preserve the execute bit: a copied hook that cannot run is worse than
        // no hook, because the chain would report a system failure instead.
        preserve_mode(from, to)
    }
}

#[cfg(unix)]
fn preserve_mode(from: &Path, to: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(from)
        .map_err(|e| io_error("generation.source_missing", from, &e))?
        .permissions()
        .mode();
    fs::set_permissions(to, fs::Permissions::from_mode(mode))
        .map_err(|e| io_error("generation.write_failed", to, &e))
}

#[cfg(not(unix))]
fn preserve_mode(_from: &Path, _to: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
        .map_err(|e| io_error("generation.write_failed", path, &e))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(unix)]
fn symlink(target: &Path, link: &Path) -> Result<()> {
    std::os::unix::fs::symlink(target, link)
        .map_err(|e| io_error("generation.link_failed", link, &e))
}

#[cfg(windows)]
fn symlink(target: &Path, link: &Path) -> Result<()> {
    let result = if target.is_dir() {
        std::os::windows::fs::symlink_dir(target, link)
    } else {
        std::os::windows::fs::symlink_file(target, link)
    };
    result.map_err(|e| io_error("generation.link_failed", link, &e))
}

// ---------------------------------------------------------------------------
// Content addressing
// ---------------------------------------------------------------------------

/// Hash the built tree into a generation id.
///
/// Symlinks are *followed*, so a link-mode and a copy-mode build of the same view
/// are the same generation — which is the right answer, because the generation
/// names what the view contains, not how it was placed on disk.
///
/// Two files get special treatment. `metadata.json` is excluded because it records
/// the id. The lock (`resolution.lock.toml`) is folded not as raw bytes but as the
/// view with its cosmetic `[properties]` cleared: a `[properties]` table is a
/// human label on the generation, and hashing it would mint a fresh generation on
/// every label edit — the exact thing PRIOR-ART-ACTIONS #9 forbids. Everything
/// else the lock records (the active set, `catalog_index`, the declared and
/// unavailable maps) still contributes, so two genuinely different resolutions
/// remain different generations.
fn hash_tree(staging: &Path, view: &ResolvedView) -> Result<GenerationId> {
    let mut hasher = blake3::Hasher::new();
    // v2: the lock is folded semantically (properties-excluded) rather than as raw
    // bytes, so old-format ids are recomputed rather than silently reused.
    hasher.update(b"aikit-generation-v2\n");
    hasher.update(view.hash.to_string().as_bytes());
    hasher.update(b"\n");

    // Fold the lock's semantic content with cosmetic properties removed.
    let mut identity_view = view.clone();
    identity_view.properties.clear();
    let canonical_lock = toml::to_string_pretty(&identity_view).map_err(|e| {
        AikitError::new(
            "generation.lock_unserializable",
            format!("the resolved view could not be canonicalized for hashing: {e}"),
        )
    })?;
    hasher.update(b"lock\n");
    hasher.update(&(canonical_lock.len() as u64).to_le_bytes());
    hasher.update(canonical_lock.as_bytes());

    let mut files: BTreeMap<String, PathBuf> = BTreeMap::new();
    for entry in walkdir::WalkDir::new(staging)
        .follow_links(true)
        .into_iter()
        .filter_map(std::result::Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(staging)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .replace('\\', "/");
        // metadata.json records the id; the lock is folded semantically above.
        if relative == METADATA_FILE || relative == LOCK_FILE {
            continue;
        }
        files.insert(relative, entry.path().to_path_buf());
    }

    for (relative, path) in files {
        let contents = fs::read(&path).map_err(|e| io_error("generation.unreadable", &path, &e))?;
        hasher.update(&(relative.len() as u64).to_le_bytes());
        hasher.update(relative.as_bytes());
        hasher.update(&(contents.len() as u64).to_le_bytes());
        hasher.update(&contents);
    }

    Ok(GenerationId::from_hash(hasher.finalize()))
}
