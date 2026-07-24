//! Target projection contracts: pure data plus a planning trait.
//!
//! A projection is a target-specific representation of an effective view. This
//! module describes *what* a target would end up with; putting the bytes on disk
//! is [`aikit-store`]'s job. The split is not tidiness for its own sake — it is
//! what makes the two properties below testable before any file handle exists.
//!
//! ## A plan cannot escape its root
//!
//! Every destination is relative and is validated at construction. A capsule that
//! names `../../.ssh/authorized_keys` is refused by [`ProjectionItem::link`], not
//! by a check somewhere in the writer that a later refactor might drop.
//!
//! ## A plan is content-addressed
//!
//! [`ProjectionPlan::digest`] hashes what the plan would produce, insensitive to
//! the order items were added but sensitive to written contents. That is what
//! makes a no-op apply a comparison instead of a rebuild, and it is why the
//! activation effect is deliberately *not* part of the digest: the effect is
//! computed by comparing plans, so including it would be circular.
//!
//! ## Isolation is asked about, never assumed
//!
//! [`TargetCapabilities::requires_isolated_tree_for_isolation`] is how a target
//! says "I can only give this context its own skill surface if the task has its
//! own working tree". When it cannot, [`TargetCapabilities::fallback_reason`]
//! produces the sentence the user is owed, and the adapter reports a fallback
//! [`ActivationEffect`] rather than writing into a tree its siblings share.
//!
//! [`aikit-store`]: https://docs.rs/aikit-store

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::context::Isolation;
use crate::error::{err, AikitError, Result};
use crate::id::CapsuleId;
use crate::platform::TargetId;
use crate::resolve::ResolvedView;

// ---------------------------------------------------------------------------
// Target capabilities
// ---------------------------------------------------------------------------

/// What a projection target can actually do.
///
/// Deliberately all-false by default: a new adapter that forgets to describe
/// itself gets the most conservative treatment rather than an optimistic one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetCapabilities {
    /// The client notices a changed projection without being restarted.
    pub live_reload: bool,
    /// Symlinks work here. Rules out some filesystems and some sandboxes.
    pub symlinks: bool,
    /// The client can be pointed at a per-context directory at all.
    pub isolated_per_context: bool,
    /// Per-context isolation only works when the *task* has its own working tree,
    /// because the client insists on a fixed project-relative path. This is the
    /// Codex shape: `.agents/skills` lives in the tree, so two shared-tree tasks
    /// would overwrite each other.
    pub requires_isolated_tree_for_isolation: bool,
    /// A broker skill (`aikit capabilities list|read`, `aikit run`) is available
    /// as a fallback when a native projection is impossible.
    pub brokered_fallback: bool,
    /// The client watches the projection directory rather than reading it once.
    pub watches_for_changes: bool,
}

impl TargetCapabilities {
    /// Can this target give *this* context a surface no sibling context sees?
    pub fn can_isolate(&self, isolation: Isolation) -> bool {
        if !self.isolated_per_context {
            return false;
        }
        if self.requires_isolated_tree_for_isolation {
            return isolation.is_isolated();
        }
        true
    }

    /// The sentence to show when a native per-context projection is impossible.
    ///
    /// `None` when it *is* possible. Returning a reason rather than a bare `false`
    /// is the point: "Codex: brokered" without a why is exactly the kind of
    /// unexplained degradation that makes people stop trusting a tool.
    pub fn fallback_reason(&self, isolation: Isolation) -> Option<String> {
        if self.can_isolate(isolation) {
            return None;
        }
        if !self.isolated_per_context {
            return Some(
                "this client cannot be pointed at a context-specific directory".to_string(),
            );
        }
        Some(format!(
            "this task uses the session's shared working tree ({}), and this client's skill \
             directory lives in the tree, so a sibling task would see the same files",
            isolation.as_str()
        ))
    }
}

// ---------------------------------------------------------------------------
// Activation effects
// ---------------------------------------------------------------------------

/// What actually happens to a client when a projection is applied.
///
/// "Active in AIKit" must never be allowed to imply "already loaded by every
/// client", so every variant that is not immediate carries the reason it is not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "effect", rename_all = "kebab-case")]
pub enum ActivationEffect {
    /// In effect now. `via` names the mechanism, so the palette can print
    /// something specific ("task worktree") rather than a generic "immediate".
    Immediate { via: String },
    /// Written; the client is expected to pick it up on its own.
    LiveReloadExpected,
    RestartClient { client: String },
    NextSessionOnly { reason: String },
    Brokered { reason: String },
    Unsupported { reason: String },
}

impl ActivationEffect {
    pub fn immediate(via: impl Into<String>) -> Self {
        Self::Immediate { via: via.into() }
    }

    pub fn live() -> Self {
        Self::LiveReloadExpected
    }

    pub fn restart_client(client: impl Into<String>) -> Self {
        Self::RestartClient {
            client: client.into(),
        }
    }

    pub fn next_session_only(reason: impl Into<String>) -> Self {
        Self::NextSessionOnly {
            reason: reason.into(),
        }
    }

    pub fn brokered(reason: impl Into<String>) -> Self {
        Self::Brokered {
            reason: reason.into(),
        }
    }

    pub fn unsupported(reason: impl Into<String>) -> Self {
        Self::Unsupported {
            reason: reason.into(),
        }
    }

    /// The short phrase the palette prints after the target's name.
    pub fn describe(&self) -> String {
        match self {
            ActivationEffect::Immediate { via } if via.is_empty() => "immediate".to_string(),
            ActivationEffect::Immediate { via } => via.clone(),
            ActivationEffect::LiveReloadExpected => "live".to_string(),
            ActivationEffect::RestartClient { client } => format!("restart {client}"),
            ActivationEffect::NextSessionOnly { reason } => {
                format!("next session only — {reason}")
            }
            ActivationEffect::Brokered { reason } => format!("brokered — {reason}"),
            ActivationEffect::Unsupported { reason } => format!("unsupported — {reason}"),
        }
    }

    /// The full line, e.g. `Claude: live` or `Codex: task worktree`.
    pub fn describe_for(&self, target: &TargetId) -> String {
        format!("{}: {}", target_label(target), self.describe())
    }

    /// Is the capability usable in the running client right now?
    pub fn takes_effect_now(&self) -> bool {
        matches!(
            self,
            ActivationEffect::Immediate { .. } | ActivationEffect::LiveReloadExpected
        )
    }

    /// Does the user have to do something before this takes effect?
    pub fn needs_user_action(&self) -> bool {
        matches!(self, ActivationEffect::RestartClient { .. })
    }
}

impl std::fmt::Display for ActivationEffect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.describe())
    }
}

/// The human name for a target, for palette lines like `Claude: live`.
///
/// Unknown targets are title-cased rather than rejected: target ids are
/// open-ended so third-party adapters can name themselves without a core release.
pub fn target_label(target: &TargetId) -> String {
    match target.as_str() {
        TargetId::CLAUDE_CODE => "Claude".to_string(),
        TargetId::CODEX => "Codex".to_string(),
        TargetId::SHELL => "Shell".to_string(),
        TargetId::AGENT_SKILLS => "Agent skills".to_string(),
        TargetId::HOOKS => "Hooks".to_string(),
        TargetId::GUIDANCE => "Guidance".to_string(),
        other => {
            let spaced = other.replace(['-', '_'], " ");
            let mut chars = spaced.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Projection items
// ---------------------------------------------------------------------------

/// One thing a projection would create.
///
/// Construct through the validating constructors; the fields are public so
/// adapters and the store can match on them, but there is no way to build one
/// with a destination that escapes the projection root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "item", rename_all = "kebab-case")]
pub enum ProjectionItem {
    /// Symlink an existing payload into place.
    Link { from: PathBuf, to: PathBuf },
    /// Copy it, for targets or filesystems where a symlink will not do.
    Copy { from: PathBuf, to: PathBuf },
    /// Generate a file. The contents are part of the plan's identity.
    Write { path: PathBuf, contents: String },
    /// A generated command shim on the contextual PATH. The adapter picks the
    /// directory, so there is no destination to validate — only the name.
    Shim {
        name: String,
        capsule: CapsuleId,
        export: String,
    },
}

impl ProjectionItem {
    pub fn link(from: impl Into<PathBuf>, to: impl AsRef<Path>) -> Result<Self> {
        Ok(Self::Link {
            from: from.into(),
            to: relative_destination(to.as_ref())?,
        })
    }

    pub fn copy(from: impl Into<PathBuf>, to: impl AsRef<Path>) -> Result<Self> {
        Ok(Self::Copy {
            from: from.into(),
            to: relative_destination(to.as_ref())?,
        })
    }

    pub fn write(path: impl AsRef<Path>, contents: impl Into<String>) -> Result<Self> {
        Ok(Self::Write {
            path: relative_destination(path.as_ref())?,
            contents: contents.into(),
        })
    }

    pub fn shim(
        name: impl Into<String>,
        capsule: CapsuleId,
        export: impl Into<String>,
    ) -> Result<Self> {
        let name = name.into();
        if name.is_empty()
            || name == "."
            || name == ".."
            || name.contains(['/', '\\'])
            || name.contains('\0')
        {
            return err(
                "projection.invalid_shim_name",
                format!(
                    "`{name}` is not a usable command name; a shim becomes a file in the \
                     contextual bin directory"
                ),
            );
        }
        Ok(Self::Shim {
            name,
            capsule,
            export: export.into(),
        })
    }

    /// The projection-root-relative path this item creates, when it has one.
    pub fn destination(&self) -> Option<&Path> {
        match self {
            ProjectionItem::Link { to, .. } | ProjectionItem::Copy { to, .. } => Some(to),
            ProjectionItem::Write { path, .. } => Some(path),
            ProjectionItem::Shim { .. } => None,
        }
    }

    /// The canonical encoding folded into [`ProjectionPlan::digest`].
    ///
    /// A `Write` contributes a hash of its contents rather than the contents
    /// themselves, so the digest stays short and a changed byte still moves it.
    fn digest_line(&self) -> String {
        match self {
            ProjectionItem::Link { from, to } => {
                format!("link|{}|{}", from.display(), to.display())
            }
            ProjectionItem::Copy { from, to } => {
                format!("copy|{}|{}", from.display(), to.display())
            }
            ProjectionItem::Write { path, contents } => format!(
                "write|{}|{}",
                path.display(),
                blake3::hash(contents.as_bytes()).to_hex()
            ),
            ProjectionItem::Shim {
                name,
                capsule,
                export,
            } => format!("shim|{name}|{capsule}|{export}"),
        }
    }
}

/// Normalize a destination and refuse anything that leaves the projection root.
///
/// `.` segments are dropped; `..`, absolute paths and Windows prefixes are
/// errors. Normalizing rather than rejecting `./x` keeps hand-written manifests
/// forgiving without weakening the boundary.
fn relative_destination(path: &Path) -> Result<PathBuf> {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(segment) => out.push(segment),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(AikitError::new(
                    "projection.destination_escapes_root",
                    format!(
                        "`{}` is not inside the projection root; a projection may only write \
                         below its own directory",
                        path.display()
                    ),
                )
                .with("destination", path.display().to_string()));
            }
        }
    }
    if out.as_os_str().is_empty() {
        return Err(AikitError::new(
            "projection.invalid_destination",
            format!("`{}` names no file", path.display()),
        )
        .with("destination", path.display().to_string()));
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Plans
// ---------------------------------------------------------------------------

/// Everything one target would end up with, plus what applying it would mean.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectionPlan {
    pub target: TargetId,
    pub items: Vec<ProjectionItem>,
    /// Human notes for the palette: which fallback was taken, and why.
    pub notes: Vec<String>,
    pub effect: ActivationEffect,
}

impl ProjectionPlan {
    pub fn new(target: TargetId, effect: ActivationEffect) -> Self {
        Self {
            target,
            items: Vec::new(),
            notes: Vec::new(),
            effect,
        }
    }

    #[must_use]
    pub fn with_item(mut self, item: ProjectionItem) -> Self {
        self.items.push(item);
        self
    }

    #[must_use]
    pub fn with_items(mut self, items: impl IntoIterator<Item = ProjectionItem>) -> Self {
        self.items.extend(items);
        self
    }

    #[must_use]
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    #[must_use]
    pub fn with_effect(mut self, effect: ActivationEffect) -> Self {
        self.effect = effect;
        self
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// A content hash of what this plan would produce.
    ///
    /// Order-independent: the item encodings are sorted before hashing, so an
    /// adapter that iterates a different map next release does not invalidate
    /// every generation. Content-sensitive for `Write`, because a generated file
    /// whose bytes changed is a different projection even at the same path.
    ///
    /// Excludes `notes` and `effect`: both are commentary about the plan rather
    /// than part of what lands on disk, and the effect is derived by comparing
    /// digests in the first place.
    pub fn digest(&self) -> String {
        let mut lines: Vec<String> = self.items.iter().map(|i| i.digest_line()).collect();
        lines.sort();

        let mut hasher = blake3::Hasher::new();
        hasher.update(b"aikit-projection-v1\n");
        hasher.update(self.target.as_str().as_bytes());
        hasher.update(b"\n");
        for line in lines {
            hasher.update(line.as_bytes());
            hasher.update(b"\n");
        }
        hasher.finalize().to_hex().to_string()
    }

    /// Would applying this change anything?
    ///
    /// `None` for `previous` means there is no generation to compare against, so
    /// the answer is no — a first apply always has work to do.
    pub fn is_noop_against(&self, previous: Option<&ProjectionPlan>) -> bool {
        previous.is_some_and(|old| old.digest() == self.digest())
    }
}

// ---------------------------------------------------------------------------
// Adapters
// ---------------------------------------------------------------------------

/// A resolved view plus where each capsule's files live.
///
/// The roots are supplied by the store: core does not know a registry path and
/// must not guess one.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedContext {
    pub view: ResolvedView,
    pub capsule_roots: BTreeMap<CapsuleId, PathBuf>,
}

impl ResolvedContext {
    pub fn new(view: ResolvedView) -> Self {
        Self {
            view,
            capsule_roots: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn with_root(mut self, capsule: CapsuleId, root: impl Into<PathBuf>) -> Self {
        self.capsule_roots.insert(capsule, root.into());
        self
    }

    pub fn root_of(&self, capsule: &CapsuleId) -> Option<&Path> {
        self.capsule_roots.get(capsule).map(PathBuf::as_path)
    }

    /// A path inside a capsule's own directory, or `None` when the store did not
    /// supply a root for it.
    pub fn payload_path(&self, capsule: &CapsuleId, relative: impl AsRef<Path>) -> Option<PathBuf> {
        self.root_of(capsule).map(|root| root.join(relative))
    }

    /// The single question adapters ask before assuming a per-task surface.
    pub fn isolation(&self) -> Isolation {
        self.view.context.isolation
    }
}

/// Plans a projection for one target. **Planning only.**
///
/// Object-safe on purpose: the CLI holds a `Vec<Box<dyn TargetAdapter>>` and
/// walks it. Materialization is I/O and lives in `aikit-store`, which is what
/// keeps a failed projection from ever replacing a working generation — the plan
/// is built and validated in full before anything is written.
pub trait TargetAdapter {
    fn target(&self) -> TargetId;

    fn capabilities(&self) -> TargetCapabilities;

    /// What this target would get for this context.
    fn plan(&self, context: &ResolvedContext) -> Result<ProjectionPlan>;

    /// What applying `new` would actually mean, given what is already in place.
    ///
    /// Separate from `plan` because the honest answer depends on the previous
    /// generation: the same plan is `Immediate` when nothing changed and
    /// `RestartClient` when it did.
    fn activation_effect(
        &self,
        old: Option<&ProjectionPlan>,
        new: &ProjectionPlan,
    ) -> ActivationEffect;
}

// ---------------------------------------------------------------------------
// Materialization
// ---------------------------------------------------------------------------

/// How payloads should be placed into a projection.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MaterializationMode {
    /// Link where possible, copy where not. The default.
    #[default]
    Auto,
    Link,
    Copy,
}

impl MaterializationMode {
    pub fn as_str(self) -> &'static str {
        match self {
            MaterializationMode::Auto => "auto",
            MaterializationMode::Link => "link",
            MaterializationMode::Copy => "copy",
        }
    }

    /// The concrete mode for a target: always `Link` or `Copy`, never `Auto`.
    ///
    /// An explicit `Link` on a target without symlink support becomes a `Copy`
    /// rather than a failure — the user asked for the cheap option, not for the
    /// projection to be impossible — but [`Self::degrades_for`] reports that it
    /// happened so the palette can say so.
    pub fn resolve_for(self, capabilities: &TargetCapabilities) -> MaterializationMode {
        match self {
            MaterializationMode::Copy => MaterializationMode::Copy,
            MaterializationMode::Auto | MaterializationMode::Link => {
                if capabilities.symlinks {
                    MaterializationMode::Link
                } else {
                    MaterializationMode::Copy
                }
            }
        }
    }

    /// True when an explicit request could not be honoured.
    pub fn degrades_for(self, capabilities: &TargetCapabilities) -> bool {
        self == MaterializationMode::Link && !capabilities.symlinks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_target_label_falls_back_to_title_case_for_third_party_adapters() {
        assert_eq!(target_label(&TargetId::new("my-editor")), "My editor");
        assert_eq!(target_label(&TargetId::claude_code()), "Claude");
    }

    #[test]
    fn a_default_target_admits_it_can_do_nothing() {
        let caps = TargetCapabilities::default();
        assert!(!caps.can_isolate(Isolation::Worktree));
        assert!(!caps.live_reload);
        assert!(!caps.symlinks);
        assert_eq!(
            MaterializationMode::Auto.resolve_for(&caps),
            MaterializationMode::Copy
        );
    }

    #[test]
    fn an_empty_plan_still_has_a_stable_digest() {
        let a = ProjectionPlan::new(TargetId::shell(), ActivationEffect::live());
        let b = ProjectionPlan::new(TargetId::shell(), ActivationEffect::live());
        assert_eq!(a.digest(), b.digest());
        assert!(a.is_empty());
    }

    #[test]
    fn two_shims_for_different_capsules_are_different_plans() {
        let one = ProjectionPlan::new(TargetId::shell(), ActivationEffect::live()).with_item(
            ProjectionItem::shim("nt", CapsuleId::parse("script/a/one").unwrap(), "nt").unwrap(),
        );
        let two = ProjectionPlan::new(TargetId::shell(), ActivationEffect::live()).with_item(
            ProjectionItem::shim("nt", CapsuleId::parse("script/a/two").unwrap(), "nt").unwrap(),
        );
        assert_ne!(one.digest(), two.digest());
    }

    #[test]
    fn an_immediate_effect_with_no_mechanism_still_reads_sensibly() {
        assert_eq!(ActivationEffect::immediate("").describe(), "immediate");
        assert_eq!(
            ActivationEffect::immediate("").to_string(),
            "immediate",
            "Display and describe must not diverge"
        );
    }
}
