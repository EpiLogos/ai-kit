//! The automatic skill auditor.
//!
//! A registry skill the profile selects but nobody has reviewed is withheld
//! (`TrustRequired`). Reviewing it by hand, again after every pull that edits it,
//! is a chore that teaches the owner to click through: the review is a mechanical
//! question — does this skill do anything a skill should not — and a scanner
//! answers it. So the selected, unreviewed revisions are handed to an auditor, and
//! what it finds decides:
//!
//! * `safe` or `caution` — the revision is recorded `Reviewed`, with the auditor's
//!   name and verdict as the note. Caution findings are informational: a skill
//!   that *talks about* editing `AGENTS.md` is not one that does.
//! * `dangerous` — the revision stays unreviewed and withheld, and the findings
//!   are returned so the owner reads what was found rather than a bare refusal.
//! * no auditor available — nothing is recorded. An absent scanner is never a
//!   pass.
//!
//! Only registry skills are audited. A skill the owner authored in their own
//! spool is trusted by promoting it (`source promote`), and a downloaded Git
//! source keeps its explicit `--trust`; this module touches neither.
//!
//! The auditor is a command, not a plugin: it receives one skill directory and
//! prints one JSON object (`{"scanner","verdict","findings":[…]}`). The default
//! is the Hermes skills guard when it is on this machine; `AIKIT_SKILL_AUDITOR`
//! (a JSON argv array) or `skill_auditor = [...]` in `config.toml` names another.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use aikit_core::capsule::Kind;
use aikit_core::catalog::Catalog;
use aikit_core::resolve::UnavailableReason;
use aikit_core::trust::{TrustKey, TrustState};
use aikit_core::{AikitError, CapsuleId, Result};
use aikit_store::home::AikitHome;
use aikit_store::index::Index;
use aikit_store::trust::TrustStore;

use crate::app::Service;

/// Read by the default auditor: the one Python module that holds the scanner.
const HERMES_GUARD: &str = ".hermes/hermes-agent/tools/skills_guard.py";

/// The shim the default auditor runs: import the guard, scan one directory as an
/// agent-authored skill, print the result as the protocol's JSON object.
const HERMES_SHIM: &str = r#"
import sys, json
from dataclasses import asdict
from pathlib import Path
sys.path.insert(0, sys.argv[1])
import skills_guard as guard
result = guard.scan_skill(Path(sys.argv[2]), "agent-created")
print(json.dumps({
    "scanner": guard.SCANNER_VERSION,
    "verdict": result.verdict,
    "findings": [asdict(finding) for finding in result.findings],
}))
"#;

/// One thing the auditor found. Carries no matched text: a finding is shown in
/// receipts and logs, and must not become a second copy of what it flagged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    #[serde(default)]
    pub pattern_id: String,
    #[serde(default)]
    pub severity: String,
    #[serde(default)]
    pub file: String,
    #[serde(default)]
    pub line: u64,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verdict {
    #[serde(default)]
    pub scanner: String,
    pub verdict: String,
    #[serde(default)]
    pub findings: Vec<Finding>,
}

impl Verdict {
    /// `safe` and `caution` may be recorded reviewed; `dangerous` and anything the
    /// protocol does not name may not.
    pub fn passes(&self) -> bool {
        matches!(self.verdict.as_str(), "safe" | "caution")
    }
}

/// What the auditor did with one selected, unreviewed skill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuditOutcome {
    pub capability: String,
    pub verdict: String,
    pub scanner: String,
    pub recorded: bool,
    pub findings: Vec<Finding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Auditor {
    argv: Vec<String>,
}

impl Auditor {
    pub fn new(argv: Vec<String>) -> Option<Self> {
        (!argv.is_empty()).then_some(Self { argv })
    }

    /// The auditor this machine names: the environment, then `config.toml`, then
    /// the Hermes guard where it is installed.
    pub fn configured(home: &AikitHome) -> Option<Self> {
        if let Ok(raw) = std::env::var("AIKIT_SKILL_AUDITOR") {
            return serde_json::from_str::<Vec<String>>(&raw)
                .ok()
                .and_then(Self::new);
        }
        if let Ok(text) = std::fs::read_to_string(home.root().join("config.toml")) {
            if let Ok(table) = text.parse::<toml::Table>() {
                if let Some(list) = table.get("skill_auditor").and_then(toml::Value::as_array) {
                    let argv = list
                        .iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect();
                    return Self::new(argv);
                }
            }
        }
        let guard = std::env::var_os("HOME")
            .map(PathBuf::from)?
            .join(HERMES_GUARD);
        let tools = guard.parent()?.to_path_buf();
        guard.is_file().then(|| Self {
            argv: vec![
                "python3".into(),
                "-c".into(),
                HERMES_SHIM.into(),
                tools.display().to_string(),
            ],
        })
    }

    pub fn audit(&self, skill_dir: &Path) -> Result<Verdict> {
        let (program, rest) = self.argv.split_first().expect("argv is never empty");
        let output = Command::new(program)
            .args(rest)
            .arg(skill_dir)
            .output()
            .map_err(|error| {
                AikitError::new(
                    "audit.unavailable",
                    format!("could not run the skill auditor `{program}`: {error}"),
                )
            })?;
        if !output.status.success() {
            return Err(AikitError::new(
                "audit.failed",
                format!(
                    "the skill auditor failed on {}: {}",
                    skill_dir.display(),
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            ));
        }
        serde_json::from_slice(&output.stdout).map_err(|error| {
            AikitError::new(
                "audit.unreadable",
                format!(
                    "the skill auditor printed something other than a verdict for {}: {error}",
                    skill_dir.display()
                ),
            )
        })
    }
}

/// Whether a catalogue source is a registry the owner pulls, rather than a skill
/// source promoted by hand (those carry their own trust at promotion).
fn is_registry_source(home: &AikitHome, source: &str) -> bool {
    !home.root().join("sources").join(source).is_dir()
}

/// Audit one capability's current revision and record it when it passes.
pub fn audit_one(
    home: &AikitHome,
    auditor: &Auditor,
    catalog: &dyn Catalog,
    id: &CapsuleId,
    record: bool,
) -> Result<AuditOutcome> {
    let capsule = catalog.get(id).ok_or_else(|| {
        AikitError::new(
            "audit.unknown_capsule",
            format!("{id} is not present in any registry"),
        )
    })?;
    let root = capsule.root.as_ref().ok_or_else(|| {
        AikitError::new(
            "audit.no_payload",
            format!("{id} has no payload on this machine to audit"),
        )
    })?;
    // A registry capsule is a manifest beside a payload; the payload is the skill.
    let payload = root.join("payload");
    let verdict = auditor.audit(if payload.is_dir() { &payload } else { root })?;
    let mut recorded = false;
    if record && verdict.passes() {
        if let (Some(source), Some(revision)) = (&capsule.source, &capsule.revision) {
            let index = Index::open(&home.database())?;
            TrustStore::new(&index).record(
                &TrustKey::new(source.clone(), capsule.id.clone(), revision.clone()),
                TrustState::Reviewed,
                Some(&format!(
                    "audited by {}: {} ({} findings)",
                    verdict.scanner,
                    verdict.verdict,
                    verdict.findings.len()
                )),
            )?;
            recorded = true;
        }
    }
    Ok(AuditOutcome {
        capability: id.to_string(),
        verdict: verdict.verdict,
        scanner: verdict.scanner,
        recorded,
        findings: verdict.findings,
    })
}

/// Audit every registry skill the service's profile selects and nobody has
/// reviewed. Returns what was decided, and why anything stayed withheld.
pub fn settle_unreviewed(service: &Service) -> (Vec<AuditOutcome>, Vec<String>) {
    let mut outcomes = Vec::new();
    let mut warnings = Vec::new();
    let view = service.resolved_view();
    let waiting: Vec<CapsuleId> = view
        .unavailable
        .iter()
        .filter(|(id, reason)| {
            id.kind() == Kind::Skill
                && matches!(reason, UnavailableReason::TrustRequired)
                && view.is_declared_enabled(id)
                && service
                    .snapshot()
                    .get(id)
                    .and_then(|capsule| capsule.source.as_ref())
                    .is_some_and(|source| is_registry_source(service.home(), source.as_str()))
        })
        .map(|(id, _)| id.clone())
        .collect();
    if waiting.is_empty() {
        return (outcomes, warnings);
    }
    let Some(auditor) = Auditor::configured(service.home()) else {
        warnings.push(format!(
            "skill-audit: {} selected skills are unreviewed and no skill auditor is configured, \
             so they reach no agent: {}",
            waiting.len(),
            names(&waiting)
        ));
        return (outcomes, warnings);
    };
    for id in &waiting {
        match audit_one(service.home(), &auditor, service.snapshot(), id, true) {
            Ok(outcome) => {
                if !outcome.recorded {
                    warnings.push(format!(
                        "skill-audit: {id} was audited {} by {} and stays withheld ({} findings) — \
                         read them with `aikit trust audit {id} --no-record`",
                        outcome.verdict,
                        outcome.scanner,
                        outcome.findings.len()
                    ));
                }
                outcomes.push(outcome);
            }
            Err(error) => warnings.push(format!(
                "skill-audit: {id} could not be audited and stays withheld: {}",
                error.message()
            )),
        }
    }
    (outcomes, warnings)
}

fn names(ids: &[CapsuleId]) -> String {
    ids.iter()
        .take(8)
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}
