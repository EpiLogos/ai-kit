//! AIKit intake of Actuation's `actuation.harness-detection/v1` records.
//!
//! Actuation owns harness detection; AIKit consumes it. Intake never
//! invents candidates: a failed run is a disclosed unavailability, never an
//! empty set read as absence - the same three-state law the Actuation
//! contract itself enforces (detected / unavailable-with-reason /
//! not-installed-with-evidence).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::runner::CommandRunner;
use aikit_core::{AikitError, Result};

pub const ACTUATION_HARNESS_DETECTION_SCHEMA: &str = "actuation.harness-detection/v1";

/// The three detection states, carried across the seam unchanged. The
/// wire format is a plain string on each entry; the reason for an
/// unavailable entry travels in `unavailable_reason`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DetectionState {
    Detected,
    Unavailable,
    NotInstalled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectionReceipts {
    pub executable: String,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub mtime: Option<u128>,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub executable_is: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectionFacet {
    pub kind: String,
    pub path: String,
    pub exists: bool,
    #[serde(default)]
    pub count: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectionProbe {
    pub kind: String,
    pub result: String,
    #[serde(default)]
    pub spec: Option<String>,
    #[serde(default)]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectionEntry {
    pub slug: String,
    pub harness_ref: String,
    pub state: DetectionState,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub receipts: Option<DetectionReceipts>,
    #[serde(default)]
    pub facets: Option<Vec<DetectionFacet>>,
    #[serde(default)]
    pub probes: Option<Vec<DetectionProbe>>,
    #[serde(default)]
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectionDetector {
    pub implementation: String,
    version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActuationDetectionRecord {
    pub schema: String,
    pub detection_ref: String,
    pub observed_at: String,
    pub catalog_revision: u32,
    pub detector: DetectionDetector,
    pub harnesses: Vec<DetectionEntry>,
    pub absent: Vec<String>,
    pub availability: String,
}

/// What intake yielded: a proven record, or a disclosed unavailability.
#[derive(Debug, Clone, PartialEq)]
pub enum DetectionOutcome {
    Record(Box<ActuationDetectionRecord>),
    Unavailable { reason: String },
}

impl DetectionOutcome {
    /// Detected entries as (slug, harness_ref) pairs.
    pub fn detected_pairs(&self) -> Vec<(String, String)> {
        match self {
            DetectionOutcome::Record(record) => record
                .harnesses
                .iter()
                .filter(|entry| matches!(entry.state, DetectionState::Detected))
                .map(|entry| (entry.slug.clone(), entry.harness_ref.clone()))
                .collect(),
            DetectionOutcome::Unavailable { .. } => Vec::new(),
        }
    }

    pub fn detection_ref(&self) -> Option<&str> {
        match self {
            DetectionOutcome::Record(record) => Some(&record.detection_ref),
            DetectionOutcome::Unavailable { .. } => None,
        }
    }
}

/// Run `actuation harness detect --json` through the given runner and parse
/// the record. A failed or unparsable run is `Unavailable { reason }` - the
/// honest third state, never a fabricated empty set.
pub fn intake_actuation_detection(
    runner: &dyn CommandRunner,
    actuation_bin: &str,
) -> DetectionOutcome {
    let argv = vec![
        actuation_bin.to_string(),
        "harness".to_string(),
        "detect".to_string(),
        "--json".to_string(),
    ];
    let output = match runner.run(&argv) {
        Ok(output) => output,
        Err(error) => {
            return DetectionOutcome::Unavailable {
                reason: format!("could not run {actuation_bin}: {error}"),
            };
        }
    };
    if output.status != 0 {
        return DetectionOutcome::Unavailable {
            reason: format!(
                "{actuation_bin} harness detect failed ({}): {}",
                output.status,
                output.stderr.trim().chars().take(200).collect::<String>()
            ),
        };
    }
    match serde_json::from_str::<ActuationDetectionRecord>(&output.stdout) {
        Ok(record) if record.schema == ACTUATION_HARNESS_DETECTION_SCHEMA => {
            DetectionOutcome::Record(Box::new(record))
        }
        Ok(record) => DetectionOutcome::Unavailable {
            reason: format!(
                "unexpected detection schema {:?} (expected {ACTUATION_HARNESS_DETECTION_SCHEMA})",
                record.schema
            ),
        },
        Err(error) => DetectionOutcome::Unavailable {
            reason: format!("detection output unparsable: {error}"),
        },
    }
}
mod tests {
    use super::*;
    use crate::runner::Output;

    struct EchoRunner;
    impl CommandRunner for EchoRunner {
        fn run(&self, _argv: &[String]) -> Result<Output> {
            Ok(Output::success(r#"{"not":"a detection record"}"#))
        }
    }

    struct FailingRunner;
    impl CommandRunner for FailingRunner {
        fn run(&self, _argv: &[String]) -> Result<Output> {
            Err(AikitError::new("runner.failed", "spawn lost"))
        }
    }

    fn sample_record() -> String {
        let text = r#"{
          "schema": "actuation.harness-detection/v1",
          "document": "detection",
          "detection_ref": "detection:2026-09-05T00:00:00Z",
          "observed_at": "2026-09-05T00:00:00Z",
          "catalog_revision": 1,
          "detector": {"implementation": "actuation harness detect", "version": "0.1.0"},
          "harnesses": [
            {"slug": "claude-code", "harness_ref": "harness/claude-code", "state": "detected",
             "receipts": {"executable": "/usr/local/bin/claude"},
             "probes": [{"kind": "executable", "result": "pass", "detail": "/usr/local/bin/claude"}]},
            {"slug": "aider", "harness_ref": "harness/aider", "state": "not-installed",
             "probes": [{"kind": "config-dir", "result": "pass", "detail": "not found"}]},
            {"slug": "flaky", "harness_ref": "harness/flaky", "state": "unavailable",
             "unavailable_reason": "all probes failed; could not run"}
          ],
          "absent": ["aider"],
          "availability": "complete"
        }"#;
        text.to_string()
    }

    struct RecordRunner;
    impl CommandRunner for RecordRunner {
        fn run(&self, _argv: &[String]) -> Result<Output> {
            Ok(Output::success(sample_record()))
        }
    }

    #[test]
    fn parses_record_and_maps_detected_candidates() {
        let outcome = intake_actuation_detection(&RecordRunner, "actuation");
        let record = match &outcome {
            DetectionOutcome::Record(record) => record.as_ref(),
            DetectionOutcome::Unavailable { reason } => panic!("expected record, got: {reason}"),
        };
        assert_eq!(record.harnesses.len(), 3);
        assert_eq!(record.absent, vec!["aider".to_string()]);
        let pairs = outcome.detected_pairs();
        assert_eq!(pairs, vec![("claude-code".to_string(), "harness/claude-code".to_string())]);
        assert_eq!(outcome.detection_ref(), Some("detection:2026-09-05T00:00:00Z"));
    }

    #[test]
    fn failed_spawn_is_unavailable_never_absence() {
        let outcome = intake_actuation_detection(&FailingRunner, "actuation");
        match outcome {
            DetectionOutcome::Unavailable { reason } => {
                assert!(reason.contains("could not run actuation"));
            }
            DetectionOutcome::Record(_) => panic!("failure must not yield a record"),
        }
    }

    #[test]
    fn garbage_output_is_unavailable_never_a_record() {
        let outcome = intake_actuation_detection(&EchoRunner, "actuation");
        match outcome {
            DetectionOutcome::Unavailable { reason } => {
                assert!(reason.contains("unparsable") || reason.contains("schema"));
            }
            DetectionOutcome::Record(_) => panic!("garbage must not yield a record"),
        }
    }
}

/// Distil a detection outcome into the core-owned ground that rides on a
/// `ContextResolution`. A failed run becomes the disclosed `Unavailable`
/// ground — never `None`, which means "detection did not run", a different
/// fact under the three-state law.
pub fn detection_summary(
    outcome: &DetectionOutcome,
) -> aikit_core::context_resolution::HarnessDetectionGround {
    use aikit_core::context_resolution::HarnessDetectionGround;
    let record = match outcome {
        DetectionOutcome::Record(record) => record.as_ref(),
        DetectionOutcome::Unavailable { reason } => {
            return HarnessDetectionGround::Unavailable {
                reason: reason.clone(),
            };
        }
    };
    let mut states = BTreeMap::new();
    let mut reasons = BTreeMap::new();
    for entry in &record.harnesses {
        let state = match entry.state {
            DetectionState::Detected => "detected",
            DetectionState::Unavailable => "unavailable",
            DetectionState::NotInstalled => "not-installed",
        };
        states.insert(entry.slug.clone(), state.to_string());
        if let Some(reason) = &entry.unavailable_reason {
            reasons.insert(entry.slug.clone(), reason.clone());
        }
    }
    HarnessDetectionGround::Observed {
        detection_ref: record.detection_ref.clone(),
        catalog_revision: record.catalog_revision,
        states,
        reasons,
    }
}

/// One ephemeral candidate resource for a detected harness. Never persisted
/// to any index: detection is a live observation, not authored ground. The
/// detection_ref rides in the descriptor annotations so the freshness chain
/// stays inspectable downstream.
pub fn detected_harness_resource(
    slug: &str,
    harness_ref: &str,
    detection_ref: &str,
) -> Result<aikit_core::context_resolution::ResolvedResource> {
    use aikit_core::context_resolution::{Availability, ResolvedResource};
    use aikit_core::resource::{ResourceDescriptor, ResourceKind, SourceState};
    use aikit_core::resource::{ResourceRef, SourceRef};

    let mut descriptor = ResourceDescriptor::new(
        aikit_core::resource::ResourceRef::parse(harness_ref)?,
        aikit_core::resource::ResourceKind::Harness,
        format!("detected harness {slug}"),
        format!("detected live by Actuation in {detection_ref}"),
    );
    descriptor.sources.push(aikit_core::resource::ResourceSource {
        source: aikit_core::resource::SourceRef::parse("source/actuation-detection")?,
        authority: None,
        revision: None,
        locator: None,
        state: SourceState::Available,
    });
    descriptor
        .annotations
        .insert("detection_ref".to_string(), detection_ref.to_string());
    Ok(ResolvedResource {
        resource: aikit_core::resource::ResourceRecord::new(descriptor),
        availability: Availability::Available,
    })
}
