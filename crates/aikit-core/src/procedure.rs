//! The Procedure: a named, planned, reviewable, **reversible** mutation of the
//! world outside AIKit's own state directory.
//!
//! Part I gave generations a discipline for everything AIKit owns. Spec II §1
//! applies the same discipline outward, because importing 644 skills, rewriting a
//! client's settings file or adopting a foreign root are not generations — they
//! are mutations of a world that existed before AIKit and will outlive it.
//!
//! ```text
//! generation                     procedure
//! ─────────────────────────────  ──────────────────────────────
//! resolve → plan                 survey → plan
//! content-addressed hash         content-addressed plan digest
//! materialize into a temp dir    stage into an isolation strategy
//! validate before promoting      validate before committing
//! atomic `current` swap          atomic commit (git, or recorded inverse)
//! `previous` retained            undo record retained
//! failed build leaves it intact  failed procedure leaves the world intact
//! ```
//!
//! **The invariant this module owns: the inverse is computed at plan time, not at
//! failure time.** A rollback written during a failure is a rollback written by
//! code that has already demonstrated it does not understand the state it is in.
//! Every [`WorldEdit`] therefore carries the [`Inverse`] that undoes it *before*
//! anything is written, and a plan whose inverse cannot be computed is refused
//! rather than attempted.
//!
//! This module is pure: it plans, digests and validates. Staging, committing and
//! undoing touch the filesystem and live in `aikit-store`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::err;
use crate::id::{CapsuleId, ProcedureId};
use crate::platform::{MuxKind, TargetId};
use crate::{AikitError, Result};

/// The marker that brackets every edit AIKit makes to a file it does not own.
pub const MARKER_BEGIN: &str = ">>> aikit >>>";
pub const MARKER_END: &str = "<<< aikit <<<";

// ---------------------------------------------------------------------------
// Mutation isolation
// ---------------------------------------------------------------------------

/// How a Procedure stages its writes.
///
/// Deliberately **not** [`crate::context::Isolation`], which says where an *agent
/// task* works and defaults to `Shared`. These two point in opposite directions on
/// purpose: an agent doing a review does not need its own checkout, but a procedure
/// restructuring your skill trees does. Mutation isolation defaults to the most
/// isolated option the target supports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "isolation", rename_all = "kebab-case")]
pub enum MutationIsolation {
    /// Target is a git repository: stage on a branch. The default when available.
    GitBranch { repo: PathBuf, branch: String },
    /// A repository, but the change is large or expected to outlive one invocation.
    GitWorktree {
        repo: PathBuf,
        branch: String,
        path: PathBuf,
    },
    /// Not under version control: build a shadow tree, diff it, then swap file by
    /// file with recorded inverses.
    Staged { shadow: PathBuf },
    /// Small, provably reversible, and explicitly confirmed.
    Direct,
}

impl MutationIsolation {
    pub fn as_str(&self) -> &'static str {
        match self {
            MutationIsolation::GitBranch { .. } => "git-branch",
            MutationIsolation::GitWorktree { .. } => "git-worktree",
            MutationIsolation::Staged { .. } => "staged",
            MutationIsolation::Direct => "direct",
        }
    }

    /// Whether this strategy produces a reviewable diff before anything is
    /// committed. Only `Direct` does not.
    pub fn is_reviewable(&self) -> bool {
        !matches!(self, MutationIsolation::Direct)
    }
}

// ---------------------------------------------------------------------------
// Edits and their inverses
// ---------------------------------------------------------------------------

/// A backed-up blob, stored under `state/procedures/<id>/undo/`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BlobId(String);

impl BlobId {
    /// A blob whose identity the **runner** assigns when it actually backs the
    /// file up.
    ///
    /// A planner cannot know this: whether a `WriteFile` overwrites something, and
    /// therefore what has to be preserved, is a fact about the filesystem at stage
    /// time, not at plan time. `ProcedureRunner` inspects the target and records
    /// the real `UndoStep` — `Recreate` for a symlink, `Remove` for a new file,
    /// `Restore` with a freshly-written blob for an overwrite. Declaring this makes
    /// the deferral explicit instead of leaving a placeholder that reads as a bug.
    pub fn deferred() -> Self {
        Self("deferred".to_string())
    }

    pub fn new(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for BlobId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// What undoes one [`WorldEdit`]. Computed at plan time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "inverse", rename_all = "kebab-case")]
pub enum Inverse {
    /// Put back the bytes that were there, from the undo store.
    Restore { blob: BlobId },
    /// There was nothing there; remove what we created.
    Remove,
    /// Recreate a symlink that pointed at `target`.
    Recreate { target: PathBuf },
    /// Nothing to undo. **Only legal when the edit is provably idempotent** — see
    /// [`Plan::validate`], which refuses `Direct` for any other use of it.
    None,
}

impl Inverse {
    pub fn as_str(&self) -> &'static str {
        match self {
            Inverse::Restore { .. } => "restore",
            Inverse::Remove => "remove",
            Inverse::Recreate { .. } => "recreate",
            Inverse::None => "none",
        }
    }
}

/// One reversible edit to the world.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "edit", rename_all = "kebab-case")]
pub enum WorldEdit {
    WriteFile {
        path: PathBuf,
        contents: Vec<u8>,
        inverse: Inverse,
    },
    DeleteFile {
        path: PathBuf,
        inverse: Inverse,
    },
    CreateLink {
        path: PathBuf,
        target: PathBuf,
        inverse: Inverse,
    },
    /// An edit to a file AIKit does not own: idempotent by construction, because
    /// applying it twice replaces the block rather than appending a second one.
    MarkedBlock {
        path: PathBuf,
        marker: String,
        contents: String,
    },
    RunCommand {
        argv: Vec<String>,
        cwd: PathBuf,
        undo: Option<Vec<String>>,
    },
}

impl WorldEdit {
    /// The path this edit touches, when it touches one.
    pub fn path(&self) -> Option<&PathBuf> {
        match self {
            WorldEdit::WriteFile { path, .. }
            | WorldEdit::DeleteFile { path, .. }
            | WorldEdit::CreateLink { path, .. }
            | WorldEdit::MarkedBlock { path, .. } => Some(path),
            WorldEdit::RunCommand { .. } => None,
        }
    }

    /// The inverse recorded for this edit. A marked block is idempotent by
    /// construction and a command carries its own optional undo.
    pub fn inverse(&self) -> Option<&Inverse> {
        match self {
            WorldEdit::WriteFile { inverse, .. }
            | WorldEdit::DeleteFile { inverse, .. }
            | WorldEdit::CreateLink { inverse, .. } => Some(inverse),
            WorldEdit::MarkedBlock { .. } | WorldEdit::RunCommand { .. } => None,
        }
    }

    /// Whether undoing this edit is provable rather than hopeful.
    ///
    /// A marked block qualifies because replacing it is idempotent; a command
    /// qualifies only when it declares an undo.
    pub fn is_provably_reversible(&self) -> bool {
        match self {
            WorldEdit::MarkedBlock { .. } => true,
            WorldEdit::RunCommand { undo, .. } => undo.is_some(),
            other => matches!(
                other.inverse(),
                Some(Inverse::Restore { .. } | Inverse::Remove | Inverse::Recreate { .. })
            ),
        }
    }

    pub fn describe(&self) -> String {
        match self {
            WorldEdit::WriteFile { path, contents, .. } => {
                format!("write {} ({} bytes)", path.display(), contents.len())
            }
            WorldEdit::DeleteFile { path, .. } => format!("delete {}", path.display()),
            WorldEdit::CreateLink { path, target, .. } => {
                format!("link {} -> {}", path.display(), target.display())
            }
            WorldEdit::MarkedBlock { path, marker, .. } => {
                format!("managed block `{marker}` in {}", path.display())
            }
            WorldEdit::RunCommand { argv, cwd, .. } => {
                format!("run `{}` in {}", argv.join(" "), cwd.display())
            }
        }
    }

    /// A stable line for the plan digest.
    fn digest_line(&self) -> String {
        match self {
            WorldEdit::WriteFile { path, contents, .. } => format!(
                "write|{}|{}",
                path.display(),
                blake3::hash(contents).to_hex()
            ),
            WorldEdit::DeleteFile { path, .. } => format!("delete|{}", path.display()),
            WorldEdit::CreateLink { path, target, .. } => {
                format!("link|{}|{}", path.display(), target.display())
            }
            WorldEdit::MarkedBlock {
                path,
                marker,
                contents,
            } => format!(
                "block|{}|{marker}|{}",
                path.display(),
                blake3::hash(contents.as_bytes()).to_hex()
            ),
            WorldEdit::RunCommand { argv, cwd, .. } => {
                format!("run|{}|{}", cwd.display(), argv.join(" "))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Marked blocks
// ---------------------------------------------------------------------------

/// Render a marked block with the given comment leader.
///
/// The leader differs per target (`#` for shell/tmux/TOML, `<!--`/`-->` for
/// markdown), which is why it is a parameter rather than a constant: AGENTS.md and
/// a shell rc file both need a block, and a `#` in markdown is a heading.
pub fn render_marked_block(leader: &str, body: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!("{leader} {MARKER_BEGIN}\n"));
    out.push_str(body);
    if !body.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&format!("{leader} {MARKER_END}\n"));
    out
}

/// Splice a marked block into `existing`, replacing any block already there.
///
/// Idempotent by construction: applying twice replaces, never appends. Human prose
/// outside the markers is never touched. Returns the new contents.
pub fn splice_marked_block(existing: &str, leader: &str, body: &str) -> String {
    let block = render_marked_block(leader, body);
    let begin = format!("{leader} {MARKER_BEGIN}");
    let end = format!("{leader} {MARKER_END}");

    let Some(start) = existing.find(&begin) else {
        // No block yet: append, keeping a blank line between prose and block.
        let mut out = existing.to_string();
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&block);
        return out;
    };

    // A begin marker with no end marker after it means a hand-truncated block;
    // replacing from the begin marker to end-of-file is the honest repair.
    let after = &existing[start..];
    let stop = match after.find(&end) {
        Some(offset) => start + offset + end.len(),
        None => existing.len(),
    };
    let mut out = String::with_capacity(existing.len() + block.len());
    out.push_str(&existing[..start]);
    out.push_str(&block);
    // Preserve whatever followed the block, including its newline.
    let tail = &existing[stop..];
    out.push_str(tail.strip_prefix('\n').unwrap_or(tail));
    out
}

// ---------------------------------------------------------------------------
// The plan
// ---------------------------------------------------------------------------

/// A content hash of a plan. Re-running a satisfied plan is a no-op, and two
/// plans with the same digest do the same thing.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PlanDigest(String);

impl PlanDigest {
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn short(&self) -> &str {
        let end = self.0.char_indices().nth(12).map_or(self.0.len(), |(b, _)| b);
        &self.0[..end]
    }
}

impl std::fmt::Display for PlanDigest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// What a Procedure is for. Each variant names a real front-end: `doctor --fix`,
/// `collate`, `promote`, `client install` and `mux install` are all thin callers
/// over this one engine, so there is one safety story rather than six.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ProcedureKind {
    Import { source: String },
    Collate { sources: Vec<String> },
    Adopt { capsules: Vec<CapsuleId> },
    Promote { candidate: String },
    Supersede { winner: CapsuleId, losers: Vec<CapsuleId> },
    ClientInstall { client: TargetId },
    MuxInstall { mux: MuxKind },
    DoctorFix { checks: Vec<String> },
    IntegrationSetup { integration: String },
    DependencyInstall { tool: CapsuleId },
    Custom { capsule: CapsuleId },
}

impl ProcedureKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProcedureKind::Import { .. } => "import",
            ProcedureKind::Collate { .. } => "collate",
            ProcedureKind::Adopt { .. } => "adopt",
            ProcedureKind::Promote { .. } => "promote",
            ProcedureKind::Supersede { .. } => "supersede",
            ProcedureKind::ClientInstall { .. } => "client-install",
            ProcedureKind::MuxInstall { .. } => "mux-install",
            ProcedureKind::DoctorFix { .. } => "doctor-fix",
            ProcedureKind::IntegrationSetup { .. } => "integration-setup",
            ProcedureKind::DependencyInstall { .. } => "dependency-install",
            ProcedureKind::Custom { .. } => "custom",
        }
    }
}

/// The edits a Procedure intends, computed before anything is written.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub edits: Vec<WorldEdit>,
    /// Human-readable notes: what was surveyed, what was skipped and why.
    #[serde(default)]
    pub notes: Vec<String>,
}

impl Plan {
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_edit(mut self, edit: WorldEdit) -> Self {
        self.edits.push(edit);
        self
    }

    #[must_use]
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    pub fn is_empty(&self) -> bool {
        self.edits.is_empty()
    }

    /// Every path this plan would touch, deduplicated and sorted.
    pub fn touched_paths(&self) -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = self.edits.iter().filter_map(|e| e.path().cloned()).collect();
        paths.sort();
        paths.dedup();
        paths
    }

    /// The content digest: order-independent, content-sensitive. Notes are
    /// excluded — they are commentary, and a reworded note must not change the
    /// identity of a plan that does the same thing.
    pub fn digest(&self) -> PlanDigest {
        let mut lines: Vec<String> = self.edits.iter().map(WorldEdit::digest_line).collect();
        lines.sort();
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"aikit-procedure-plan-v1\n");
        for line in lines {
            hasher.update(&(line.len() as u64).to_le_bytes());
            hasher.update(line.as_bytes());
        }
        PlanDigest(hasher.finalize().to_hex().to_string())
    }

    /// Check the plan against the rule that makes a Procedure safe.
    ///
    /// `Direct` is refused entirely for any plan containing an edit whose inverse
    /// is `None` and which is not a marked block (Spec II §1.2): those are exactly
    /// the edits nothing could undo, and they may not be applied in place.
    pub fn validate(&self, isolation: &MutationIsolation) -> Result<()> {
        if !matches!(isolation, MutationIsolation::Direct) {
            return Ok(());
        }
        for edit in &self.edits {
            if !edit.is_provably_reversible() {
                return Err(AikitError::new(
                    "procedure.not_reversible",
                    format!(
                        "`{}` cannot be applied directly because it is not provably \
                         reversible; stage it on a branch or in a shadow tree instead",
                        edit.describe()
                    ),
                )
                .with("edit", edit.describe())
                .with("isolation", isolation.as_str()));
            }
        }
        Ok(())
    }
}

/// A planned mutation of the world outside AIKit's own state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Procedure {
    pub id: ProcedureId,
    pub kind: ProcedureKind,
    pub plan: Plan,
    pub isolation: MutationIsolation,
    pub digest: PlanDigest,
}

impl Procedure {
    /// Assemble a procedure, computing its digest and refusing a plan its isolation
    /// cannot safely carry.
    pub fn new(kind: ProcedureKind, plan: Plan, isolation: MutationIsolation) -> Result<Self> {
        plan.validate(&isolation)?;
        let digest = plan.digest();
        Ok(Self {
            id: ProcedureId::generate(),
            kind,
            plan,
            isolation,
            digest,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.plan.is_empty()
    }
}

/// Choose the isolation for a plan, per Spec II §1.2's ordered rule.
///
/// 1. Every touched path inside one git repository → `GitBranch`.
/// 2. Otherwise → `Staged` (a shadow tree, a real diff, recorded inverses).
/// 3. `Direct` is never chosen automatically; it requires explicit confirmation.
///
/// `repo_of` is injected rather than discovered here because this crate does no
/// I/O: the store passes a closure that runs the real repository lookup.
pub fn select_isolation<F>(plan: &Plan, shadow_root: &std::path::Path, repo_of: F) -> MutationIsolation
where
    F: Fn(&std::path::Path) -> Option<PathBuf>,
{
    let paths = plan.touched_paths();
    // Every path must resolve to the *same* repository. A path outside version
    // control (`None`) disqualifies the whole plan: a branch cannot stage an edit
    // the repository does not contain.
    let repos: Option<Vec<PathBuf>> = paths.iter().map(|p| repo_of(p)).collect();
    if let Some(repos) = repos {
        if let Some(first) = repos.first() {
            if repos.iter().all(|r| r == first) {
                return MutationIsolation::GitBranch {
                    repo: first.clone(),
                    branch: format!("aikit/{}", plan.digest().short()),
                };
            }
        }
    }
    MutationIsolation::Staged {
        shadow: shadow_root.join(plan.digest().short()),
    }
}

/// The record kept so a committed Procedure can be undone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndoRecord {
    pub procedure: ProcedureId,
    pub digest: PlanDigest,
    /// The inverses, in the order they must be applied — the reverse of the order
    /// the edits were made, so a later edit's undo cannot be undone by an earlier
    /// one's.
    pub steps: Vec<UndoStep>,
}

/// One recorded inverse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndoStep {
    pub path: Option<PathBuf>,
    pub inverse: Inverse,
    /// What the forward edit was, so the undo report reads as prose.
    pub description: String,
}

impl UndoStep {
    pub fn new(path: Option<PathBuf>, inverse: Inverse, description: impl Into<String>) -> Self {
        Self {
            path,
            inverse,
            description: description.into(),
        }
    }
}

/// Fidelity: what a projection had to drop or degrade to fit a target.
///
/// Spec II §4: projecting a rich capsule onto a poor target loses fields, and that
/// loss is recorded rather than hidden. `aikit explain` prints it, so "why does
/// this behave differently in Codex" has an answer instead of a guess.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fidelity {
    pub target: Option<TargetId>,
    /// (field, why it was dropped)
    #[serde(default)]
    pub dropped: Vec<(String, String)>,
    /// (field, how it was degraded)
    #[serde(default)]
    pub degraded: Vec<(String, String)>,
}

impl Fidelity {
    pub fn for_target(target: TargetId) -> Self {
        Self {
            target: Some(target),
            ..Default::default()
        }
    }

    #[must_use]
    pub fn dropping(mut self, field: impl Into<String>, why: impl Into<String>) -> Self {
        self.dropped.push((field.into(), why.into()));
        self
    }

    #[must_use]
    pub fn degrading(mut self, field: impl Into<String>, how: impl Into<String>) -> Self {
        self.degraded.push((field.into(), how.into()));
        self
    }

    pub fn is_faithful(&self) -> bool {
        self.dropped.is_empty() && self.degraded.is_empty()
    }

    /// The lines `aikit explain` prints.
    pub fn render(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (field, why) in &self.dropped {
            out.push(format!("dropped `{field}`: {why}"));
        }
        for (field, how) in &self.degraded {
            out.push(format!("degraded `{field}`: {how}"));
        }
        out
    }
}

/// Where a capsule field's value came from when it was lifted from a foreign
/// schema (Spec II §4).
///
/// The point is `Absent`: lifting a Claude skill produces a capsule whose
/// `version`, `platforms` and `related_skills` are **visibly absent**, not
/// silently defaulted. The palette shows it, `doctor` counts it, and promotion is
/// where a human fills it in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "origin", rename_all = "kebab-case")]
pub enum FieldOrigin {
    Declared,
    Inferred { from: String },
    Defaulted,
    Absent,
}

impl FieldOrigin {
    pub fn as_str(&self) -> &'static str {
        match self {
            FieldOrigin::Declared => "declared",
            FieldOrigin::Inferred { .. } => "inferred",
            FieldOrigin::Defaulted => "defaulted",
            FieldOrigin::Absent => "absent",
        }
    }

    /// Whether a human still needs to supply this field.
    pub fn needs_a_human(&self) -> bool {
        matches!(self, FieldOrigin::Absent)
    }
}

/// The provenance of every lifted field, by field name.
pub type FieldOrigins = BTreeMap<String, FieldOrigin>;

/// Count the fields that are visibly absent, for `doctor` and the palette.
pub fn absent_fields(origins: &FieldOrigins) -> Vec<&str> {
    origins
        .iter()
        .filter(|(_, o)| o.needs_a_human())
        .map(|(k, _)| k.as_str())
        .collect()
}

/// Who owns a registry AIKit indexes.
///
/// Import is read-only and always safe; **adoption is a Procedure**, because it
/// moves authority: afterwards the original path is regenerated from the capsule
/// rather than edited by hand, and a broken link becomes a `doctor` finding rather
/// than a silent failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "ownership", rename_all = "kebab-case")]
pub enum RegistryOwnership {
    /// AIKit's own registries. Writable.
    Owned,
    /// Indexed, read-only: `~/.claude/skills`, a plugin cache.
    Foreign,
    /// Was foreign; a human ran an Adopt procedure. AIKit owns it now and the
    /// original location becomes a projection.
    Adopted {
        adopted_at: String,
        procedure: ProcedureId,
    },
}

impl RegistryOwnership {
    pub fn as_str(&self) -> &'static str {
        match self {
            RegistryOwnership::Owned => "owned",
            RegistryOwnership::Foreign => "foreign",
            RegistryOwnership::Adopted { .. } => "adopted",
        }
    }

    /// Whether AIKit may write into this registry.
    pub fn is_writable(&self) -> bool {
        matches!(
            self,
            RegistryOwnership::Owned | RegistryOwnership::Adopted { .. }
        )
    }
}

/// Refuse a write to a registry AIKit does not own, naming the fix.
pub fn require_writable(ownership: &RegistryOwnership, registry: &str) -> Result<()> {
    if ownership.is_writable() {
        return Ok(());
    }
    err(
        "registry.foreign_is_read_only",
        format!(
            "`{registry}` is a foreign registry: AIKit indexes it but does not own it. \
             Adopt it first (`aikit procedure plan adopt`) — adoption is a reviewable, \
             reversible Procedure that moves authority deliberately."
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_marked_block_replaces_rather_than_appends() {
        let leader = "#";
        let first = splice_marked_block("# my config\nset -g mouse on\n", leader, "aikit line 1");
        assert_eq!(first.matches(MARKER_BEGIN).count(), 1);
        assert!(first.contains("set -g mouse on"), "human prose survives");

        let second = splice_marked_block(&first, leader, "aikit line 2");
        assert_eq!(
            second.matches(MARKER_BEGIN).count(),
            1,
            "applying twice replaces, never appends"
        );
        assert!(second.contains("aikit line 2"));
        assert!(!second.contains("aikit line 1"));
        assert!(second.contains("set -g mouse on"), "prose still survives");
    }

    #[test]
    fn a_marked_block_preserves_prose_after_it() {
        let existing = "before\n\n# >>> aikit >>>\nold\n# <<< aikit <<<\nafter\n";
        let spliced = splice_marked_block(existing, "#", "new");
        assert!(spliced.contains("before"));
        assert!(spliced.contains("after"), "the tail survives: {spliced}");
        assert!(spliced.contains("new"));
        assert!(!spliced.contains("old"));
    }

    #[test]
    fn direct_isolation_refuses_an_edit_that_cannot_be_undone() {
        let plan = Plan::new().with_edit(WorldEdit::WriteFile {
            path: PathBuf::from("/etc/thing"),
            contents: b"x".to_vec(),
            inverse: Inverse::None,
        });
        let error = plan.validate(&MutationIsolation::Direct).unwrap_err();
        assert_eq!(error.code(), "procedure.not_reversible");

        // The same plan is fine when staged, because the shadow tree is the undo.
        assert!(plan
            .validate(&MutationIsolation::Staged {
                shadow: PathBuf::from("/tmp/shadow")
            })
            .is_ok());
    }

    #[test]
    fn direct_isolation_accepts_a_marked_block_because_it_is_idempotent() {
        let plan = Plan::new().with_edit(WorldEdit::MarkedBlock {
            path: PathBuf::from("/home/me/.tmux.conf"),
            marker: "aikit".to_string(),
            contents: "set -g @aikit 1".to_string(),
        });
        assert!(plan.validate(&MutationIsolation::Direct).is_ok());
    }

    #[test]
    fn a_plan_digest_is_order_independent_but_content_sensitive() {
        let a = WorldEdit::WriteFile {
            path: PathBuf::from("/a"),
            contents: b"one".to_vec(),
            inverse: Inverse::Remove,
        };
        let b = WorldEdit::DeleteFile {
            path: PathBuf::from("/b"),
            inverse: Inverse::Restore {
                blob: BlobId::new("blob1"),
            },
        };
        let forward = Plan::new().with_edit(a.clone()).with_edit(b.clone());
        let backward = Plan::new().with_edit(b).with_edit(a);
        assert_eq!(forward.digest(), backward.digest(), "order must not matter");

        let different = Plan::new().with_edit(WorldEdit::WriteFile {
            path: PathBuf::from("/a"),
            contents: b"two".to_vec(),
            inverse: Inverse::Remove,
        });
        assert_ne!(forward.digest(), different.digest(), "content must matter");
    }

    #[test]
    fn a_note_does_not_change_a_plans_identity() {
        let plan = Plan::new().with_edit(WorldEdit::DeleteFile {
            path: PathBuf::from("/x"),
            inverse: Inverse::Remove,
        });
        let annotated = plan.clone().with_note("surveyed 13 roots");
        assert_eq!(plan.digest(), annotated.digest());
    }

    #[test]
    fn isolation_selection_prefers_a_branch_when_every_path_is_in_one_repo() {
        let plan = Plan::new()
            .with_edit(WorldEdit::DeleteFile {
                path: PathBuf::from("/repo/a"),
                inverse: Inverse::Remove,
            })
            .with_edit(WorldEdit::DeleteFile {
                path: PathBuf::from("/repo/b"),
                inverse: Inverse::Remove,
            });
        let isolation = select_isolation(&plan, std::path::Path::new("/shadow"), |_| {
            Some(PathBuf::from("/repo"))
        });
        assert!(matches!(isolation, MutationIsolation::GitBranch { .. }));

        // Spanning two repositories cannot be one branch, so it stages.
        let staged = select_isolation(&plan, std::path::Path::new("/shadow"), |p| {
            Some(PathBuf::from(if p.ends_with("a") { "/repo1" } else { "/repo2" }))
        });
        assert!(matches!(staged, MutationIsolation::Staged { .. }));

        // Nothing under version control at all stages too.
        let none = select_isolation(&plan, std::path::Path::new("/shadow"), |_| None);
        assert!(matches!(none, MutationIsolation::Staged { .. }));
    }

    #[test]
    fn a_foreign_registry_refuses_a_write_and_names_the_fix() {
        let error = require_writable(&RegistryOwnership::Foreign, "@claude").unwrap_err();
        assert_eq!(error.code(), "registry.foreign_is_read_only");
        assert!(error.message().contains("Adopt"), "the fix is named");

        assert!(require_writable(&RegistryOwnership::Owned, "personal").is_ok());
        assert!(require_writable(
            &RegistryOwnership::Adopted {
                adopted_at: "2026-07-24T00:00:00Z".to_string(),
                procedure: ProcedureId::generate(),
            },
            "@claude"
        )
        .is_ok());
    }

    #[test]
    fn a_lifted_field_is_visibly_absent_rather_than_silently_defaulted() {
        let mut origins = FieldOrigins::new();
        origins.insert("name".to_string(), FieldOrigin::Declared);
        origins.insert("version".to_string(), FieldOrigin::Absent);
        origins.insert("platforms".to_string(), FieldOrigin::Absent);
        origins.insert(
            "kind".to_string(),
            FieldOrigin::Inferred {
                from: "the shebang".to_string(),
            },
        );

        let absent = absent_fields(&origins);
        assert_eq!(absent, vec!["platforms", "version"]);
        assert!(!FieldOrigin::Declared.needs_a_human());
        assert!(FieldOrigin::Absent.needs_a_human());
    }

    #[test]
    fn fidelity_records_what_a_target_could_not_carry() {
        let fidelity = Fidelity::for_target(TargetId::codex())
            .dropping("related_skills", "Codex has no field for it")
            .degrading("description", "truncated to the first sentence");
        assert!(!fidelity.is_faithful());
        let rendered = fidelity.render();
        assert_eq!(rendered.len(), 2);
        assert!(rendered[0].contains("related_skills"));
        assert!(Fidelity::default().is_faithful());
    }
}
