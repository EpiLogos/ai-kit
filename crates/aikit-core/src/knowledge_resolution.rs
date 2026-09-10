//! U3.1 owner-side resolution rows for the search aperture.
//!
//! One canonical owner resolution operation covers the real provider set —
//! Central sources/files, Flows, skills and knowledge subjects — and every row
//! is a ref carrying its owner, its provenance and the canonical Actions
//! available on it. Invoking one of those Actions forwards the ref to its
//! native owner; the resolution surface itself never becomes a parallel
//! command family.
//!
//! Resolution is deliberately inert: querying, displaying and refreshing
//! record nothing. Only an explicit `knowledge open` of a row records one
//! successful-use familiarity observation, through the same owner operation.

use serde::{Deserialize, Serialize};

use crate::resource::ResourceRef;

pub const KNOWLEDGE_RESOLUTION_VERSION: &str = "aikit.knowledge-resolution/v1";

/// Canonical Actions a file (Central source) row forwards to.
pub const ACTION_KNOWLEDGE_READ: &str = "knowledge/read";
pub const ACTION_KNOWLEDGE_SOURCES: &str = "knowledge/sources";
pub const ACTION_KNOWLEDGE_RELATIONS: &str = "knowledge/relations";
pub const ACTION_KNOWLEDGE_EXPLAIN: &str = "knowledge/explain";
pub const ACTION_KNOWLEDGE_ROUTE: &str = "knowledge/route";
pub const ACTION_KNOWLEDGE_OPEN: &str = "knowledge/open";
pub const ACTION_RUN: &str = "run";
pub const ACTION_SKILL_OVERLAY_SET: &str = "skill/overlay/set";
/// The Flow owner's contemplation Action, owned by `aikit-core/src/flow.rs`.
pub const ACTION_CONTEMPLATE_FLOW: &str = "action:contemplate-flow";

/// Which real provider one resolution row belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResolutionKind {
    /// A Central source / file carried by the native SourcePool.
    File,
    /// An owner-supplied Flow thread (a SemanticWiki node of type `flow`).
    Flow,
    /// A catalogued Skill in the resolved capability view.
    Skill,
    /// Any other SemanticWiki subject node.
    KnowledgeSubject,
}

impl ResolutionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Flow => "flow",
            Self::Skill => "skill",
            Self::KnowledgeSubject => "knowledge-subject",
        }
    }
}

/// One row of the owner resolution surface: a ref carrying its owner,
/// provenance and the canonical Actions available on it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolutionRow {
    pub reference: ResourceRef,
    pub kind: ResolutionKind,
    pub label: String,
    /// The owning provider/operation this ref forwards to.
    pub owner: String,
    pub provenance: Vec<String>,
    /// Canonical Actions available on this ref, in stable order.
    pub actions: Vec<String>,
}

/// A provider that could not contribute rows, disclosed explicitly instead of
/// being silently omitted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnavailableProvider {
    pub provider: String,
    /// `unavailable` (present but degraded/absent) or `excluded` (declared
    /// out of scope for this horizon).
    pub state: String,
    pub detail: String,
}

/// The full owner resolution result for one query.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeResolution {
    pub version: String,
    pub query: String,
    pub rows: Vec<ResolutionRow>,
    pub unavailable: Vec<UnavailableProvider>,
    pub absences: Vec<String>,
}

impl KnowledgeResolution {
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            version: KNOWLEDGE_RESOLUTION_VERSION.into(),
            query: query.into(),
            rows: Vec::new(),
            unavailable: Vec::new(),
            absences: Vec::new(),
        }
    }

    /// Offer one candidate row; the query filter decides whether it enters.
    pub fn offer_row(&mut self, row: ResolutionRow) {
        if row_matches(&row, &self.query) {
            self.rows.push(row);
        }
    }

    /// Disclose providers that could not contribute rows, classified from the
    /// runtime's own absence statements so nothing is silently omitted.
    pub fn disclose_absences(&mut self, absences: &[String]) {
        self.absences.extend(absences.iter().cloned());
        for absence in absences {
            let entry = classify_absence(absence);
            if !self
                .unavailable
                .iter()
                .any(|existing| existing.provider == entry.provider)
            {
                self.unavailable.push(entry);
            }
        }
    }

    /// Stable finish: rows sort by provider kind then ref, so the aperture
    /// renders a deterministic list between keystrokes.
    pub fn finish(mut self, limit: usize) -> Self {
        self.rows.sort_by(|left, right| {
            left.kind
                .cmp(&right.kind)
                .then_with(|| left.reference.cmp(&right.reference))
        });
        if limit > 0 {
            self.rows.truncate(limit);
        }
        self
    }
}

/// A query matches a row when it case-insensitively hits the ref, the label,
/// the owner or any provenance entry. An empty query matches every row.
pub fn row_matches(row: &ResolutionRow, query: &str) -> bool {
    let query = query.trim();
    if query.is_empty() {
        return true;
    }
    let needle = query.to_lowercase();
    row.reference.as_str().to_lowercase().contains(&needle)
        || row.label.to_lowercase().contains(&needle)
        || row.owner.to_lowercase().contains(&needle)
        || row
            .provenance
            .iter()
            .any(|entry| entry.to_lowercase().contains(&needle))
}

/// Map one runtime absence statement onto the provider it names. Unknown
/// statements fall through to the horizon `environment` bucket rather than
/// being dropped.
fn classify_absence(absence: &str) -> UnavailableProvider {
    let provider = if absence.to_lowercase().contains("bkmr") {
        "source-pool/bkmr"
    } else if absence.contains("GitNexus") || absence.contains("CodeIndex") {
        "code-index/gitnexus"
    } else if absence.contains("Central wiki") || absence.contains("Central world sources") {
        "wiki/central"
    } else if absence.contains("SemanticWiki") || absence.contains("Semantic wiki") {
        "semantic-wiki"
    } else if absence.contains("SourcePool") || absence.contains("source pool") {
        "source-pool/native"
    } else if absence.contains("ProjectMap") || absence.contains("project map") {
        "project-map"
    } else if absence.contains("registr") {
        "registry"
    } else {
        "environment"
    };
    UnavailableProvider {
        provider: provider.into(),
        state: "unavailable".into(),
        detail: absence.to_string(),
    }
}

/// Receipt for one explicit `knowledge open`: the ref resolved, read
/// successfully through the owner operation, and exactly one successful-use
/// familiarity observation was recorded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeOpenReceipt {
    pub opened: ResourceRef,
    pub address: crate::knowledge_navigation::KnowledgeAddress,
    pub provider: Option<String>,
    pub recorded: String,
    pub observation_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(raw: &str) -> ResourceRef {
        ResourceRef::parse(raw).unwrap()
    }

    fn row(kind: ResolutionKind, reference: &str, label: &str, owner: &str) -> ResolutionRow {
        ResolutionRow {
            reference: r(reference),
            kind,
            label: label.into(),
            owner: owner.into(),
            provenance: vec!["fixture-origin".into()],
            actions: vec![ACTION_KNOWLEDGE_OPEN.into()],
        }
    }

    #[test]
    fn empty_query_matches_every_row_and_finish_sorts_by_kind_then_ref() {
        let mut resolution = KnowledgeResolution::new("");
        resolution.offer_row(row(
            ResolutionKind::KnowledgeSubject,
            "wiki:node:b",
            "B",
            "semantic-wiki",
        ));
        resolution.offer_row(row(
            ResolutionKind::File,
            "source:file:a",
            "A",
            "provider/source-pool/native",
        ));
        resolution.offer_row(row(
            ResolutionKind::File,
            "source:file:c",
            "C",
            "provider/source-pool/native",
        ));
        let resolution = resolution.finish(0);
        assert_eq!(resolution.rows.len(), 3);
        assert_eq!(resolution.rows[0].kind, ResolutionKind::File);
        assert_eq!(resolution.rows[0].reference.as_str(), "source:file:a");
        assert_eq!(resolution.rows[2].kind, ResolutionKind::KnowledgeSubject);
    }

    #[test]
    fn query_filters_case_insensitively_across_ref_label_owner_and_provenance() {
        let mut resolution = KnowledgeResolution::new("FLOW");
        resolution.offer_row(row(
            ResolutionKind::Flow,
            "wiki:node:staged/flow-note",
            "note",
            "semantic-wiki",
        ));
        resolution.offer_row(row(
            ResolutionKind::File,
            "source:file:other",
            "Other",
            "provider/source-pool/native",
        ));
        let resolution = resolution.finish(0);
        assert_eq!(resolution.rows.len(), 1);
        assert_eq!(resolution.rows[0].kind, ResolutionKind::Flow);
    }

    #[test]
    fn limit_truncates_after_sorting() {
        let mut resolution = KnowledgeResolution::new("");
        for name in ["a", "b", "c"] {
            resolution.offer_row(row(
                ResolutionKind::File,
                &format!("source:file:{name}"),
                name,
                "provider/source-pool/native",
            ));
        }
        let resolution = resolution.finish(2);
        assert_eq!(resolution.rows.len(), 2);
        assert_eq!(resolution.rows[0].reference.as_str(), "source:file:a");
    }

    #[test]
    fn absences_surface_as_explicit_unavailable_states() {
        let mut resolution = KnowledgeResolution::new("");
        resolution.disclose_absences(&[
            "GitNexus CodeIndex unavailable for this Project".into(),
            "bkmr SourcePool configured but provider executable is unavailable".into(),
            "something wholly unexpected".into(),
        ]);
        assert_eq!(resolution.unavailable.len(), 3);
        assert!(resolution
            .unavailable
            .iter()
            .any(|entry| entry.provider == "code-index/gitnexus" && entry.state == "unavailable"));
        assert!(resolution
            .unavailable
            .iter()
            .any(|entry| entry.provider == "source-pool/bkmr"));
        assert!(resolution
            .unavailable
            .iter()
            .any(|entry| entry.provider == "environment"));
        assert_eq!(resolution.absences.len(), 3);
    }

    #[test]
    fn duplicate_provider_absences_disclose_once() {
        let mut resolution = KnowledgeResolution::new("");
        resolution.disclose_absences(&[
            "bkmr SourcePool degraded: one".into(),
            "bkmr SourcePool configured but provider executable is unavailable".into(),
        ]);
        assert_eq!(
            resolution
                .unavailable
                .iter()
                .filter(|entry| entry.provider == "source-pool/bkmr")
                .count(),
            1
        );
    }

    #[test]
    fn canonical_action_names_are_stable_refs() {
        assert_eq!(ACTION_KNOWLEDGE_OPEN, "knowledge/open");
        assert_eq!(ACTION_CONTEMPLATE_FLOW, "action:contemplate-flow");
        assert_eq!(
            ResolutionKind::KnowledgeSubject.as_str(),
            "knowledge-subject"
        );
        assert_eq!(ResolutionKind::File.as_str(), "file");
    }
}
