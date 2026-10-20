//! Read-only History projection over existing persistence authorities.
//!
//! There is intentionally no history database here. Generations remain immutable
//! generation directories, familiarity remains replayed event evidence, and
//! SessionSpace receipts remain part of the canonical SessionSpace document. This
//! module only projects those owners into the common `aikit-core` History grammar.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use aikit_core::resource::{ResourceRef, SourceAuthority};
use aikit_core::session_space::SessionSpaceRef;
use aikit_core::{
    familiarity_history_evidence, AikitError, ContextId, EvidenceProvenance, FamiliarityStore,
    HistoryEvidence, HistoryKind, HistoryReadModel, HistoryRecoverability, Result,
    EXPLAIN_HISTORY_VERSION,
};

use crate::derived_cache::{self, FileSignature};
use crate::generation::{
    is_generation, read_lock, read_metadata, GenerationMetadata, GENERATIONS, LOCK_FILE,
};
use crate::{
    AikitHome, KnowledgeApplicationReceipt, KnowledgeHistoryOperation,
    SessionSpaceApplicationStore, SessionSpaceReceipt,
};

/// Where the lock summaries are kept: derived data under the home's cache, never an authority.
const LOCK_SUMMARIES_FILE: &str = "history/generation-locks.v1.json";
const LOCK_SUMMARIES_VERSION: u32 = 1;

/// What the history reading takes from a generation's lock: the capsules it carried and its
/// Project. A lock lists a registry's worth of capsules with all their detail (hundreds of KB of
/// TOML), a generation is immutable, and a person's `aikit history` reads every one of them —
/// parsing 287 locks was 80% of a one-second reading — so what the reading needs is derived from
/// a lock once and kept, keyed by the lock file it came from (its length and modification time)
/// and the resolution it names. A lock that was rewritten, or a cache that is missing, unreadable
/// or of another version, derives again; nothing is ever read *from* the cache that the lock
/// would not say.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct LockSummary {
    lock: FileSignature,
    resolution_hash: String,
    project: Option<String>,
    /// Indexes into the cache's capsule table, in the lock's order.
    active: Vec<u32>,
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct LockSummaries {
    version: u32,
    /// Every capsule id any summary names, once.
    capsules: Vec<String>,
    generations: BTreeMap<String, LockSummary>,
}

/// The lock summaries of a run of reads: loaded once, added to as locks are derived, written once.
struct LockSummaryStore {
    path: PathBuf,
    kept: LockSummaries,
    index: HashMap<String, u32>,
    seen: BTreeSet<String>,
    dirty: bool,
}

/// A generation's lock as the history reading takes it.
struct LockFacts {
    project: Option<String>,
    active: Vec<String>,
}

impl LockSummaryStore {
    fn open(home: &AikitHome) -> Self {
        let path = derived_cache::path_in(home, LOCK_SUMMARIES_FILE);
        let kept = derived_cache::load::<LockSummaries>(&path)
            .filter(|kept| kept.version == LOCK_SUMMARIES_VERSION)
            .unwrap_or_else(|| LockSummaries {
                version: LOCK_SUMMARIES_VERSION,
                ..LockSummaries::default()
            });
        let index = kept
            .capsules
            .iter()
            .enumerate()
            .map(|(position, capsule)| (capsule.clone(), position as u32))
            .collect();
        Self {
            path,
            kept,
            index,
            seen: BTreeSet::new(),
            dirty: false,
        }
    }

    /// The facts of `generation_dir`'s lock: from the summary kept for it when the lock file and
    /// resolution are the ones it was derived from, else from the lock, which is then kept.
    fn facts(&mut self, generation_dir: &Path, metadata: &GenerationMetadata) -> Result<LockFacts> {
        let id = metadata.generation_id.to_string();
        self.seen.insert(id.clone());
        let lock = FileSignature::of(&generation_dir.join(LOCK_FILE));
        if let Some(summary) = self.kept.generations.get(&id) {
            if summary.lock == lock
                && summary.resolution_hash == metadata.resolution_hash
                && summary
                    .active
                    .iter()
                    .all(|&position| (position as usize) < self.kept.capsules.len())
            {
                return Ok(LockFacts {
                    project: summary.project.clone(),
                    active: summary
                        .active
                        .iter()
                        .map(|&position| self.kept.capsules[position as usize].clone())
                        .collect(),
                });
            }
        }
        let resolved = read_lock(generation_dir)?;
        let active: Vec<String> = resolved.active.keys().map(ToString::to_string).collect();
        let project = resolved
            .context
            .project_id
            .as_ref()
            .map(ToString::to_string);
        let positions = active
            .iter()
            .map(|capsule| {
                *self.index.entry(capsule.clone()).or_insert_with(|| {
                    self.kept.capsules.push(capsule.clone());
                    (self.kept.capsules.len() - 1) as u32
                })
            })
            .collect();
        self.kept.generations.insert(
            id,
            LockSummary {
                lock,
                resolution_hash: metadata.resolution_hash.clone(),
                project: project.clone(),
                active: positions,
            },
        );
        self.dirty = true;
        Ok(LockFacts { project, active })
    }

    /// Keep what was derived. A sweep of every context (`prune`) also lets go of the summaries of
    /// generations that are gone. A cache that cannot be written is a cache not kept: the reading
    /// it would have saved is simply made again.
    fn save(mut self, prune: bool) {
        if prune {
            let before = self.kept.generations.len();
            let seen = &self.seen;
            self.kept.generations.retain(|id, _| seen.contains(id));
            self.dirty |= self.kept.generations.len() != before;
        }
        if !self.dirty {
            return;
        }
        derived_cache::save(&self.path, &self.kept);
    }
}

/// Read immutable generation evidence for one canonical Context. A previous
/// Generation is inspectable historical ground; this function does not invent a
/// generic arbitrary-generation restore operation.
pub fn generation_history_evidence(
    home: &AikitHome,
    context: &ContextId,
) -> Result<Vec<HistoryEvidence>> {
    let mut locks = LockSummaryStore::open(home);
    let entries = generation_history_evidence_with(home, context, &mut locks);
    locks.save(false);
    entries
}

fn generation_history_evidence_with(
    home: &AikitHome,
    context: &ContextId,
    locks: &mut LockSummaryStore,
) -> Result<Vec<HistoryEvidence>> {
    let root = home.context_dir(context).join(GENERATIONS);
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut entries = Vec::new();
    for entry in fs::read_dir(&root).map_err(|error| {
        AikitError::new(
            "history.generation_list_failed",
            format!("could not list {}: {error}", root.display()),
        )
    })? {
        let entry = entry.map_err(|error| {
            AikitError::new(
                "history.generation_list_failed",
                format!("could not read {}: {error}", root.display()),
            )
        })?;
        let path = entry.path();
        if !path.is_dir() || !is_generation(&path) {
            continue;
        }
        let metadata = read_metadata(&path)?;
        let lock = locks.facts(&path, &metadata)?;
        let subject = ResourceRef::parse(&format!("generation/{}", metadata.generation_id))?;
        let mut canonical_refs = BTreeSet::new();
        canonical_refs.insert(subject.clone());
        // The lock names every capsule this generation actually carried, so a
        // skill's history answers through the generations that enabled it —
        // `history --resource skill/…` was empty before this joined them.
        for capsule in &lock.active {
            if let Ok(reference) = ResourceRef::parse(capsule) {
                canonical_refs.insert(reference);
            }
        }
        if let Some(project) = lock.project.as_ref() {
            canonical_refs.insert(ResourceRef::parse(&format!("project/{project}"))?);
        }

        let mut details = BTreeMap::new();
        details.insert("context".into(), metadata.context_id.clone());
        details.insert("resolutionHash".into(), metadata.resolution_hash.clone());
        details.insert("catalogRevision".into(), metadata.catalog_revision.clone());
        details.insert("isolation".into(), format!("{:?}", metadata.isolation));
        details.insert(
            "materialization".into(),
            format!("{:?}", metadata.materialization),
        );
        if let Some(project) = lock.project.as_ref() {
            details.insert("project".into(), project.clone());
        }
        if let Some(base) = &metadata.base_generation {
            details.insert("baseGeneration".into(), base.to_string());
        }
        if !metadata.targets.is_empty() {
            details.insert(
                "targetEffects".into(),
                metadata
                    .targets
                    .iter()
                    .map(|target| format!("{}:{}", target.target, target.effect))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
        }
        if !metadata.notes.is_empty() {
            details.insert("notes".into(), metadata.notes.join(" | "));
        }

        let nanos = metadata.created_at.as_nanos().max(0) as u128;
        entries.push(HistoryEvidence {
            schema: EXPLAIN_HISTORY_VERSION.into(),
            id: metadata.generation_id.to_string(),
            kind: HistoryKind::Generation,
            subject: subject.clone(),
            authorities: vec![SourceAuthority::Generated],
            occurred_at_unix_ms: Some(nanos / 1_000_000),
            summary: format!(
                "generation {} · resolution {} · catalog {}",
                metadata.generation_id, metadata.resolution_hash, metadata.catalog_revision
            ),
            canonical_refs: canonical_refs.into_iter().collect(),
            provenance: Vec::new(),
            recoverability: HistoryRecoverability::InspectOnly,
            details,
        });
    }
    entries.sort_by(|left, right| {
        right
            .occurred_at_unix_ms
            .cmp(&left.occurred_at_unix_ms)
            .then_with(|| right.id.cmp(&left.id))
    });
    Ok(entries)
}

/// Generation evidence across every context recorded under this home.
///
/// Applies historically minted a fresh context whenever the resolution
/// identity changed, so reading only the caller's current context hid most of
/// the lifecycle from `aikit history`. Generations are immutable,
/// content-addressed evidence; sweeping contexts changes no authority and
/// loses nothing. Identical content minted into two contexts is one
/// generation here: deduplicated by id, with the context carried in details.
pub fn all_contexts_generation_history_evidence(home: &AikitHome) -> Result<Vec<HistoryEvidence>> {
    let mut entries: Vec<HistoryEvidence> = Vec::new();
    let mut seen = BTreeSet::new();
    let Ok(contexts) = fs::read_dir(home.contexts()) else {
        return Ok(entries);
    };
    let mut locks = LockSummaryStore::open(home);
    let mut names: Vec<String> = contexts
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
        .collect();
    names.sort();
    for name in names {
        let Ok(context) = ContextId::parse(&name) else {
            continue;
        };
        for mut entry in generation_history_evidence_with(home, &context, &mut locks)? {
            if seen.insert(entry.id.clone()) {
                // Name the context DIRECTORY that holds this generation: it
                // is the location identity the sweep adds over a single-
                // context read.
                entry.details.insert("context".into(), name.clone());
                entries.push(entry);
            }
        }
    }
    locks.save(true);
    entries.sort_by(|left, right| {
        right
            .occurred_at_unix_ms
            .cmp(&left.occurred_at_unix_ms)
            .then_with(|| right.id.cmp(&left.id))
    });
    Ok(entries)
}

/// Read-only projections of the registered skill sources: one entry per
/// registration and one per immutable snapshot, with every snapshot's skill
/// ids carried as canonical refs so a skill's history reaches back to its
/// promotion. Snapshot persistence records no wall-clock time, so these
/// entries carry none rather than inventing one.
pub fn source_history_evidence(home: &AikitHome) -> Result<Vec<HistoryEvidence>> {
    let mut entries = Vec::new();
    let sources = home.root().join("sources");
    let Ok(ids) = fs::read_dir(&sources) else {
        return Ok(entries);
    };
    let mut ids: Vec<String> = ids
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
        .collect();
    ids.sort();

    for id in ids {
        let subject = ResourceRef::parse(&format!("source/{id}"))?;
        let spec_text =
            fs::read_to_string(sources.join(&id).join("source.toml")).map_err(|error| {
                AikitError::new(
                    "history.source_unreadable",
                    format!("could not read the registration of source `{id}`: {error}"),
                )
            })?;
        let spec: toml::Value = toml::from_str(&spec_text).map_err(|error| {
            AikitError::new(
                "history.source_unreadable",
                format!("the registration of source `{id}` is not readable TOML: {error}"),
            )
        })?;

        let kind = spec
            .get("kind")
            .and_then(|kind| kind.as_table())
            .and_then(|table| table.iter().next())
            .map(|(name, value)| (name.to_string(), value.clone()));
        let mut details = BTreeMap::new();
        if let Some((name, value)) = &kind {
            details.insert("kind".into(), name.clone());
            if let Some(repository) = value.get("repository").and_then(|v| v.as_str()) {
                details.insert("repository".into(), repository.to_string());
            }
            if let Some(revision) = value.get("revision").and_then(|v| v.as_str()) {
                details.insert("revision".into(), revision.to_string());
            }
            if let Some(path) = value.get("path").and_then(|v| v.as_str()) {
                details.insert("path".into(), path.to_string());
            }
        }

        entries.push(HistoryEvidence {
            schema: EXPLAIN_HISTORY_VERSION.into(),
            id: format!("source:{id}"),
            kind: HistoryKind::Source,
            subject: subject.clone(),
            authorities: vec![SourceAuthority::Generated],
            occurred_at_unix_ms: None,
            summary: match &kind {
                Some((name, value)) => {
                    let repository = value.get("repository").and_then(|v| v.as_str());
                    let revision = value.get("revision").and_then(|v| v.as_str());
                    let path = value.get("path").and_then(|v| v.as_str());
                    match (name.as_str(), repository, revision, path) {
                        ("git", Some(repo), Some(rev), _) => {
                            format!(
                                "skill source `{id}` registered · git {repo} @ {}",
                                &rev[..rev.len().min(12)]
                            )
                        }
                        ("directory", _, _, Some(path)) => {
                            format!("skill source `{id}` registered · directory {path}")
                        }
                        _ => format!("skill source `{id}` registered"),
                    }
                }
                None => format!("skill source `{id}` registered"),
            },
            canonical_refs: vec![subject.clone()],
            provenance: Vec::new(),
            recoverability: HistoryRecoverability::InspectOnly,
            details,
        });

        // Snapshots: candidate first, then whatever the state marks active,
        // then the rest — but every snapshot is projected, promoted or not.
        let state: toml::Value = toml::from_str(
            &fs::read_to_string(sources.join(&id).join("state.toml")).unwrap_or_default(),
        )
        .unwrap_or(toml::Value::Table(toml::map::Map::new()));
        let active = state
            .get("active_snapshot")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let candidate = state
            .get("candidate_snapshot")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let rollback_points = state
            .get("history")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);

        let snapshot_files = fs::read_dir(sources.join(&id).join("snapshots"));
        let mut digests: Vec<String> = match snapshot_files {
            Ok(files) => files
                .filter_map(std::result::Result::ok)
                .filter(|entry| entry.path().is_dir())
                .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
                .collect(),
            Err(_) => Vec::new(),
        };
        digests.sort();

        for digest in digests {
            let record_text = fs::read_to_string(
                sources
                    .join(&id)
                    .join("snapshots")
                    .join(&digest)
                    .join("snapshot.toml"),
            )
            .unwrap_or_default();
            let record: toml::Value =
                toml::from_str(&record_text).unwrap_or(toml::Value::Table(toml::map::Map::new()));
            let mut canonical_refs = BTreeSet::new();
            canonical_refs.insert(subject.clone());
            if let Some(skills) = record.get("skills").and_then(|v| v.as_array()) {
                for skill in skills {
                    if let Some(skill_id) = skill.get("id").and_then(|v| v.as_str()) {
                        if let Ok(reference) = ResourceRef::parse(skill_id) {
                            canonical_refs.insert(reference);
                        }
                    }
                }
            }
            let skill_count = record
                .get("skills")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            let mut details = BTreeMap::new();
            details.insert("digest".into(), digest.clone());
            details.insert(
                "state".into(),
                if active.as_deref() == Some(digest.as_str()) {
                    "active".to_string()
                } else if candidate.as_deref() == Some(digest.as_str()) {
                    "candidate".to_string()
                } else {
                    "retained".to_string()
                },
            );
            if rollback_points > 0 {
                details.insert("rollbackPoints".into(), rollback_points.to_string());
            }
            entries.push(HistoryEvidence {
                schema: EXPLAIN_HISTORY_VERSION.into(),
                id: format!("source:{id}:{digest}"),
                kind: HistoryKind::Source,
                subject: subject.clone(),
                authorities: vec![SourceAuthority::Generated],
                occurred_at_unix_ms: None,
                summary: format!(
                    "snapshot {} · {} skill{} · {}",
                    &digest[..digest.len().min(12)],
                    skill_count,
                    if skill_count == 1 { "" } else { "s" },
                    if active.as_deref() == Some(digest.as_str()) {
                        "active"
                    } else if candidate.as_deref() == Some(digest.as_str()) {
                        "candidate"
                    } else {
                        "retained"
                    },
                ),
                canonical_refs: canonical_refs.into_iter().collect(),
                provenance: Vec::new(),
                recoverability: HistoryRecoverability::InspectOnly,
                details,
            });
        }
    }
    Ok(entries)
}

/// Project learned accessibility events without changing their epistemic class.
pub fn familiarity_history_evidence_model(store: &FamiliarityStore) -> HistoryReadModel {
    let mut observations = store.snapshot().observations;
    observations.sort_by(|left, right| {
        right
            .observed_at_ms
            .cmp(&left.observed_at_ms)
            .then_with(|| right.observation_id.cmp(&left.observation_id))
    });
    HistoryReadModel::new(
        observations
            .iter()
            .map(familiarity_history_evidence)
            .collect(),
    )
}

/// Project one durable AIKit-owned Knowledge operation receipt into the common
/// timeline. Provider/source semantics remain in their providers; this is only the
/// audit evidence that AIKit actually traversed a route or materialised a frame.
pub fn knowledge_application_receipt_evidence(
    receipt: &KnowledgeApplicationReceipt,
) -> Result<HistoryEvidence> {
    let mut canonical_refs = BTreeSet::new();
    let mut authorities = vec![SourceAuthority::Generated];
    let mut provenance = Vec::new();
    let mut details = BTreeMap::new();
    details.insert("sequence".into(), receipt.sequence.to_string());

    let (kind, subject, summary, recoverability) = match receipt.operation {
        KnowledgeHistoryOperation::Route => {
            let route = receipt.route.as_ref().ok_or_else(|| {
                AikitError::new(
                    "history.knowledge_route_receipt_invalid",
                    format!("{} contains no KnowledgeRoute", receipt.receipt_id),
                )
            })?;
            canonical_refs.insert(route.route.clone());
            if let Some(query) = &route.query {
                details.insert("query".into(), query.clone());
            }
            details.insert("steps".into(), route.steps.len().to_string());
            for step in &route.steps {
                canonical_refs.insert(step.resource.clone());
                if !authorities.contains(&step.authority) {
                    authorities.push(step.authority);
                }
                provenance.push(EvidenceProvenance {
                    provider: step
                        .provider
                        .as_ref()
                        .and_then(|provider| ResourceRef::parse(&provider.to_string()).ok()),
                    source: Some(step.resource.clone()),
                    lens: step.lens.clone(),
                    revision: step.revision.clone(),
                    native_id: None,
                });
            }
            (
                HistoryKind::KnowledgeRoute,
                route.route.clone(),
                format!(
                    "Knowledge route {} · {} step{}",
                    route.route,
                    route.steps.len(),
                    if route.steps.len() == 1 { "" } else { "s" }
                ),
                HistoryRecoverability::ReplayNavigation,
            )
        }
        KnowledgeHistoryOperation::Frame => {
            let frame = receipt.frame.as_ref().ok_or_else(|| {
                AikitError::new(
                    "history.knowledge_frame_receipt_invalid",
                    format!("{} contains no Knowledge context frame", receipt.receipt_id),
                )
            })?;
            let subject = ResourceRef::parse(&receipt.receipt_id)?;
            canonical_refs.insert(subject.clone());
            canonical_refs.extend(frame.selected.iter().cloned());
            details.insert("readings".into(), frame.readings.len().to_string());
            details.insert("routes".into(), frame.routes.len().to_string());
            details.insert("absences".into(), frame.absences.len().to_string());
            details.insert(
                "contradictions".into(),
                frame.contradictions.len().to_string(),
            );
            details.insert(
                "openQuestions".into(),
                frame.open_questions.len().to_string(),
            );
            for reading in &frame.readings {
                canonical_refs.insert(reading.resource.clone());
                if !authorities.contains(&reading.authority) {
                    authorities.push(reading.authority);
                }
                provenance.push(EvidenceProvenance {
                    provider: reading
                        .provider
                        .as_ref()
                        .and_then(|provider| ResourceRef::parse(&provider.to_string()).ok()),
                    source: Some(reading.resource.clone()),
                    lens: reading.lens.clone(),
                    revision: reading.revision.clone(),
                    native_id: None,
                });
            }
            for route in &frame.routes {
                canonical_refs.insert(route.route.clone());
                for step in &route.steps {
                    canonical_refs.insert(step.resource.clone());
                    if !authorities.contains(&step.authority) {
                        authorities.push(step.authority);
                    }
                }
            }
            (
                HistoryKind::KnowledgeFrame,
                subject,
                format!(
                    "Knowledge frame · {} reading{} · {} route{} · {} absence{}",
                    frame.readings.len(),
                    if frame.readings.len() == 1 { "" } else { "s" },
                    frame.routes.len(),
                    if frame.routes.len() == 1 { "" } else { "s" },
                    frame.absences.len(),
                    if frame.absences.len() == 1 { "" } else { "s" },
                ),
                HistoryRecoverability::InspectOnly,
            )
        }
    };

    canonical_refs.insert(subject.clone());
    Ok(HistoryEvidence {
        schema: EXPLAIN_HISTORY_VERSION.into(),
        id: receipt.receipt_id.clone(),
        kind,
        subject,
        authorities,
        occurred_at_unix_ms: Some(u128::from(receipt.recorded_at_ms)),
        summary,
        canonical_refs: canonical_refs.into_iter().collect(),
        provenance,
        recoverability,
        details,
    })
}

/// Project canonical SessionSpace receipts. The receipt is generated evidence of
/// authored semantic state, so both classes remain visible. Restoration is not a
/// direct History write: it must go through `stage_restore` and the normal current
/// basis/apply authority.
pub fn session_space_history_evidence(
    store: &SessionSpaceApplicationStore,
    space: &SessionSpaceRef,
) -> Result<Vec<HistoryEvidence>> {
    store
        .history(space)?
        .iter()
        .map(session_space_receipt_evidence)
        .collect()
}

pub fn session_space_receipt_evidence(receipt: &SessionSpaceReceipt) -> Result<HistoryEvidence> {
    let subject = receipt.space.as_resource_ref().clone();
    let mut canonical_refs = BTreeSet::new();
    canonical_refs.insert(subject.clone());
    collect_session_space_refs(&receipt.after, &mut canonical_refs)?;

    let mut details = BTreeMap::new();
    details.insert("sequence".into(), receipt.sequence.to_string());
    details.insert(
        "resultingRevision".into(),
        receipt.resulting_basis.revision.to_string(),
    );
    details.insert(
        "resultingStateHash".into(),
        receipt.resulting_basis.state_hash.clone(),
    );
    details.insert("operation".into(), format!("{:?}", receipt.operation));
    if !receipt.changed.is_empty() {
        details.insert(
            "changed".into(),
            receipt
                .changed
                .iter()
                .map(|change| format!("{change:?}"))
                .collect::<Vec<_>>()
                .join(", "),
        );
    }

    Ok(HistoryEvidence {
        schema: EXPLAIN_HISTORY_VERSION.into(),
        id: format!("session-space:{}:{}", receipt.space, receipt.sequence),
        kind: HistoryKind::SessionSpace,
        subject,
        authorities: vec![SourceAuthority::Authored, SourceAuthority::Generated],
        occurred_at_unix_ms: Some(receipt.applied_at_unix_ms),
        summary: format!(
            "SessionSpace {} receipt {} · {} semantic change{}",
            receipt.space,
            receipt.sequence,
            receipt.changed.len(),
            if receipt.changed.len() == 1 { "" } else { "s" }
        ),
        canonical_refs: canonical_refs.into_iter().collect(),
        provenance: Vec::new(),
        recoverability: HistoryRecoverability::RestageThroughCurrentAuthority,
        details,
    })
}

fn collect_session_space_refs(
    state: &aikit_core::session_space_application::SessionSpaceAuthoredState,
    refs: &mut BTreeSet<ResourceRef>,
) -> Result<()> {
    for (project, context) in &state.project_contexts {
        refs.insert(ResourceRef::parse(&project.to_string())?);
        refs.insert(context.reference.as_resource_ref().clone());
        refs.extend(context.basis.context_sources.iter().cloned());
        if let Some(host) = &context.basis.host {
            refs.insert(host.clone());
        }
    }
    refs.extend(state.agent_sessions.keys().cloned());
    for attachment in state.surfaces.values() {
        refs.insert(attachment.surface.clone());
        if let Some(component) = &attachment.component {
            refs.insert(component.clone());
        }
    }
    for binding in state.native_references.values() {
        refs.insert(binding.reference.clone());
        refs.extend(binding.owner.iter().cloned());
        refs.extend(binding.provider.iter().cloned());
        refs.extend(binding.host.iter().cloned());
    }
    if let Some(focus) = &state.focus {
        refs.insert(focus.target.clone());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_core::session_space_application::{SessionSpaceFocus, SessionSpaceMutation};

    #[test]
    fn session_space_history_keeps_authored_and_generated_truth_separate_from_restore() {
        let dir = tempfile::tempdir().unwrap();
        let home = AikitHome::at(dir.path());
        home.ensure_layout().unwrap();
        let store = SessionSpaceApplicationStore::new(home);
        let space = SessionSpaceRef::parse("session-space/history").unwrap();
        let create = store
            .stage(
                None,
                SessionSpaceMutation::Create {
                    id: space.clone(),
                    label: Some("History".into()),
                },
            )
            .unwrap();
        store.apply(&create).unwrap();
        let focus = store
            .stage(
                Some(&space),
                SessionSpaceMutation::Focus {
                    focus: Some(SessionSpaceFocus {
                        target: ResourceRef::parse("surface/editor").unwrap(),
                        region: Some("main".into()),
                        provenance: vec!["test".into()],
                    }),
                },
            )
            .unwrap();
        store.apply(&focus).unwrap();

        let history = session_space_history_evidence(&store, &space).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(
            history[1].authorities,
            vec![SourceAuthority::Authored, SourceAuthority::Generated]
        );
        assert_eq!(
            history[1].recoverability,
            HistoryRecoverability::RestageThroughCurrentAuthority
        );
        assert!(history[1]
            .canonical_refs
            .contains(&ResourceRef::parse("surface/editor").unwrap()));

        let before = store.load(&space).unwrap();
        let restore_preview = store.stage_restore(&space, 0).unwrap();
        assert_eq!(store.load(&space).unwrap(), before);
        assert!(matches!(
            restore_preview.intent,
            SessionSpaceMutation::Restore { .. }
        ));
    }

    #[test]
    fn familiarity_projection_never_changes_learned_authority() {
        let mut store = FamiliarityStore::new();
        store
            .record(aikit_core::FamiliarityObservation::destination(
                "obs-1",
                ResourceRef::parse("project/app").unwrap(),
                aikit_core::FamiliarityContext::default(),
                10,
            ))
            .unwrap();
        let history = familiarity_history_evidence_model(&store);
        assert_eq!(history.entries.len(), 1);
        assert_eq!(
            history.entries[0].authorities,
            vec![SourceAuthority::Learned]
        );
    }
}
