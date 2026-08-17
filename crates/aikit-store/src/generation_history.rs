//! Historical comparison of immutable Generation worlds.
//!
//! A committed Generation already contains both the exact `metadata.json` receipt
//! and the exact resolved `resolution.lock.toml`. History therefore compares those
//! immutable artefacts directly. It never reruns the resolver against today's
//! catalog and never treats a cosmetic current label as historical semantic truth.

use std::collections::{BTreeMap, BTreeSet};

use aikit_core::resource::{ResourceRef, SourceAuthority};
use aikit_core::{
    AikitError, ContextId, GenerationId, HistoryEvidence, HistoryKind, HistoryRecoverability,
    ResolvedView, Result, EXPLAIN_HISTORY_VERSION,
};
use serde::{Deserialize, Serialize};

use crate::generation::{is_generation, read_lock, read_metadata, GenerationMetadata, GENERATIONS};
use crate::AikitHome;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenerationWorldComparison {
    pub schema: String,
    pub before: GenerationId,
    pub after: GenerationId,
    pub before_resolution_hash: String,
    pub after_resolution_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_project: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_project: Option<ResourceRef>,
    #[serde(default)]
    pub activated: Vec<ResourceRef>,
    #[serde(default)]
    pub deactivated: Vec<ResourceRef>,
    #[serde(default)]
    pub active_changed: Vec<ResourceRef>,
    #[serde(default)]
    pub declaration_changed: Vec<ResourceRef>,
    #[serde(default)]
    pub availability_changed: Vec<ResourceRef>,
    #[serde(default)]
    pub target_effects_changed: Vec<String>,
    pub warnings_changed: bool,
    pub catalog_revision_changed: bool,
    pub isolation_changed: bool,
    pub materialization_changed: bool,
    /// Common History navigation row for the comparison itself. `canonical_refs`
    /// includes both Generations, both Projects when present, and every changed
    /// canonical Capability reference.
    pub evidence: HistoryEvidence,
}

impl GenerationWorldComparison {
    pub fn is_noop(&self) -> bool {
        self.before_resolution_hash == self.after_resolution_hash
            && self.before_project == self.after_project
            && self.activated.is_empty()
            && self.deactivated.is_empty()
            && self.active_changed.is_empty()
            && self.declaration_changed.is_empty()
            && self.availability_changed.is_empty()
            && self.target_effects_changed.is_empty()
            && !self.warnings_changed
            && !self.catalog_revision_changed
            && !self.isolation_changed
            && !self.materialization_changed
    }
}

pub fn compare_generation_worlds(
    home: &AikitHome,
    context: &ContextId,
    before: &GenerationId,
    after: &GenerationId,
) -> Result<GenerationWorldComparison> {
    let (before_metadata, before_view) = load_generation(home, context, before)?;
    let (after_metadata, after_view) = load_generation(home, context, after)?;

    if before_view.context.context_id != after_view.context.context_id {
        return Err(AikitError::new(
            "history.generation_context_mismatch",
            format!(
                "cannot compare Generations {before} and {after} from different Context identities"
            ),
        ));
    }

    let before_active = before_view.active.keys().cloned().collect::<BTreeSet<_>>();
    let after_active = after_view.active.keys().cloned().collect::<BTreeSet<_>>();
    let activated = capsule_refs(after_active.difference(&before_active).cloned())?;
    let deactivated = capsule_refs(before_active.difference(&after_active).cloned())?;
    let active_changed = capsule_refs(
        before_active
            .intersection(&after_active)
            .filter(|id| before_view.active.get(*id) != after_view.active.get(*id))
            .cloned(),
    )?;

    let declaration_changed = capsule_refs(changed_map_keys(
        &before_view.declared,
        &after_view.declared,
    ))?;
    let availability_changed = capsule_refs(changed_map_keys(
        &before_view.unavailable,
        &after_view.unavailable,
    ))?;
    let target_effects_changed = changed_target_effects(&before_metadata, &after_metadata);

    let before_project = project_ref(&before_view)?;
    let after_project = project_ref(&after_view)?;
    let warnings_changed = before_view.warnings != after_view.warnings;
    let catalog_revision_changed = before_metadata.catalog_revision != after_metadata.catalog_revision;
    let isolation_changed = before_metadata.isolation != after_metadata.isolation;
    let materialization_changed = before_metadata.materialization != after_metadata.materialization;

    let mut canonical_refs = BTreeSet::new();
    canonical_refs.insert(generation_ref(before)?);
    canonical_refs.insert(generation_ref(after)?);
    canonical_refs.extend(before_project.iter().cloned());
    canonical_refs.extend(after_project.iter().cloned());
    canonical_refs.extend(activated.iter().cloned());
    canonical_refs.extend(deactivated.iter().cloned());
    canonical_refs.extend(active_changed.iter().cloned());
    canonical_refs.extend(declaration_changed.iter().cloned());
    canonical_refs.extend(availability_changed.iter().cloned());

    let mut details = BTreeMap::new();
    details.insert("beforeResolutionHash".into(), before_metadata.resolution_hash.clone());
    details.insert("afterResolutionHash".into(), after_metadata.resolution_hash.clone());
    details.insert("activated".into(), join_refs(&activated));
    details.insert("deactivated".into(), join_refs(&deactivated));
    details.insert("activeChanged".into(), join_refs(&active_changed));
    details.insert("declarationChanged".into(), join_refs(&declaration_changed));
    details.insert("availabilityChanged".into(), join_refs(&availability_changed));
    if !target_effects_changed.is_empty() {
        details.insert("targetEffectsChanged".into(), target_effects_changed.join(", "));
    }
    details.insert("warningsChanged".into(), warnings_changed.to_string());
    details.insert(
        "catalogRevisionChanged".into(),
        catalog_revision_changed.to_string(),
    );
    details.insert("isolationChanged".into(), isolation_changed.to_string());
    details.insert(
        "materializationChanged".into(),
        materialization_changed.to_string(),
    );

    let semantic_change_count = activated.len()
        + deactivated.len()
        + active_changed.len()
        + declaration_changed.len()
        + availability_changed.len();
    let evidence = HistoryEvidence {
        schema: EXPLAIN_HISTORY_VERSION.into(),
        id: format!("generation-comparison:{before}:{after}"),
        kind: HistoryKind::Generation,
        subject: generation_ref(after)?,
        // The two worlds are Generated receipts; the comparison is a Derived
        // read over those immutable receipts.
        authorities: vec![SourceAuthority::Generated, SourceAuthority::Derived],
        occurred_at_unix_ms: Some(
            after_metadata.created_at.as_nanos().max(0) as u128 / 1_000_000,
        ),
        summary: format!(
            "Generation {before} -> {after}: {semantic_change_count} capability-state change{}, {} target-effect change{}",
            plural(semantic_change_count),
            target_effects_changed.len(),
            plural(target_effects_changed.len())
        ),
        canonical_refs: canonical_refs.into_iter().collect(),
        provenance: Vec::new(),
        // AIKit has a narrow current<->previous rollback primitive, but this
        // arbitrary historical comparison deliberately does not advertise that as
        // a generic restore path that could bypass current policy/resolution.
        recoverability: HistoryRecoverability::InspectOnly,
        details,
    };

    Ok(GenerationWorldComparison {
        schema: EXPLAIN_HISTORY_VERSION.into(),
        before: before.clone(),
        after: after.clone(),
        before_resolution_hash: before_metadata.resolution_hash,
        after_resolution_hash: after_metadata.resolution_hash,
        before_project,
        after_project,
        activated,
        deactivated,
        active_changed,
        declaration_changed,
        availability_changed,
        target_effects_changed,
        warnings_changed,
        catalog_revision_changed,
        isolation_changed,
        materialization_changed,
        evidence,
    })
}

fn load_generation(
    home: &AikitHome,
    context: &ContextId,
    id: &GenerationId,
) -> Result<(GenerationMetadata, ResolvedView)> {
    let path = home.context_dir(context).join(GENERATIONS).join(id.as_str());
    if !is_generation(&path) {
        return Err(AikitError::new(
            "history.generation_not_found",
            format!("{id} is not a committed Generation for Context {context}"),
        ));
    }
    let metadata = read_metadata(&path)?;
    if &metadata.generation_id != id || metadata.context_id != context.to_string() {
        return Err(AikitError::new(
            "history.generation_identity_mismatch",
            format!("{} does not contain the requested Generation/Context identity", path.display()),
        ));
    }
    let view = read_lock(&path)?;
    if view.context.context_id != *context {
        return Err(AikitError::new(
            "history.generation_identity_mismatch",
            format!("{} lock belongs to a different Context", path.display()),
        ));
    }
    Ok((metadata, view))
}

fn generation_ref(id: &GenerationId) -> Result<ResourceRef> {
    ResourceRef::parse(&format!("generation/{id}"))
}

fn project_ref(view: &ResolvedView) -> Result<Option<ResourceRef>> {
    view.context
        .project_id
        .as_ref()
        .map(|project| ResourceRef::parse(&format!("project/{project}")))
        .transpose()
}

fn capsule_refs(
    ids: impl IntoIterator<Item = aikit_core::CapsuleId>,
) -> Result<Vec<ResourceRef>> {
    ids.into_iter()
        .map(|id| ResourceRef::parse(&id.to_string()))
        .collect()
}

fn changed_map_keys<K, V>(before: &BTreeMap<K, V>, after: &BTreeMap<K, V>) -> Vec<K>
where
    K: Ord + Clone,
    V: PartialEq,
{
    before
        .keys()
        .chain(after.keys())
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|key| before.get(key) != after.get(key))
        .collect()
}

fn changed_target_effects(before: &GenerationMetadata, after: &GenerationMetadata) -> Vec<String> {
    let before_targets = before
        .targets
        .iter()
        .map(|record| (&record.target, &record.effect))
        .collect::<BTreeMap<_, _>>();
    let after_targets = after
        .targets
        .iter()
        .map(|record| (&record.target, &record.effect))
        .collect::<BTreeMap<_, _>>();
    let targets = before_targets
        .keys()
        .chain(after_targets.keys())
        .copied()
        .collect::<BTreeSet<_>>();
    targets
        .into_iter()
        .filter(|target| before_targets.get(target) != after_targets.get(target))
        .map(ToOwned::to_owned)
        .collect()
}

fn join_refs(refs: &[ResourceRef]) -> String {
    refs.iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

fn plural(count: usize) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test)]
    fn changed_map_keys_reports_add_remove_and_value_change() {
        let before = BTreeMap::from([("a", 1), ("b", 2), ("c", 3)]);
        let after = BTreeMap::from([("b", 20), ("c", 3), ("d", 4)]);
        assert_eq!(changed_map_keys(&before, &after), vec!["a", "b", "d"]);
    }
}
