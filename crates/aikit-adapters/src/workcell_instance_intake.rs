//! AIKit intake of Workcell's `workcell.harness-instance/v1` records.
//!
//! Workcell owns the live-instance registry (host+services layer, M1); AIKit
//! consumes it alongside Actuation's `actuation.harness-detection/v1`
//! (`actuation_harness_detection.rs`) for the M4 projection surface. The
//! instance record is the contract that projects: identity held by contract
//! (`instance:<slug>:<sha256>` over executable receipt + first-seen identity
//! material), never by host.
//!
//! Intake law (unchanged from Actuation detection): a failed run is a
//! disclosed `Unavailable { reason }` — never an empty set read as absence.
//! A completed listing that holds zero instances is honest ground: the
//! registry exists and has observed nothing, which is a different fact from
//! "the registry could not be read".

use serde::{Deserialize, Serialize};

use crate::runner::CommandRunner;

pub const WORKCELL_HARNESS_INSTANCE_SCHEMA: &str = "workcell.harness-instance/v1";

/// Evidence grades, strongest first. `LivePid` and `GatewayConfirmed` are
/// detected instances; `DeclaredUnverified` is conformance intent and never
/// counts toward projection candidacy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstanceEvidenceGrade {
    LivePid,
    GatewayConfirmed,
    DeclaredUnverified,
}

impl InstanceEvidenceGrade {
    /// Detected = evidence observed, not merely declared.
    pub fn is_detected(self) -> bool {
        !matches!(self, Self::DeclaredUnverified)
    }
}

/// Liveness is disclosed, never silently deleted: `Stale` follows N missed
/// scans and the record stays in the registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstanceLiveness {
    Live,
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceExecutable {
    pub path: String,
    pub sha256: String,
}

/// One Actuation facet seam on the instance (skills|plugins|hooks|…), the
/// same `{kind, path, exists, count?}` shape the detection intake carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceSeam {
    pub kind: String,
    pub path: String,
    pub exists: bool,
    #[serde(default)]
    pub count: Option<usize>,
}

/// One `workcell.harness-instance/v1` record. Field-for-field the schema
/// Workcell validates; intake never invents or relaxes a field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkcellInstanceRecord {
    pub schema: String,
    pub instance_ref: String,
    pub harness_ref: String,
    pub workcell_ref: String,
    #[serde(default)]
    pub pids: Vec<u32>,
    pub executable: InstanceExecutable,
    #[serde(default)]
    pub seams: Vec<InstanceSeam>,
    pub evidence_grade: InstanceEvidenceGrade,
    pub liveness: InstanceLiveness,
    #[serde(default)]
    pub consecutive_misses: u64,
    pub observed_at: String,
}

impl WorkcellInstanceRecord {
    /// §4 projection candidacy for one record: detected evidence. The
    /// workcell-level predicate is "≥1 detected instance"; liveness is
    /// disclosed in the record but does not gate candidacy.
    pub fn is_projection_candidate(&self) -> bool {
        self.evidence_grade.is_detected()
    }
}

/// The `workcell instances list --json` envelope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkcellInstanceListing {
    pub ok: bool,
    pub workcell_ref: String,
    #[serde(default)]
    pub instances: Vec<WorkcellInstanceRecord>,
}

/// What instance intake yielded: proven records, or a disclosed
/// unavailability. A completed empty listing is `Records(vec![])` — honest
/// ground, distinct from `Unavailable`.
#[derive(Debug, Clone, PartialEq)]
pub enum InstancesOutcome {
    Records(Vec<WorkcellInstanceRecord>),
    Unavailable { reason: String },
}

impl InstancesOutcome {
    /// §4 candidate view: the detected instances a consuming surface may
    /// project. Unavailable yields nothing — absence is never fabricated.
    pub fn projection_candidates(&self) -> Vec<&WorkcellInstanceRecord> {
        match self {
            InstancesOutcome::Records(records) => records
                .iter()
                .filter(|record| record.is_projection_candidate())
                .collect(),
            InstancesOutcome::Unavailable { .. } => Vec::new(),
        }
    }
}

/// Run `workcell instances list --json` through the given runner and parse
/// the listing. `state_root` overrides the workcell state root (a projection
/// target reads its own registry). A failed or unparsable run — or a record
/// whose schema is not `workcell.harness-instance/v1` — is
/// `Unavailable { reason }`; nothing is silently dropped.
pub fn intake_workcell_instances(
    runner: &dyn CommandRunner,
    workcell_bin: &str,
    state_root: Option<&str>,
) -> InstancesOutcome {
    let mut argv = vec![
        workcell_bin.to_string(),
        "--json".to_string(),
        "instances".to_string(),
        "list".to_string(),
    ];
    if let Some(root) = state_root {
        argv.push("--state-root".to_string());
        argv.push(root.to_string());
    }
    let output = match runner.run(&argv) {
        Ok(output) => output,
        Err(error) => {
            return InstancesOutcome::Unavailable {
                reason: format!("could not run {workcell_bin}: {error}"),
            };
        }
    };
    if output.status != 0 {
        return InstancesOutcome::Unavailable {
            reason: format!(
                "{workcell_bin} instances list failed ({}): {}",
                output.status,
                output.stderr.trim().chars().take(200).collect::<String>()
            ),
        };
    }
    let listing = match serde_json::from_str::<WorkcellInstanceListing>(&output.stdout) {
        Ok(listing) => listing,
        Err(error) => {
            return InstancesOutcome::Unavailable {
                reason: format!("instance listing unparsable: {error}"),
            };
        }
    };
    if !listing.ok {
        return InstancesOutcome::Unavailable {
            reason: "instance listing reported ok: false".to_string(),
        };
    }
    for record in &listing.instances {
        if record.schema != WORKCELL_HARNESS_INSTANCE_SCHEMA {
            return InstancesOutcome::Unavailable {
                reason: format!(
                    "unexpected instance schema {:?} for {} (expected {WORKCELL_HARNESS_INSTANCE_SCHEMA})",
                    record.schema, record.instance_ref
                ),
            };
        }
    }
    InstancesOutcome::Records(listing.instances)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::Output;
    use aikit_core::AikitError;

    struct FailingRunner;
    impl CommandRunner for FailingRunner {
        fn run(&self, _argv: &[String]) -> aikit_core::Result<Output> {
            Err(AikitError::new("runner.failed", "spawn lost"))
        }
    }

    struct EchoRunner(String);
    impl CommandRunner for EchoRunner {
        fn run(&self, _argv: &[String]) -> aikit_core::Result<Output> {
            Ok(Output::success(self.0.clone()))
        }
    }

    struct RecordingRunner {
        seen: std::sync::Mutex<Vec<Vec<String>>>,
        output: Output,
    }
    impl CommandRunner for RecordingRunner {
        fn run(&self, argv: &[String]) -> aikit_core::Result<Output> {
            self.seen.lock().unwrap().push(argv.to_vec());
            Ok(self.output.clone())
        }
    }

    fn live_record(slug: &str, grade: &str) -> String {
        format!(
            r#"{{"schema": "workcell.harness-instance/v1",
                "instance_ref": "instance:{slug}:abcd1234",
                "harness_ref": "harness/{slug}",
                "workcell_ref": "workcell:local",
                "pids": [42],
                "executable": {{"path": "/usr/local/bin/{slug}", "sha256": "{sha}"}},
                "seams": [{{"kind": "skills", "path": "~/.{slug}/skills", "exists": true, "count": 3}}],
                "evidence_grade": "{grade}",
                "liveness": "live",
                "consecutive_misses": 0,
                "observed_at": "unix:1788868104"}}"#,
            sha = "ab".repeat(32),
        )
    }

    fn listing(records: &[String]) -> String {
        format!(
            r#"{{"ok": true, "workcell_ref": "workcell:local", "instances": [{}]}}"#,
            records.join(",")
        )
    }

    #[test]
    fn parses_listing_and_maps_projection_candidates() {
        let payload = listing(&[
            live_record("hermes", "live-pid"),
            live_record("claude-code", "gateway-confirmed"),
            live_record("pi", "declared-unverified"),
        ]);
        let outcome = intake_workcell_instances(&EchoRunner(payload), "workcell", None);
        let records = match &outcome {
            InstancesOutcome::Records(records) => records,
            InstancesOutcome::Unavailable { reason } => panic!("expected records: {reason}"),
        };
        assert_eq!(records.len(), 3);
        let candidates = outcome.projection_candidates();
        assert_eq!(candidates.len(), 2, "declared-unverified is not detection");
        assert!(candidates
            .iter()
            .all(|record| record.evidence_grade.is_detected()));
        assert_eq!(candidates[0].instance_ref, "instance:hermes:abcd1234");
        assert_eq!(candidates[0].seams[0].kind, "skills");
    }

    #[test]
    fn completed_empty_listing_is_honest_ground_not_absence() {
        let outcome = intake_workcell_instances(&EchoRunner(listing(&[])), "workcell", None);
        match outcome {
            InstancesOutcome::Records(records) => assert!(records.is_empty()),
            InstancesOutcome::Unavailable { reason } => {
                panic!("an empty registry is ground, not unavailability: {reason}")
            }
        }
    }

    #[test]
    fn failed_spawn_is_unavailable_never_absence() {
        let outcome = intake_workcell_instances(&FailingRunner, "workcell", None);
        match outcome {
            InstancesOutcome::Unavailable { reason } => {
                assert!(reason.contains("could not run workcell"));
            }
            InstancesOutcome::Records(_) => panic!("failure must not yield records"),
        }
    }

    #[test]
    fn non_zero_exit_is_unavailable_with_stderr_reason() {
        let output = Output {
            status: 2,
            stdout: String::new(),
            stderr: "unknown instance `instance:ghost`".to_string(),
        };
        let outcome = intake_workcell_instances(&EchoRunner(String::new()), "workcell", None);
        let _ = outcome;
        let runner = RecordingRunner {
            seen: std::sync::Mutex::new(Vec::new()),
            output,
        };
        let outcome = intake_workcell_instances(&runner, "workcell", None);
        match outcome {
            InstancesOutcome::Unavailable { reason } => {
                assert!(reason.contains("instances list failed (2)"));
                assert!(reason.contains("unknown instance"));
            }
            InstancesOutcome::Records(_) => panic!("non-zero exit must not yield records"),
        }
    }

    #[test]
    fn garbage_output_is_unavailable_never_records() {
        let outcome = intake_workcell_instances(
            &EchoRunner(r#"{"not":"a listing"}"#.to_string()),
            "workcell",
            None,
        );
        match outcome {
            InstancesOutcome::Unavailable { reason } => {
                assert!(reason.contains("unparsable") || reason.contains("ok"));
            }
            InstancesOutcome::Records(_) => panic!("garbage must not yield records"),
        }
    }

    #[test]
    fn wrong_record_schema_is_unavailable_and_named() {
        let bad = r#"{"schema": "workcell.something-else/v1", "instance_ref": "instance:x:y",
                      "harness_ref": "harness/x", "workcell_ref": "workcell:local",
                      "pids": [], "executable": {"path": "/x", "sha256": "s"},
                      "evidence_grade": "live-pid", "liveness": "live", "observed_at": "unix:1"}"#;
        let outcome =
            intake_workcell_instances(&EchoRunner(listing(&[bad.to_string()])), "workcell", None);
        match outcome {
            InstancesOutcome::Unavailable { reason } => {
                assert!(reason.contains("unexpected instance schema"));
                assert!(reason.contains("instance:x:y"));
            }
            InstancesOutcome::Records(_) => panic!("schema drift must not be silently kept"),
        }
    }

    #[test]
    fn state_root_rides_the_argv_for_projection_targets() {
        let runner = RecordingRunner {
            seen: std::sync::Mutex::new(Vec::new()),
            output: Output::success(listing(&[])),
        };
        let outcome = intake_workcell_instances(&runner, "workcell", Some("/mnt/target-state"));
        assert!(matches!(outcome, InstancesOutcome::Records(_)));
        let seen = runner.seen.lock().unwrap();
        let argv = &seen[0];
        assert_eq!(
            argv,
            &vec![
                "workcell".to_string(),
                "--json".to_string(),
                "instances".to_string(),
                "list".to_string(),
                "--state-root".to_string(),
                "/mnt/target-state".to_string(),
            ]
        );
    }
}
