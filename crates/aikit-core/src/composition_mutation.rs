//! Explicit staged mutation for canonical V2 composition grammars.
//!
//! This module owns UI-neutral stage -> preview/explain -> confirm -> apply
//! contracts. Renderers may project these contracts, but they do not own them.
//!
//! Harness composition changes remain resolver-owned desired-body changes. Profile
//! composition changes remain authored resolution intent: a [`PoolPatch`] is never
//! confused with its derived [`ResolvedView`]. Skill-set relations remain additive
//! projection requests: there is deliberately no reorder/precedence mutation,
//! because SkillSet composition is union-only by domain law.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::composition::{
    resolve_harness_composition, ComponentSelection, CompositionCatalog, HarnessComposition,
    HarnessCompositionRequest,
};
use crate::composition_view::{diff_harness_compositions, HarnessCompositionDiff};
use crate::id::{CapsuleId, ProfileId};
use crate::profile::PoolPatch;
use crate::resolve::ResolvedView;
use crate::resource::ResourceRef;
use crate::scope::ScopeKind;
use crate::skillset::{project as project_skill_set, SetMembership, SkillSet};
use crate::{AikitError, Result};

/// One explicit edit to the desired body of a Harness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum HarnessCompositionMutation {
    /// Mount or replace the selection for a canonical Component identity.
    Select { selection: ComponentSelection },
    /// Retract the selected Component while preserving every unrelated identity.
    Retract { component: ResourceRef },
}

/// User/agent-authored intent waiting for preview. This contains no resolved
/// provider bindings, contributions or Surfaces; those remain resolver output.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StagedHarnessComposition {
    #[serde(default)]
    mutations: Vec<HarnessCompositionMutation>,
}

impl StagedHarnessComposition {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn select(&mut self, selection: ComponentSelection) {
        // One staged answer per Component identity. Re-staging replaces the prior
        // answer rather than creating ordering-sensitive duplicate intent.
        self.mutations.retain(|mutation| match mutation {
            HarnessCompositionMutation::Select { selection: existing } => {
                existing.component != selection.component
            }
            HarnessCompositionMutation::Retract { component } => component != &selection.component,
        });
        self.mutations
            .push(HarnessCompositionMutation::Select { selection });
    }

    pub fn retract(&mut self, component: ResourceRef) {
        self.mutations.retain(|mutation| match mutation {
            HarnessCompositionMutation::Select { selection } => selection.component != component,
            HarnessCompositionMutation::Retract { component: existing } => existing != &component,
        });
        self.mutations
            .push(HarnessCompositionMutation::Retract { component });
    }

    pub fn is_empty(&self) -> bool {
        self.mutations.is_empty()
    }

    pub fn len(&self) -> usize {
        self.mutations.len()
    }

    pub fn mutations(&self) -> &[HarnessCompositionMutation] {
        &self.mutations
    }

    fn apply_to(&self, request: &mut HarnessCompositionRequest) {
        for mutation in &self.mutations {
            match mutation {
                HarnessCompositionMutation::Select { selection } => {
                    request
                        .selections
                        .retain(|existing| existing.component != selection.component);
                    request.selections.push(selection.clone());
                }
                HarnessCompositionMutation::Retract { component } => {
                    request
                        .selections
                        .retain(|existing| &existing.component != component);
                }
            }
        }
    }
}

/// Resolver-owned preview of a staged body change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessCompositionPreview {
    pub staged: StagedHarnessComposition,
    pub before_fingerprint: String,
    pub projected: HarnessComposition,
    pub diff: HarnessCompositionDiff,
}

impl HarnessCompositionPreview {
    /// Confirmation is a distinct type transition so callers cannot accidentally
    /// apply a preview merely because it was successfully resolved.
    pub fn confirm(self) -> ConfirmedHarnessCompositionPreview {
        ConfirmedHarnessCompositionPreview { preview: self }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfirmedHarnessCompositionPreview {
    preview: HarnessCompositionPreview,
}

impl ConfirmedHarnessCompositionPreview {
    pub fn preview(&self) -> &HarnessCompositionPreview {
        &self.preview
    }
}

/// Preview staged Component intent by reusing the canonical resolver.
///
/// The current resolved body is converted back only into its explicit Component
/// selections and stable actor/harness anchors. Provider bindings, contributions,
/// Surfaces and projections are recomputed by `resolve_harness_composition`.
pub fn preview_harness_composition_change(
    catalog: &CompositionCatalog,
    current: &HarnessComposition,
    staged: StagedHarnessComposition,
) -> Result<HarnessCompositionPreview> {
    let mut request = request_from_resolved(current);
    staged.apply_to(&mut request);
    let projected = resolve_harness_composition(catalog, request)?;
    let diff = diff_harness_compositions(current, &projected)?;
    Ok(HarnessCompositionPreview {
        staged,
        before_fingerprint: current.fingerprint.clone(),
        projected,
        diff,
    })
}

/// Apply a *confirmed semantic composition* as the new desired body.
///
/// This function has no target adapter and therefore cannot claim live mounting,
/// process state or Workcell materialisation. The returned composition remains in
/// resolver-owned `CompositionState::Resolved` until an owning target/provider
/// separately observes stronger material truth.
pub fn apply_confirmed_harness_composition(
    confirmed: ConfirmedHarnessCompositionPreview,
) -> HarnessComposition {
    confirmed.preview.projected
}

fn request_from_resolved(current: &HarnessComposition) -> HarnessCompositionRequest {
    HarnessCompositionRequest {
        harness: current.harness.clone(),
        project: current.project.clone(),
        agent: current.agent.clone(),
        agency: current.agency.clone(),
        session: current.session.clone(),
        model: current.model.clone(),
        selections: current
            .component_bindings
            .iter()
            .map(|binding| ComponentSelection {
                component: binding.component.clone(),
                resolution_scope: binding.resolution_scope.clone(),
                activation_scope: binding.activation_scope.clone(),
                lifetime_owner: binding.lifetime_owner.clone(),
                activation_mode: binding.activation_mode,
            })
            .collect(),
        target_revision: current.target_revision.clone(),
        generation: current.generation.clone(),
    }
}

// ---------------------------------------------------------------------------
// Profile / SkillSet application composition
// ---------------------------------------------------------------------------

/// Exact resolver basis a profile-composition preview was produced against.
///
/// `catalog_revision` protects provider/catalog identity while `resolution_hash`
/// protects the complete effective resolution. Consumers must compare this basis
/// immediately before applying an accepted preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompositionBasis {
    pub catalog_revision: String,
    pub resolution_hash: String,
}

impl CompositionBasis {
    pub fn from_view(view: &ResolvedView) -> Self {
        Self {
            catalog_revision: view.catalog_revision.clone(),
            resolution_hash: view.hash.to_string(),
        }
    }

    pub fn matches(&self, view: &ResolvedView) -> bool {
        self.catalog_revision == view.catalog_revision && self.resolution_hash == view.hash.to_string()
    }
}

/// Canonical authored activation intent. This is intentionally narrower than a
/// generic Resource mutation: Profile activation is a package-backed Capability
/// relation and must not pretend every V2 Resource is activatable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileActivationIntent {
    Enable,
    Disable,
}

/// Typed, write-free staged Profile activation intent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StagedProfileComposition {
    changes: BTreeMap<CapsuleId, ProfileActivationIntent>,
}

impl StagedProfileComposition {
    pub fn stage(&mut self, capability: CapsuleId, intent: ProfileActivationIntent) {
        self.changes.insert(capability, intent);
    }

    pub fn unstage(&mut self, capability: &CapsuleId) -> Option<ProfileActivationIntent> {
        self.changes.remove(capability)
    }

    pub fn get(&self, capability: &CapsuleId) -> Option<ProfileActivationIntent> {
        self.changes.get(capability).copied()
    }

    pub fn capabilities(&self) -> impl Iterator<Item = &CapsuleId> {
        self.changes.keys()
    }

    pub fn len(&self) -> usize {
        self.changes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// Materialise staged intent into an authored copy for resolver preview.
    /// The caller's source patch is never changed.
    pub fn authored_after(&self, authored_before: &PoolPatch) -> PoolPatch {
        let mut after = authored_before.clone();
        for (capability, intent) in &self.changes {
            after.set(capability, matches!(intent, ProfileActivationIntent::Enable));
        }
        after
    }
}

/// The only SkillSet membership mutations supported by the current domain.
///
/// There is deliberately no `Reorder`: SkillSets compose by union and have no
/// precedence. Presentation order in an optional manifest is not resolution
/// authority and must not be exposed as semantic composition ordering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum SkillSetRelationMutation {
    Add {
        skill_set: String,
        capability: CapsuleId,
    },
    Remove {
        skill_set: String,
        capability: CapsuleId,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StagedSkillSetRelations {
    mutations: Vec<SkillSetRelationMutation>,
}

impl StagedSkillSetRelations {
    pub fn add(&mut self, skill_set: impl Into<String>, capability: CapsuleId) {
        self.replace(SkillSetRelationMutation::Add {
            skill_set: skill_set.into(),
            capability,
        });
    }

    pub fn remove(&mut self, skill_set: impl Into<String>, capability: CapsuleId) {
        self.replace(SkillSetRelationMutation::Remove {
            skill_set: skill_set.into(),
            capability,
        });
    }

    fn replace(&mut self, next: SkillSetRelationMutation) {
        let (next_set, next_capability) = relation_key(&next);
        self.mutations.retain(|existing| {
            let (set, capability) = relation_key(existing);
            set != next_set || capability != next_capability
        });
        self.mutations.push(next);
    }

    pub fn mutations(&self) -> &[SkillSetRelationMutation] {
        &self.mutations
    }

    pub fn is_empty(&self) -> bool {
        self.mutations.is_empty()
    }

    pub fn len(&self) -> usize {
        self.mutations.len()
    }

    /// Produce a write-free authored-after copy of the named sets. Unknown set
    /// names remain an application-service error rather than silently creating a
    /// new ownership boundary.
    pub fn authored_after(&self, sets: &[SkillSet]) -> Result<Vec<SkillSet>> {
        let mut after = sets.to_vec();
        for mutation in &self.mutations {
            let (name, capability) = relation_key(mutation);
            let set = after.iter_mut().find(|set| set.name == name).ok_or_else(|| {
                AikitError::new(
                    "composition.skillset_not_found",
                    format!("SkillSet `{name}` is not present in the inspected composition"),
                )
            })?;
            if !set.provenance.is_writable() {
                return Err(AikitError::new(
                    "composition.skillset_read_only",
                    format!("SkillSet `{name}` is observed and cannot be mutated in place"),
                ));
            }
            match mutation {
                SkillSetRelationMutation::Add { .. } => {
                    set.members.insert(capability.clone(), SetMembership::Explicit);
                }
                SkillSetRelationMutation::Remove { .. } => {
                    set.members.remove(capability);
                }
            }
        }
        Ok(after)
    }
}

fn relation_key(mutation: &SkillSetRelationMutation) -> (&str, &CapsuleId) {
    match mutation {
        SkillSetRelationMutation::Add {
            skill_set,
            capability,
        }
        | SkillSetRelationMutation::Remove {
            skill_set,
            capability,
        } => (skill_set, capability),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthoredProfileReadModel {
    pub profiles: Vec<ProfileId>,
    pub enabled: Vec<CapsuleId>,
    pub disabled: Vec<CapsuleId>,
}

impl From<&PoolPatch> for AuthoredProfileReadModel {
    fn from(patch: &PoolPatch) -> Self {
        Self {
            profiles: patch.profiles.clone(),
            enabled: patch.enable.clone(),
            disabled: patch.disable.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnavailableCapabilityReadModel {
    pub capability: CapsuleId,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectiveProfileReadModel {
    pub active: Vec<CapsuleId>,
    pub declared_enabled: Vec<CapsuleId>,
    pub declared_disabled: Vec<CapsuleId>,
    pub unavailable: Vec<UnavailableCapabilityReadModel>,
    /// Human/agent-readable resolver provenance. The authoritative typed
    /// explanation remains on `ResolvedView`; this is a stable application read
    /// projection rather than a second authority system.
    pub provenance: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum SkillSetMemberRelationState {
    Effective,
    Withheld { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillSetMemberRelationReadModel {
    pub capability: CapsuleId,
    pub membership: SetMembership,
    pub state: SkillSetMemberRelationState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillSetRelationReadModel {
    pub identity: String,
    pub provenance: String,
    pub writable: bool,
    /// Stable presentation order only. SkillSet semantics remain set union with no
    /// precedence, override or reorder authority.
    pub members: Vec<SkillSetMemberRelationReadModel>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileCompositionReadModel {
    pub basis: CompositionBasis,
    pub scope: ScopeKind,
    pub authored: AuthoredProfileReadModel,
    pub effective: EffectiveProfileReadModel,
    pub skill_sets: Vec<SkillSetRelationReadModel>,
    pub warnings: Vec<String>,
}

/// Inspect authored intent and derived resolution without collapsing either side.
pub fn inspect_profile_composition(
    scope: ScopeKind,
    authored: &PoolPatch,
    effective: &ResolvedView,
    skill_sets: &[SkillSet],
) -> ProfileCompositionReadModel {
    let declared_enabled = effective
        .declared
        .iter()
        .filter(|(_, state)| state.enabled)
        .map(|(capability, _)| capability.clone())
        .collect();
    let declared_disabled = effective
        .declared
        .iter()
        .filter(|(_, state)| !state.enabled)
        .map(|(capability, _)| capability.clone())
        .collect();
    let unavailable = effective
        .unavailable
        .iter()
        .map(|(capability, reason)| UnavailableCapabilityReadModel {
            capability: capability.clone(),
            reason: reason.describe(),
        })
        .collect();
    let provenance = effective
        .selection_log
        .iter()
        .map(|selection| selection.describe())
        .collect();
    let skill_sets = skill_sets
        .iter()
        .map(|set| inspect_skill_set_relation(set, effective))
        .collect();

    ProfileCompositionReadModel {
        basis: CompositionBasis::from_view(effective),
        scope,
        authored: AuthoredProfileReadModel::from(authored),
        effective: EffectiveProfileReadModel {
            active: effective.active.keys().cloned().collect(),
            declared_enabled,
            declared_disabled,
            unavailable,
            provenance,
        },
        skill_sets,
        warnings: effective.warnings.clone(),
    }
}

fn inspect_skill_set_relation(set: &SkillSet, effective: &ResolvedView) -> SkillSetRelationReadModel {
    let projection = project_skill_set(set, effective);
    let projected: BTreeSet<CapsuleId> = projection.projected.into_iter().collect();
    let withheld: BTreeMap<CapsuleId, String> = projection
        .withheld
        .into_iter()
        .map(|entry| (entry.capsule, entry.reason.describe()))
        .collect();
    let members = set
        .members
        .iter()
        .map(|(capability, membership)| SkillSetMemberRelationReadModel {
            capability: capability.clone(),
            membership: membership.clone(),
            state: if projected.contains(capability) {
                SkillSetMemberRelationState::Effective
            } else {
                SkillSetMemberRelationState::Withheld {
                    reason: withheld
                        .get(capability)
                        .cloned()
                        .unwrap_or_else(|| "not projected from this context".to_string()),
                }
            },
        })
        .collect();
    SkillSetRelationReadModel {
        identity: set.name.clone(),
        provenance: set.provenance.as_str().to_string(),
        writable: set.provenance.is_writable(),
        members,
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangedGround {
    pub capabilities_added: Vec<CapsuleId>,
    pub capabilities_removed: Vec<CapsuleId>,
    pub declared_enabled_added: Vec<CapsuleId>,
    pub declared_enabled_removed: Vec<CapsuleId>,
    pub unavailable_added: Vec<UnavailableCapabilityReadModel>,
    pub unavailable_removed: Vec<UnavailableCapabilityReadModel>,
    pub warnings_added: Vec<String>,
    pub warnings_removed: Vec<String>,
}

impl ChangedGround {
    pub fn is_empty(&self) -> bool {
        self.capabilities_added.is_empty()
            && self.capabilities_removed.is_empty()
            && self.declared_enabled_added.is_empty()
            && self.declared_enabled_removed.is_empty()
            && self.unavailable_added.is_empty()
            && self.unavailable_removed.is_empty()
            && self.warnings_added.is_empty()
            && self.warnings_removed.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileCompositionPreview {
    pub basis_before: CompositionBasis,
    pub basis_after: CompositionBasis,
    pub scope: ScopeKind,
    pub staged_profile: StagedProfileComposition,
    pub staged_skill_sets: StagedSkillSetRelations,
    pub before: ProfileCompositionReadModel,
    pub after: ProfileCompositionReadModel,
    pub changed_ground: ChangedGround,
}

/// Build a structured preview *only* from canonical resolver outputs supplied by
/// the application service. This function never guesses effective state and never
/// writes authored state.
pub fn preview_profile_composition_change(
    scope: ScopeKind,
    authored_before: &PoolPatch,
    effective_before: &ResolvedView,
    skill_sets_before: &[SkillSet],
    staged_profile: StagedProfileComposition,
    staged_skill_sets: StagedSkillSetRelations,
    effective_after: &ResolvedView,
) -> Result<ProfileCompositionPreview> {
    let authored_after = staged_profile.authored_after(authored_before);
    let skill_sets_after = staged_skill_sets.authored_after(skill_sets_before)?;
    let before = inspect_profile_composition(scope, authored_before, effective_before, skill_sets_before);
    let after = inspect_profile_composition(scope, &authored_after, effective_after, &skill_sets_after);
    let changed_ground = changed_ground(effective_before, effective_after);
    Ok(ProfileCompositionPreview {
        basis_before: before.basis.clone(),
        basis_after: after.basis.clone(),
        scope,
        staged_profile,
        staged_skill_sets,
        before,
        after,
        changed_ground,
    })
}

/// Reject applying a preview whose effective/catalog basis has materially moved.
pub fn ensure_profile_composition_preview_current(
    preview: &ProfileCompositionPreview,
    current: &ResolvedView,
) -> Result<()> {
    if preview.basis_before.matches(current) {
        return Ok(());
    }
    Err(AikitError::new(
        "composition.preview_stale",
        "the accepted composition preview was produced against a different resolution basis",
    )
    .with("expected_catalog_revision", preview.basis_before.catalog_revision.clone())
    .with("expected_resolution_hash", preview.basis_before.resolution_hash.clone())
    .with("current_catalog_revision", current.catalog_revision.clone())
    .with("current_resolution_hash", current.hash.to_string()))
}

pub fn changed_ground(before: &ResolvedView, after: &ResolvedView) -> ChangedGround {
    let before_active: BTreeSet<_> = before.active.keys().cloned().collect();
    let after_active: BTreeSet<_> = after.active.keys().cloned().collect();
    let before_enabled: BTreeSet<_> = before
        .declared
        .iter()
        .filter(|(_, state)| state.enabled)
        .map(|(capability, _)| capability.clone())
        .collect();
    let after_enabled: BTreeSet<_> = after
        .declared
        .iter()
        .filter(|(_, state)| state.enabled)
        .map(|(capability, _)| capability.clone())
        .collect();
    let before_unavailable: BTreeMap<_, _> = before
        .unavailable
        .iter()
        .map(|(capability, reason)| (capability.clone(), reason.describe()))
        .collect();
    let after_unavailable: BTreeMap<_, _> = after
        .unavailable
        .iter()
        .map(|(capability, reason)| (capability.clone(), reason.describe()))
        .collect();
    let before_warnings: BTreeSet<_> = before.warnings.iter().cloned().collect();
    let after_warnings: BTreeSet<_> = after.warnings.iter().cloned().collect();

    ChangedGround {
        capabilities_added: after_active.difference(&before_active).cloned().collect(),
        capabilities_removed: before_active.difference(&after_active).cloned().collect(),
        declared_enabled_added: after_enabled.difference(&before_enabled).cloned().collect(),
        declared_enabled_removed: before_enabled.difference(&after_enabled).cloned().collect(),
        unavailable_added: after_unavailable
            .iter()
            .filter(|(capability, reason)| before_unavailable.get(*capability) != Some(*reason))
            .map(|(capability, reason)| UnavailableCapabilityReadModel {
                capability: capability.clone(),
                reason: reason.clone(),
            })
            .collect(),
        unavailable_removed: before_unavailable
            .iter()
            .filter(|(capability, reason)| after_unavailable.get(*capability) != Some(*reason))
            .map(|(capability, reason)| UnavailableCapabilityReadModel {
                capability: capability.clone(),
                reason: reason.clone(),
            })
            .collect(),
        warnings_added: after_warnings.difference(&before_warnings).cloned().collect(),
        warnings_removed: before_warnings.difference(&after_warnings).cloned().collect(),
    }
}
