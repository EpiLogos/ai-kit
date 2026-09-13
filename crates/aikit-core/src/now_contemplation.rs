//! Owner-side NOW contemplation: the typed `Contemplate(now_ref)` reading.
//!
//! The founding shape (Central #175, cells 1–2): a NOW is a task boundary;
//! `T/` is the raw contemplative stream of one clearing and the prime stream
//! holds learnings distilled from T. Contemplate is the process that turns T
//! into T-prime — parsing the signal out of the raw stream. Its output home
//! is the NOW's own T/T' system, not knowledge nodes.
//!
//! Division of ownership: the caller (O-I kernel) fetches the NOW's raw
//! stream from Central (`central.now.thoughts.read`) and supplies it as a
//! seam, verbatim — this module fabricates no seam and never reads Central.
//! The executor parses signal; the distilled learning leaves here as an
//! unapplied proposal, and the T-prime write belongs to the caller through
//! Central's `central.now.learnings.distill` Action. Without a host executor
//! the reading is explicitly `unavailable` — contemplate is never
//! auto-invoked, and preflight stays first.
use serde::{Deserialize, Serialize};

use crate::explain_history::{EvidenceProvenance, ExplainEvidence, ExplainFact};
use crate::resource::{ResourceRef, SourceAuthority};
use crate::{AikitError, Result};

pub const NOW_CONTEMPLATION_VERSION: &str = "aikit.now-contemplation/v1";

/// Central's reading the seam carries, verbatim from
/// `central.now.thoughts.read`.
pub const THOUGHTS_READING_SCHEMA: &str = "central.thoughts-reading/v1";

/// One raw contemplative fixture as Central listed it. Fields beyond the
/// file/revision identity are Central's own attribution facts and ride
/// verbatim; `content` is present only when the caller asked for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NowFixture {
    pub file: String,
    pub revision: String,
    #[serde(default)]
    pub conforming: bool,
    #[serde(default)]
    pub day: Option<String>,
    #[serde(default)]
    pub actor: Option<String>,
    #[serde(default)]
    pub actor_kind: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
}

/// The caller-supplied raw-stream seam: Central's thoughts reading carried
/// verbatim. `include_content` must have been requested for execution; a
/// seam without bodies supports a factual preflight only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NowFixturesSeam {
    pub schema: String,
    pub now_ref: String,
    #[serde(default)]
    pub total: usize,
    #[serde(default)]
    pub truncated: bool,
    #[serde(default)]
    pub fixtures: Vec<NowFixture>,
}

impl NowFixturesSeam {
    pub fn parse(input: &str) -> Result<Self> {
        let seam: Self = serde_json::from_str(input).map_err(|error| {
            AikitError::new(
                "now.fixtures_seam_invalid",
                format!("the fixtures seam must be the NOW's thoughts reading JSON: {error}"),
            )
        })?;
        if seam.schema != THOUGHTS_READING_SCHEMA {
            return Err(AikitError::new(
                "now.fixtures_seam_schema_unsupported",
                format!(
                    "fixtures seam schema `{}` is not `{THOUGHTS_READING_SCHEMA}`",
                    seam.schema
                ),
            ));
        }
        if seam.now_ref.is_empty() {
            return Err(AikitError::new(
                "now.fixtures_seam_without_now",
                "the fixtures seam names no now_ref",
            ));
        }
        if seam.truncated {
            return Err(AikitError::new(
                "now.fixtures_seam_truncated",
                "the fixtures seam is a truncated reading; contemplate refuses a partial stream — re-read with a higher limit",
            ));
        }
        for fixture in &seam.fixtures {
            if fixture.file.is_empty() || fixture.revision.is_empty() {
                return Err(AikitError::new(
                    "now.fixtures_seam_row_incomplete",
                    "every fixture row names a file and an exact revision",
                ));
            }
        }
        Ok(seam)
    }

    /// The deterministic invocation ref (`now-contemplate/<digest>`) of the
    /// preflight computed over this seam.
    pub fn invocation_ref(&self) -> Result<ResourceRef> {
        let bytes = serde_json::to_vec(self)
            .map_err(|error| AikitError::new("now.preflight_unserializable", error.to_string()))?;
        let digest = blake3::hash(&bytes).to_hex().to_string();
        ResourceRef::parse(&format!("now-contemplate/{}", &digest[..24]))
    }
}

/// Deterministic facts of one contemplation basis: exactly what contemplate
/// would read. Preflight never touches an executor and records nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NowContemplationPreflight {
    pub version: String,
    pub invocation_ref: ResourceRef,
    pub now_ref: String,
    pub fixture_count: usize,
    /// Civil days represented in the stream, sorted, deduplicated.
    pub days: Vec<String>,
    /// Rows Central listed as nonconforming (pre-law fixtures) — they ride
    /// the stream and are named, never hidden.
    pub unstructured: Vec<String>,
    /// Fixture filenames in Central's listing order.
    pub source_fixtures: Vec<String>,
    /// Preflight remains deterministic; this field changes only in explicit execution.
    pub automatic_agent_or_model_invocation: bool,
}

/// The explicit, caller-supplied execution record: the validated preflight a
/// contemplate execution may run under. Anything else refuses before any
/// executor could be called.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NowContemplateRecord {
    pub version: String,
    pub invocation_ref: ResourceRef,
    pub now_ref: String,
    pub preflight: NowContemplationPreflight,
}

/// Validate one execution record against a freshly computed preflight. Drift
/// (a different stream, a foreign version, another NOW) refuses before any
/// executor could be called.
pub fn validate_now_contemplate_record(
    record: &NowContemplateRecord,
    fresh: &NowContemplationPreflight,
) -> Result<()> {
    if record.version != NOW_CONTEMPLATION_VERSION {
        return Err(AikitError::new(
            "now.contemplate_record_version_unsupported",
            format!(
                "Contemplate record version `{}` is not `{NOW_CONTEMPLATION_VERSION}`",
                record.version
            ),
        ));
    }
    if record.now_ref != fresh.now_ref || record.invocation_ref != fresh.invocation_ref {
        return Err(AikitError::new(
            "now.contemplate_record_drifted",
            format!(
                "the execution record names {} @ {} but the fresh preflight computes {} @ {}; the stream moved — preflight again",
                record.now_ref, record.invocation_ref, fresh.now_ref, fresh.invocation_ref
            ),
        ));
    }
    if record.preflight != *fresh {
        return Err(AikitError::new(
            "now.contemplate_record_drifted",
            "the execution record's preflight differs from the deterministic recomputation",
        ));
    }
    Ok(())
}

/// The distilled learning an executor proposed. Unapplied: the caller writes
/// it through Central's `central.now.learnings.distill`, which owns the
/// linkage law.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NowLearningProposal {
    pub content: String,
    /// The T fixtures the learning was parsed from — every fixture the
    /// stream supplied to the executor.
    pub source_fixtures: Vec<String>,
}

/// Typed contemplation reading for one explicit `Contemplate(now_ref)`.
/// `proposed` records the one deliberate Agent/model crossing; `unavailable`
/// and `refused` are explicit terminal states that carry a reason and never
/// fake a reading.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum NowContemplation {
    Proposed {
        version: String,
        invocation_ref: ResourceRef,
        now_ref: String,
        proposal: NowLearningProposal,
        /// `true` here and only here: the executor crossed the Agent/model seam.
        automatic_agent_or_model_invocation: bool,
    },
    Unavailable {
        version: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        invocation_ref: Option<ResourceRef>,
        now_ref: String,
        reason: String,
    },
    Refused {
        version: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        invocation_ref: Option<ResourceRef>,
        now_ref: String,
        reason: String,
    },
}

/// The executor aperture: parse signal out of the raw stream. The executor
/// never writes; it returns the distilled learning text.
pub trait NowContemplateExecutor {
    fn distill(
        &mut self,
        preflight: &NowContemplationPreflight,
        fixtures: &[NowFixture],
    ) -> Result<String>;
}

/// Compute the deterministic preflight over a supplied seam. The seam needs
/// no fixture bodies for the facts; execution requires them.
pub fn now_contemplate_preflight(seam: &NowFixturesSeam) -> Result<NowContemplationPreflight> {
    let mut days: Vec<String> = seam
        .fixtures
        .iter()
        .filter_map(|fixture| fixture.day.clone())
        .collect();
    days.sort();
    days.dedup();
    Ok(NowContemplationPreflight {
        version: NOW_CONTEMPLATION_VERSION.into(),
        invocation_ref: seam.invocation_ref()?,
        now_ref: seam.now_ref.clone(),
        fixture_count: seam.fixtures.len(),
        days,
        unstructured: seam
            .fixtures
            .iter()
            .filter(|fixture| !fixture.conforming)
            .map(|fixture| fixture.file.clone())
            .collect(),
        source_fixtures: seam
            .fixtures
            .iter()
            .map(|fixture| fixture.file.clone())
            .collect(),
        automatic_agent_or_model_invocation: false,
    })
}

/// Explicit execution under a validated record. Without a host executor the
/// reading is explicitly `unavailable`; the executor's distilled text leaves
/// as an unapplied proposal and nothing is written here.
pub fn explicit_now_contemplate(
    seam: &NowFixturesSeam,
    record: &NowContemplateRecord,
    executor: &mut dyn NowContemplateExecutor,
) -> Result<NowContemplation> {
    let fresh = now_contemplate_preflight(seam)?;
    validate_now_contemplate_record(record, &fresh)?;
    for fixture in &seam.fixtures {
        if fixture.content.is_none() {
            return Err(AikitError::new(
                "now.contemplate_fixture_body_absent",
                format!(
                    "fixture {} has no body in the seam; execution requires the stream's content (re-read with include_content)",
                    fixture.file
                ),
            ));
        }
    }
    let content = executor.distill(&fresh, &seam.fixtures)?;
    if content.trim().is_empty() {
        return Err(AikitError::new(
            "now.contemplate_empty_distillation",
            "the executor returned an empty distillation; prose that is not a learning cannot become T-prime",
        ));
    }
    Ok(NowContemplation::Proposed {
        version: NOW_CONTEMPLATION_VERSION.into(),
        invocation_ref: fresh.invocation_ref.clone(),
        now_ref: fresh.now_ref.clone(),
        proposal: NowLearningProposal {
            content,
            source_fixtures: fresh.source_fixtures.clone(),
        },
        automatic_agent_or_model_invocation: true,
    })
}

/// Explain disclosure for one NOW contemplation preflight: one
/// `ExplainEvidence` whose facts name exactly what the operation will read
/// and where the output would land, before anything runs.
pub fn explain_now_contemplate_preflight(
    preflight: &NowContemplationPreflight,
) -> Result<Vec<ExplainEvidence>> {
    let now_resource = ResourceRef::parse(&preflight.now_ref)?;
    let facts = vec![
        ExplainFact {
            relation: "now-stream-read".into(),
            authority: Some(SourceAuthority::Derived),
            summary: format!(
                "Contemplate reads NOW {} raw stream: {} fixture(s) across day(s) {}",
                preflight.now_ref,
                preflight.fixture_count,
                if preflight.days.is_empty() {
                    "none".to_owned()
                } else {
                    preflight.days.join(", ")
                }
            ),
            canonical_refs: vec![now_resource.clone()],
            provenance: vec![EvidenceProvenance {
                source: Some(now_resource.clone()),
                revision: None,
                ..EvidenceProvenance::default()
            }],
        },
        ExplainFact {
            relation: "prime-stream-write".into(),
            authority: Some(SourceAuthority::Authored),
            summary: "the distilled learning is proposed here, unapplied; the T-prime write belongs to Central's central.now.learnings.distill Action".into(),
            canonical_refs: vec![now_resource.clone()],
            provenance: Vec::new(),
        },
    ];
    Ok(vec![ExplainEvidence {
        schema: crate::explain_history::EXPLAIN_HISTORY_VERSION.into(),
        subject: now_resource,
        facts,
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seam() -> NowFixturesSeam {
        NowFixturesSeam::parse(
            r#"{
                "schema": "central.thoughts-reading/v1",
                "now_ref": "central:now:control:root:abc",
                "total": 2,
                "truncated": false,
                "fixtures": [
                    {"file": "raw-one-2026-09-13.md", "revision": "central.content-fnv1a64/v1:1:aa", "conforming": true, "day": "2026-09-13", "actor": "agent:test", "actor_kind": "agent", "content": "raw one"},
                    {"file": "legacy-2026-09-12.md", "revision": "central.content-fnv1a64/v1:2:bb", "conforming": false, "content": "pre-law body"}
                ]
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn seam_rejects_foreign_schemas_truncation_and_incomplete_rows() {
        assert!(
            NowFixturesSeam::parse("{\"schema\":\"aikit.flow-cognition/v1\",\"now_ref\":\"x\"}")
                .is_err()
        );
        let mut truncated = serde_json::to_string(&serde_json::json!({
            "schema": THOUGHTS_READING_SCHEMA, "now_ref": "central:now:control:root:abc",
            "total": 5, "truncated": true, "fixtures": []
        }))
        .unwrap();
        assert_eq!(
            NowFixturesSeam::parse(&truncated).unwrap_err().code(),
            "now.fixtures_seam_truncated"
        );
        truncated = serde_json::to_string(&serde_json::json!({
            "schema": THOUGHTS_READING_SCHEMA, "now_ref": "",
            "fixtures": []
        }))
        .unwrap();
        assert_eq!(
            NowFixturesSeam::parse(&truncated).unwrap_err().code(),
            "now.fixtures_seam_without_now"
        );
    }

    #[test]
    fn preflight_names_the_stream_and_digests_deterministically() {
        let stream = seam();
        let first = now_contemplate_preflight(&stream).unwrap();
        let second = now_contemplate_preflight(&stream).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.fixture_count, 2);
        // The pre-law fixture carries no day; only conforming rows date the stream.
        assert_eq!(first.days, vec!["2026-09-13".to_owned()]);
        assert_eq!(first.unstructured, vec!["legacy-2026-09-12.md".to_owned()]);
        assert_eq!(
            first.source_fixtures,
            vec![
                "raw-one-2026-09-13.md".to_owned(),
                "legacy-2026-09-12.md".to_owned()
            ]
        );
        assert!(
            first
                .invocation_ref
                .to_string()
                .starts_with("now-contemplate/")
        );
        assert_eq!(first.automatic_agent_or_model_invocation, false);
    }

    struct Echo;

    impl NowContemplateExecutor for Echo {
        fn distill(
            &mut self,
            _preflight: &NowContemplationPreflight,
            fixtures: &[NowFixture],
        ) -> Result<String> {
            Ok(format!("learning from {} fixtures", fixtures.len()))
        }
    }

    #[test]
    fn execution_is_record_gated_and_links_every_source_fixture() {
        let stream = seam();
        let preflight = now_contemplate_preflight(&stream).unwrap();
        let forged = NowContemplateRecord {
            version: "aikit.flow-contemplate/v1".into(),
            invocation_ref: preflight.invocation_ref.clone(),
            now_ref: preflight.now_ref.clone(),
            preflight: preflight.clone(),
        };
        assert_eq!(
            explicit_now_contemplate(&stream, &forged, &mut Echo)
                .unwrap_err()
                .code(),
            "now.contemplate_record_version_unsupported"
        );

        let mut drifted = NowContemplateRecord {
            version: NOW_CONTEMPLATION_VERSION.into(),
            invocation_ref: preflight.invocation_ref.clone(),
            now_ref: preflight.now_ref.clone(),
            preflight: preflight.clone(),
        };
        assert!(explicit_now_contemplate(&stream, &drifted, &mut Echo).is_ok());
        drifted.preflight.fixture_count = 99;
        assert_eq!(
            explicit_now_contemplate(&stream, &drifted, &mut Echo)
                .unwrap_err()
                .code(),
            "now.contemplate_record_drifted"
        );

        // A drifted stream under a stale record refuses too.
        let stale = NowContemplateRecord {
            version: NOW_CONTEMPLATION_VERSION.into(),
            invocation_ref: preflight.invocation_ref.clone(),
            now_ref: preflight.now_ref.clone(),
            preflight,
        };
        let mut moved = stream.clone();
        moved.fixtures.push(NowFixture {
            file: "late-2026-09-13.md".into(),
            revision: "central.content-fnv1a64/v1:3:cc".into(),
            conforming: true,
            day: Some("2026-09-13".into()),
            actor: None,
            actor_kind: None,
            content: Some("late body".into()),
        });
        assert_eq!(
            explicit_now_contemplate(&moved, &stale, &mut Echo)
                .unwrap_err()
                .code(),
            "now.contemplate_record_drifted"
        );

        let good = NowContemplateRecord {
            version: NOW_CONTEMPLATION_VERSION.into(),
            invocation_ref: now_contemplate_preflight(&stream).unwrap().invocation_ref,
            now_ref: stream.now_ref.clone(),
            preflight: now_contemplate_preflight(&stream).unwrap(),
        };
        match explicit_now_contemplate(&stream, &good, &mut Echo).unwrap() {
            NowContemplation::Proposed {
                proposal,
                automatic_agent_or_model_invocation,
                ..
            } => {
                assert_eq!(proposal.content, "learning from 2 fixtures");
                assert_eq!(
                    proposal.source_fixtures,
                    vec![
                        "raw-one-2026-09-13.md".to_owned(),
                        "legacy-2026-09-12.md".to_owned()
                    ]
                );
                assert!(automatic_agent_or_model_invocation);
            }
            other => panic!("expected a proposal, got {other:?}"),
        }
    }

    #[test]
    fn execution_without_fixture_bodies_is_refused_not_guessed() {
        let mut bodyless = seam();
        for fixture in &mut bodyless.fixtures {
            fixture.content = None;
        }
        let preflight = now_contemplate_preflight(&bodyless).unwrap();
        let record = NowContemplateRecord {
            version: NOW_CONTEMPLATION_VERSION.into(),
            invocation_ref: preflight.invocation_ref.clone(),
            now_ref: preflight.now_ref.clone(),
            preflight,
        };
        assert_eq!(
            explicit_now_contemplate(&bodyless, &record, &mut Echo)
                .unwrap_err()
                .code(),
            "now.contemplate_fixture_body_absent"
        );
    }
}
