//! Deterministic capability resolution.
//!
//! Given a catalog, a trust oracle, a stack of scope layers and a managed policy,
//! produce the effective capability view for one context — plus an explanation for
//! every decision and a content hash for the whole thing.
//!
//! The seven rules the resolver implements, in the specification's words:
//!
//! 1. Later layers may undo earlier ordinary enable or disable operations.
//! 2. Managed denials cannot be overridden.
//! 3. Dependencies are expanded after explicit selection.
//! 4. If a required dependency has been explicitly disabled, resolution fails
//!    rather than silently re-enabling it.
//! 5. Conflicts fail visibly by default.
//! 6. No capability becomes active merely because it matches a tag query.
//! 7. Every final decision is explainable.
//!
//! Rules 4 and 5 are the ones that make the system trustworthy: a user's explicit
//! decision is never quietly reversed, and an incoherent request never produces a
//! half-working view.

mod explain;
mod hash;

pub use explain::Explanation;
pub use hash::{resolution_hash, ResolutionHash};

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::capsule::{Capsule, Kind, Maturity};
use crate::catalog::Catalog;
use crate::context::ContextDescriptor;
use crate::error::AikitError;
use crate::id::{CapsuleId, ProfileId, RegistrySource, Revision};
use crate::platform::TargetId;
use crate::policy::ManagedPolicy;
use crate::profile::{combine_config, ConfigTable, PoolPatch};
use crate::scope::{LayerOrigin, ScopeKind, ScopeLayer};
use crate::trust::{TrustOracle, TrustState};

// ---------------------------------------------------------------------------
// Request
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct ResolveRequest {
    pub context: ContextDescriptor,
    /// Supplied in any order; the resolver sorts by precedence.
    pub layers: Vec<ScopeLayer>,
    pub policy: ManagedPolicy,
}

// ---------------------------------------------------------------------------
// Decisions
// ---------------------------------------------------------------------------

/// Why a capability entered the selection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum SelectionOrigin {
    Layer {
        scope: ScopeKind,
        origin: LayerOrigin,
        via_profile: Option<ProfileId>,
    },
    Dependency {
        required_by: CapsuleId,
    },
    Policy {
        source: String,
    },
}

impl SelectionOrigin {
    pub fn describe(&self) -> String {
        match self {
            SelectionOrigin::Layer {
                scope,
                origin,
                via_profile,
            } => match via_profile {
                Some(p) => format!("{scope} {origin} via {p}"),
                None => format!("{scope} {origin}"),
            },
            SelectionOrigin::Dependency { required_by } => {
                format!("required by {required_by}")
            }
            SelectionOrigin::Policy { source } => format!("managed policy {source}"),
        }
    }
}

/// One enable/disable operation recorded during layer folding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SelectionOp {
    pub capsule: CapsuleId,
    pub enable: bool,
    pub scope: ScopeKind,
    pub origin: LayerOrigin,
    pub via_profile: Option<ProfileId>,
}

impl SelectionOp {
    pub fn describe(&self) -> String {
        let verb = if self.enable { "enabled" } else { "disabled" };
        match &self.via_profile {
            Some(p) => format!("{verb} by {} {} via {p}", self.scope, self.origin),
            None => format!("{verb} by {} {}", self.scope, self.origin),
        }
    }
}

/// The declared state of a capability after folding all layers, before any
/// availability check. The palette shows this alongside the effective state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeclaredState {
    pub enabled: bool,
    pub scope: ScopeKind,
    pub origin: LayerOrigin,
    pub via_profile: Option<ProfileId>,
}

/// Why a declared-enabled capability is nevertheless not in the effective view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "kebab-case")]
pub enum UnavailableReason {
    NotInCatalog,
    DeniedByPolicy,
    PlatformUnsupported,
    NoSupportedTarget,
    /// The revision has not been reviewed, and this kind changes agent behaviour.
    TrustRequired,
    Quarantined,
    Blocked,
    DependencyUnavailable {
        dependency: CapsuleId,
    },
}

impl UnavailableReason {
    pub fn describe(&self) -> String {
        match self {
            UnavailableReason::NotInCatalog => "not present in any registry".to_string(),
            UnavailableReason::DeniedByPolicy => "denied by managed policy".to_string(),
            UnavailableReason::PlatformUnsupported => {
                "not supported on this platform".to_string()
            }
            UnavailableReason::NoSupportedTarget => {
                "supports none of this context's targets".to_string()
            }
            UnavailableReason::TrustRequired => {
                "this revision has not been reviewed".to_string()
            }
            UnavailableReason::Quarantined => "quarantined".to_string(),
            UnavailableReason::Blocked => "blocked".to_string(),
            UnavailableReason::DependencyUnavailable { dependency } => {
                format!("its dependency {dependency} is unavailable")
            }
        }
    }
}

/// A capability in the effective view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActiveCapability {
    pub id: CapsuleId,
    pub kind: Kind,
    pub name: String,
    pub revision: Option<Revision>,
    pub source: Option<RegistrySource>,
    pub origin: SelectionOrigin,
    pub config: ConfigTable,
    /// The subset of the context's targets this capsule supports.
    pub targets: Vec<TargetId>,
    pub trust: TrustState,
    /// An unreviewed script may be exposed but must be confirmed before running.
    pub requires_run_confirmation: bool,
    pub exports: Vec<String>,
    pub dependencies: Vec<CapsuleId>,
    pub required_by: Vec<CapsuleId>,
}

/// A compact record of a catalogued capsule, kept so the view can explain and
/// rank capabilities that are *not* active without reaching back to the catalog.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogEntry {
    pub id: CapsuleId,
    pub kind: Kind,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub maturity: Maturity,
    pub revision: Option<Revision>,
    pub trust: TrustState,
    pub exports: Vec<String>,
    /// Advisory "often used with…" pointers (PRIOR-ART-ACTIONS L5), surfaced by
    /// `explain`, the palette and the tree. Never a dependency.
    #[serde(default)]
    pub related_skills: Vec<CapsuleId>,
}

/// The resolved graph for one context.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedView {
    pub context: ContextDescriptor,
    pub policy: ManagedPolicy,
    pub active: BTreeMap<CapsuleId, ActiveCapability>,
    pub declared: BTreeMap<CapsuleId, DeclaredState>,
    pub unavailable: BTreeMap<CapsuleId, UnavailableReason>,
    pub selection_log: Vec<SelectionOp>,
    pub catalog_index: BTreeMap<CapsuleId, CatalogEntry>,
    pub warnings: Vec<String>,
    pub hash: ResolutionHash,
    pub catalog_revision: String,
    /// Cosmetic, human-attached metadata about the *generation* this view will be
    /// materialised into — a label like `known-good`, a note. Serialised as the
    /// `[properties]` table in the lock, and deliberately **excluded from both the
    /// resolution hash and the generation's content identity**: a label edit must
    /// never invalidate a generation (PRIOR-ART-ACTIONS #9, Guix
    /// `manifest-entry-properties`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, String>,
}

impl ResolvedView {
    pub fn is_active(&self, id: &CapsuleId) -> bool {
        self.active.contains_key(id)
    }

    pub fn is_declared_enabled(&self, id: &CapsuleId) -> bool {
        self.declared.get(id).map(|d| d.enabled).unwrap_or(false)
    }

    pub fn is_declared_disabled(&self, id: &CapsuleId) -> bool {
        self.declared.get(id).map(|d| !d.enabled).unwrap_or(false)
    }

    pub fn unavailable_reason(&self, id: &CapsuleId) -> Option<&UnavailableReason> {
        self.unavailable.get(id)
    }

    /// Whether `aikit run` may invoke this capsule right now.
    ///
    /// Scripts, tools and templates can be run while inactive — activation only
    /// controls ambient exposure. Hooks, skills and guidance cannot: for them,
    /// "run" is not a meaningful separate act.
    ///
    /// Note what is deliberately *not* here: being active does not confer
    /// runnability. Activation and runnability answer different questions, and
    /// conflating them would put every live hook into the palette's `>` lane,
    /// where choosing one could only disappoint.
    pub fn can_run(&self, id: &CapsuleId) -> bool {
        match self.catalog_index.get(id) {
            Some(entry) => entry.maturity.is_selectable() && entry.kind.runnable_while_inactive(),
            None => false,
        }
    }

    /// Command names this view places on the contextual PATH.
    pub fn exported_commands(&self) -> BTreeMap<String, CapsuleId> {
        let mut out = BTreeMap::new();
        for (id, capability) in &self.active {
            for export in &capability.exports {
                out.insert(export.clone(), id.clone());
            }
        }
        out
    }

    pub fn active_of_kind(&self, kind: Kind) -> Vec<&ActiveCapability> {
        self.active.values().filter(|c| c.kind == kind).collect()
    }

    /// The capabilities related to `id` — "often used with…" — walked from both
    /// ends and restricted to what is actually catalogued.
    ///
    /// The edge is **declared** by an author (never inferred from usage, which
    /// would make the graph drift and would smuggle back the "usage promotes"
    /// failure the design refuses), **directed** in the manifest so one author
    /// states one relationship once, and **surfaced symmetrically** because a user
    /// asking "what goes with this?" wants an answer at whichever end they are
    /// standing. An edge pointing at something absent is dropped rather than
    /// reported: `related` is advisory, so a dangling one is not an error the way a
    /// dangling `requires` is.
    ///
    /// Sorted and deduplicated: the palette and the tree render this directly, and
    /// a list that reordered between keystrokes would read as a broken UI.
    pub fn related_to(&self, id: &CapsuleId) -> Vec<CapsuleId> {
        let mut out: BTreeSet<CapsuleId> = BTreeSet::new();

        // Forward: what this capsule declares.
        if let Some(entry) = self.catalog_index.get(id) {
            for related in &entry.related_skills {
                if related != id && self.catalog_index.contains_key(related) {
                    out.insert(related.clone());
                }
            }
        }

        // Reverse: what declares this capsule.
        for (other, entry) in &self.catalog_index {
            if other != id && entry.related_skills.contains(id) {
                out.insert(other.clone());
            }
        }

        out.into_iter().collect()
    }

    pub fn explain(&self, id: &CapsuleId) -> Option<Explanation> {
        explain::explain(self, id)
    }
}

// ---------------------------------------------------------------------------
// Problems
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Problem {
    pub error: AikitError,
    /// A fatal problem prevents a view from being produced at all.
    pub fatal: bool,
}

impl Problem {
    pub fn code(&self) -> &'static str {
        self.error.code()
    }

    fn fatal(error: AikitError) -> Self {
        Self { error, fatal: true }
    }
}

/// A resolution attempt that reports every problem instead of stopping at the
/// first. `aikit doctor` and the palette's "why can't I enable this" both use it.
#[derive(Debug, Clone)]
pub struct Diagnosis {
    pub view: Option<ResolvedView>,
    pub problems: Vec<Problem>,
    pub warnings: Vec<String>,
}

// ---------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------

/// Resolve strictly: any fatal problem becomes an error and no view is produced.
///
/// This is what `apply` uses, because a failed build must never replace a working
/// generation.
pub fn resolve(
    catalog: &dyn Catalog,
    trust: &dyn TrustOracle,
    request: &ResolveRequest,
) -> Result<ResolvedView, AikitError> {
    let diagnosis = resolve_diagnostic(catalog, trust, request);
    if let Some(problem) = diagnosis.problems.iter().find(|p| p.fatal) {
        return Err(problem.error.clone());
    }
    diagnosis.view.ok_or_else(|| {
        AikitError::new(
            "resolution.failed",
            "resolution produced no view and no fatal problem, which is a bug",
        )
    })
}

/// Resolve leniently, collecting every problem.
pub fn resolve_diagnostic(
    catalog: &dyn Catalog,
    trust: &dyn TrustOracle,
    request: &ResolveRequest,
) -> Diagnosis {
    let mut resolver = Resolver {
        catalog,
        trust,
        request,
        problems: Vec::new(),
        warnings: Vec::new(),
    };
    let view = resolver.run();
    Diagnosis {
        view,
        problems: resolver.problems,
        warnings: resolver.warnings,
    }
}

// ---------------------------------------------------------------------------
// The resolver
// ---------------------------------------------------------------------------

struct Resolver<'a> {
    catalog: &'a dyn Catalog,
    trust: &'a dyn TrustOracle,
    request: &'a ResolveRequest,
    problems: Vec<Problem>,
    warnings: Vec<String>,
}

impl<'a> Resolver<'a> {
    fn fail(&mut self, error: AikitError) {
        self.problems.push(Problem::fatal(error));
    }

    fn warn(&mut self, message: impl Into<String>) {
        self.warnings.push(message.into());
    }

    fn run(&mut self) -> Option<ResolvedView> {
        // 1. Fold the scope chain into an ordered operation log.
        let (ops, config) = self.fold_layers()?;

        // 2. Last operation wins per capsule.
        let mut declared: BTreeMap<CapsuleId, DeclaredState> = BTreeMap::new();
        for op in &ops {
            declared.insert(
                op.capsule.clone(),
                DeclaredState {
                    enabled: op.enable,
                    scope: op.scope,
                    origin: op.origin.clone(),
                    via_profile: op.via_profile.clone(),
                },
            );
        }

        // 3. Managed policy sits above the whole chain.
        let policy = &self.request.policy;
        let mut policy_forced: BTreeSet<CapsuleId> = BTreeSet::new();
        for required in &policy.require {
            if let Some(existing) = declared.get(required) {
                if !existing.enabled {
                    self.warn(format!(
                        "{required} is disabled by {} {} but is required by managed policy {}; \
                         the managed policy wins",
                        existing.scope, existing.origin, policy.source
                    ));
                }
            }
            declared.insert(
                required.clone(),
                DeclaredState {
                    enabled: true,
                    scope: ScopeKind::Global,
                    origin: LayerOrigin::new(format!("managed policy {}", policy.source)),
                    via_profile: None,
                },
            );
            policy_forced.insert(required.clone());
        }

        let explicit_disabled: BTreeSet<CapsuleId> = declared
            .iter()
            .filter(|(_, state)| !state.enabled)
            .map(|(id, _)| id.clone())
            .collect();

        // 4. Roots: everything declared-enabled that actually exists.
        let mut roots: Vec<CapsuleId> = Vec::new();
        for (id, state) in &declared {
            if !state.enabled {
                if self.catalog.get(id).is_none() {
                    self.warn(format!(
                        "{id} is disabled by {} {} but is not present in any registry",
                        state.scope, state.origin
                    ));
                }
                continue;
            }
            if self.catalog.get(id).is_none() {
                self.fail(
                    AikitError::new(
                        "resolution.unknown_capability",
                        format!(
                            "{id} is enabled by {} {} but is not present in any registry",
                            state.scope, state.origin
                        ),
                    )
                    .with("capability", id.to_string())
                    .with("scope", state.scope.as_str())
                    .with("origin", state.origin.to_string()),
                );
                continue;
            }
            roots.push(id.clone());
        }

        // 5. Expand dependencies (rules 3 and 4) and detect cycles (rule 5).
        let expansion = self.expand(&roots, &declared, &explicit_disabled);

        // 6. Availability, in dependency order so cascades are computed once.
        let mut unavailable: BTreeMap<CapsuleId, UnavailableReason> = BTreeMap::new();
        let mut active: BTreeMap<CapsuleId, ActiveCapability> = BTreeMap::new();

        for id in &expansion.order {
            let Some(capsule) = self.catalog.get(id) else {
                unavailable.insert(id.clone(), UnavailableReason::NotInCatalog);
                continue;
            };

            if let Some(reason) = self.availability(capsule, &expansion, &unavailable) {
                unavailable.insert(id.clone(), reason);
                continue;
            }

            let trust_state = self.trust.state_for(
                capsule.source.as_ref(),
                &capsule.id,
                capsule.revision.as_ref(),
            );
            let targets: Vec<TargetId> = self
                .request
                .context
                .targets
                .iter()
                .filter(|t| capsule.supports_target(t))
                .cloned()
                .collect();

            active.insert(
                id.clone(),
                ActiveCapability {
                    id: id.clone(),
                    kind: capsule.kind,
                    name: capsule.name.clone(),
                    revision: capsule.revision.clone(),
                    source: capsule.source.clone(),
                    origin: expansion
                        .origins
                        .get(id)
                        .cloned()
                        .unwrap_or(SelectionOrigin::Policy {
                            source: policy.source.clone(),
                        }),
                    config: config.get(id).cloned().unwrap_or_default(),
                    targets,
                    trust: trust_state,
                    requires_run_confirmation: capsule.kind.is_executable()
                        && !trust_state.may_run_unattended(),
                    exports: capsule.exported_commands(),
                    dependencies: expansion.dependencies.get(id).cloned().unwrap_or_default(),
                    required_by: expansion.required_by.get(id).cloned().unwrap_or_default(),
                },
            );
        }

        // 7. Conflicts and export collisions among what is actually active.
        self.check_conflicts(&active);
        self.check_export_collisions(&active);

        if self.problems.iter().any(|p| p.fatal) {
            return None;
        }

        let catalog_index = self.build_catalog_index();
        let hash = hash::resolution_hash(&self.request.context, policy, &active);

        Some(ResolvedView {
            context: self.request.context.clone(),
            policy: policy.clone(),
            active,
            declared,
            unavailable,
            selection_log: ops,
            catalog_index,
            warnings: self.warnings.clone(),
            hash,
            catalog_revision: self.catalog.catalog_revision(),
            // Resolution produces no cosmetic properties; a label is attached to
            // the generation afterwards, never derived from the resolution.
            properties: BTreeMap::new(),
        })
    }

    /// Sort layers by precedence and expand their profiles into a flat op log.
    fn fold_layers(&mut self) -> Option<(Vec<SelectionOp>, BTreeMap<CapsuleId, ConfigTable>)> {
        let mut indexed: Vec<(usize, &ScopeLayer)> =
            self.request.layers.iter().enumerate().collect();
        indexed.sort_by_key(|(i, layer)| layer.precedence_key(*i));

        let mut ops: Vec<SelectionOp> = Vec::new();
        let mut config: BTreeMap<CapsuleId, ConfigTable> = BTreeMap::new();

        for (_, layer) in indexed {
            if let Err(e) = layer.patch.validate() {
                self.fail(e);
                continue;
            }
            // Profiles first, in declaration order, each expanded depth-first.
            let mut expanded: BTreeSet<ProfileId> = BTreeSet::new();
            for profile_id in &layer.patch.profiles {
                let mut stack: Vec<ProfileId> = Vec::new();
                if self
                    .expand_profile(
                        profile_id,
                        layer,
                        &mut ops,
                        &mut config,
                        &mut expanded,
                        &mut stack,
                    )
                    .is_none()
                {
                    // The problem has already been recorded; keep going so the
                    // diagnosis is complete.
                }
            }
            self.apply_patch(&layer.patch, layer, None, &mut ops, &mut config);
        }

        Some((ops, config))
    }

    fn expand_profile(
        &mut self,
        profile_id: &ProfileId,
        layer: &ScopeLayer,
        ops: &mut Vec<SelectionOp>,
        config: &mut BTreeMap<CapsuleId, ConfigTable>,
        expanded: &mut BTreeSet<ProfileId>,
        stack: &mut Vec<ProfileId>,
    ) -> Option<()> {
        if stack.contains(profile_id) {
            let mut cycle: Vec<String> = stack.iter().map(|p| p.to_string()).collect();
            cycle.push(profile_id.to_string());
            self.fail(
                AikitError::new(
                    "resolution.profile_cycle",
                    format!("profile extends cycle: {}", cycle.join(" → ")),
                )
                .with("cycle", cycle.join(" -> ")),
            );
            return None;
        }
        if expanded.contains(profile_id) {
            return Some(());
        }

        let Some(profile) = self.catalog.profile(profile_id) else {
            self.fail(
                AikitError::new(
                    "resolution.unknown_profile",
                    format!(
                        "{profile_id} is referenced by {} {} but is not present in any registry",
                        layer.kind, layer.origin
                    ),
                )
                .with("profile", profile_id.to_string())
                .with("scope", layer.kind.as_str())
                .with("origin", layer.origin.to_string()),
            );
            return None;
        };

        let extends = profile.extends.clone();
        let patch = profile.patch.clone();

        stack.push(profile_id.clone());
        for parent in &extends {
            self.expand_profile(parent, layer, ops, config, expanded, stack);
        }
        // Nested `profiles = [...]` inside a profile behave like `extends`.
        for nested in &patch.profiles {
            self.expand_profile(nested, layer, ops, config, expanded, stack);
        }
        stack.pop();

        expanded.insert(profile_id.clone());
        self.apply_patch(&patch, layer, Some(profile_id), ops, config);
        Some(())
    }

    fn apply_patch(
        &mut self,
        patch: &PoolPatch,
        layer: &ScopeLayer,
        via_profile: Option<&ProfileId>,
        ops: &mut Vec<SelectionOp>,
        config: &mut BTreeMap<CapsuleId, ConfigTable>,
    ) {
        for id in &patch.enable {
            ops.push(SelectionOp {
                capsule: id.clone(),
                enable: true,
                scope: layer.kind,
                origin: layer.origin.clone(),
                via_profile: via_profile.cloned(),
            });
        }
        for id in &patch.disable {
            ops.push(SelectionOp {
                capsule: id.clone(),
                enable: false,
                scope: layer.kind,
                origin: layer.origin.clone(),
                via_profile: via_profile.cloned(),
            });
        }
        for (id, table) in &patch.config {
            // The merge mode is a property of the capsule the section configures,
            // not of the writer: a whole-record capsule replaces, a key/value one
            // deep-merges. A config section for an uncatalogued id (harmlessly
            // ignored later) falls back to the deep-merge default.
            let mode = self
                .catalog
                .get(id)
                .map(|c| c.config_merge)
                .unwrap_or_default();
            combine_config(config.entry(id.clone()).or_default(), table, mode);
        }
    }

    fn expand(
        &mut self,
        roots: &[CapsuleId],
        declared: &BTreeMap<CapsuleId, DeclaredState>,
        explicit_disabled: &BTreeSet<CapsuleId>,
    ) -> Expansion {
        let mut state = Expansion::default();
        let mut colour: BTreeMap<CapsuleId, Colour> = BTreeMap::new();
        let mut path: Vec<CapsuleId> = Vec::new();

        for root in roots {
            state.origins.insert(
                root.clone(),
                declared
                    .get(root)
                    .map(|d| SelectionOrigin::Layer {
                        scope: d.scope,
                        origin: d.origin.clone(),
                        via_profile: d.via_profile.clone(),
                    })
                    .unwrap_or(SelectionOrigin::Policy {
                        source: self.request.policy.source.clone(),
                    }),
            );
            self.visit(
                root,
                declared,
                explicit_disabled,
                &mut state,
                &mut colour,
                &mut path,
            );
        }
        state
    }

    fn visit(
        &mut self,
        id: &CapsuleId,
        declared: &BTreeMap<CapsuleId, DeclaredState>,
        explicit_disabled: &BTreeSet<CapsuleId>,
        state: &mut Expansion,
        colour: &mut BTreeMap<CapsuleId, Colour>,
        path: &mut Vec<CapsuleId>,
    ) {
        match colour.get(id) {
            Some(Colour::Black) => return,
            Some(Colour::Grey) => {
                let mut cycle: Vec<String> = path
                    .iter()
                    .skip_while(|p| *p != id)
                    .map(|p| p.to_string())
                    .collect();
                cycle.push(id.to_string());
                self.fail(
                    AikitError::new(
                        "resolution.dependency_cycle",
                        format!("dependency cycle: {}", cycle.join(" → ")),
                    )
                    .with("cycle", cycle.join(" -> ")),
                );
                return;
            }
            None => {}
        }

        colour.insert(id.clone(), Colour::Grey);
        path.push(id.clone());

        if let Some(capsule) = self.catalog.get(id) {
            let requires = capsule.requires.clone();
            for requirement in &requires {
                let dep = &requirement.id;

                if explicit_disabled.contains(dep) {
                    if requirement.optional {
                        self.warn(format!(
                            "{id} optionally requires {dep}, which is explicitly disabled; \
                             continuing without it"
                        ));
                        continue;
                    }
                    let disabling = declared.get(dep);
                    let mut error = AikitError::new(
                        "resolution.required_capability_disabled",
                        format!(
                            "cannot enable {id}: required capability {dep} is disabled by the {} \
                             scope",
                            disabling.map(|d| d.scope.as_str()).unwrap_or("unknown")
                        ),
                    )
                    .with("capability", dep.to_string())
                    .with("required_by", id.to_string());
                    if let Some(d) = disabling {
                        error = error
                            .with("scope", d.scope.as_str())
                            .with("origin", d.origin.to_string());
                    }
                    if let Some(reason) = &requirement.reason {
                        error = error.with("reason", reason.clone());
                    }
                    self.fail(error);
                    continue;
                }

                if self.catalog.get(dep).is_none() {
                    if requirement.optional {
                        self.warn(format!(
                            "{id} optionally requires {dep}, which is not in any registry; \
                             continuing without it"
                        ));
                        continue;
                    }
                    self.fail(
                        AikitError::new(
                            "resolution.missing_dependency",
                            format!("{id} requires {dep}, which is not present in any registry"),
                        )
                        .with("capability", dep.to_string())
                        .with("required_by", id.to_string()),
                    );
                    continue;
                }

                state
                    .origins
                    .entry(dep.clone())
                    .or_insert(SelectionOrigin::Dependency {
                        required_by: id.clone(),
                    });
                self.visit(dep, declared, explicit_disabled, state, colour, path);

                let deps = state.dependencies.entry(id.clone()).or_default();
                if !deps.contains(dep) {
                    deps.push(dep.clone());
                }
                let dependents = state.required_by.entry(dep.clone()).or_default();
                if !dependents.contains(id) {
                    dependents.push(id.clone());
                }
            }
        }

        path.pop();
        colour.insert(id.clone(), Colour::Black);
        state.order.push(id.clone());
    }

    /// The availability gate. Order matters: a hard refusal (policy, quarantine,
    /// block) must win over a soft one so the message names the real obstacle.
    fn availability(
        &self,
        capsule: &Capsule,
        expansion: &Expansion,
        unavailable: &BTreeMap<CapsuleId, UnavailableReason>,
    ) -> Option<UnavailableReason> {
        if self.request.policy.denies(capsule).is_some() {
            return Some(UnavailableReason::DeniedByPolicy);
        }
        if !capsule.maturity.is_selectable() {
            return Some(UnavailableReason::Blocked);
        }

        let trust = self.trust.state_for(
            capsule.source.as_ref(),
            &capsule.id,
            capsule.revision.as_ref(),
        );
        if trust == TrustState::Quarantined {
            return Some(UnavailableReason::Quarantined);
        }
        if trust == TrustState::Blocked {
            return Some(UnavailableReason::Blocked);
        }

        if !capsule.supports_platform(self.request.context.platform) {
            return Some(UnavailableReason::PlatformUnsupported);
        }
        if !self
            .request
            .context
            .targets
            .iter()
            .any(|t| capsule.supports_target(t))
        {
            return Some(UnavailableReason::NoSupportedTarget);
        }
        if capsule.kind.requires_trust_to_activate() && !trust.may_project() {
            return Some(UnavailableReason::TrustRequired);
        }

        // Cascade: a capability whose requirement could not be activated is not
        // itself activatable. Dependencies are visited first, so this is a lookup.
        if let Some(deps) = expansion.dependencies.get(&capsule.id) {
            for dep in deps {
                if unavailable.contains_key(dep) {
                    return Some(UnavailableReason::DependencyUnavailable {
                        dependency: dep.clone(),
                    });
                }
            }
        }
        None
    }

    fn check_conflicts(&mut self, active: &BTreeMap<CapsuleId, ActiveCapability>) {
        let mut reported: BTreeSet<(CapsuleId, CapsuleId)> = BTreeSet::new();
        for id in active.keys() {
            let Some(capsule) = self.catalog.get(id) else {
                continue;
            };
            for conflict in &capsule.conflicts {
                if !active.contains_key(&conflict.id) {
                    continue;
                }
                let pair = if *id < conflict.id {
                    (id.clone(), conflict.id.clone())
                } else {
                    (conflict.id.clone(), id.clone())
                };
                if !reported.insert(pair.clone()) {
                    continue;
                }
                let mut error = AikitError::new(
                    "resolution.conflict",
                    match &conflict.reason {
                        Some(reason) => format!(
                            "{} conflicts with {}: {reason}",
                            pair.0, pair.1
                        ),
                        None => format!("{} conflicts with {}", pair.0, pair.1),
                    },
                )
                .with("capability", pair.0.to_string())
                .with("conflicts_with", pair.1.to_string());
                if let Some(reason) = &conflict.reason {
                    error = error.with("reason", reason.clone());
                }
                self.fail(error);
            }
        }
    }

    fn check_export_collisions(&mut self, active: &BTreeMap<CapsuleId, ActiveCapability>) {
        let mut owners: BTreeMap<String, CapsuleId> = BTreeMap::new();
        for (id, capability) in active {
            for export in &capability.exports {
                if let Some(existing) = owners.get(export) {
                    self.fail(
                        AikitError::new(
                            "resolution.export_collision",
                            format!(
                                "{existing} and {id} both export the command `{export}` in this \
                                 context"
                            ),
                        )
                        .with("export", export.clone())
                        .with("capability", existing.to_string())
                        .with("collides_with", id.to_string()),
                    );
                } else {
                    owners.insert(export.clone(), id.clone());
                }
            }
        }
    }

    fn build_catalog_index(&self) -> BTreeMap<CapsuleId, CatalogEntry> {
        self.catalog
            .capsules()
            .into_iter()
            .map(|c| {
                (
                    c.id.clone(),
                    CatalogEntry {
                        id: c.id.clone(),
                        kind: c.kind,
                        name: c.name.clone(),
                        description: c.description.clone(),
                        tags: c.tags.clone(),
                        maturity: c.maturity,
                        revision: c.revision.clone(),
                        trust: self
                            .trust
                            .state_for(c.source.as_ref(), &c.id, c.revision.as_ref()),
                        exports: c.exported_commands(),
                        related_skills: c.related_skills.clone(),
                    },
                )
            })
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Colour {
    Grey,
    Black,
}

#[derive(Debug, Default)]
struct Expansion {
    /// Post-order: dependencies before dependents.
    order: Vec<CapsuleId>,
    origins: BTreeMap<CapsuleId, SelectionOrigin>,
    dependencies: BTreeMap<CapsuleId, Vec<CapsuleId>>,
    required_by: BTreeMap<CapsuleId, Vec<CapsuleId>>,
}
