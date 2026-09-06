//! The Control ground skill manifest contract (`central.skill/v1`).
//!
//! Central K1 publishes this contract for skills authored on Control ground; a
//! skill directory may carry a `skill.json` beside its `SKILL.md` declaring
//! standing, scope, provenance and — when the standing is retired — the
//! retirement record. AIKit treats it exactly like the harness adapters treat
//! Central projections: a **published contract of a sibling product**, read at
//! its documented surface and never inferred beyond it (no path heuristics, no
//! second opinion about what Central "must have meant").
//!
//! The reading rules, mirrored from the contract itself:
//!
//! * absent `skill.json` ⇒ unresolved Control standing; the source adapter
//!   preserves that fact and withholds it (ordinary sources remain compatible);
//! * any other `schema` ⇒ refused, naming the file — a neighbour's future
//!   contract version is a loud event, not a guess;
//! * `standing` is `active` or `retired`; anything else is unresolved and
//!   refused rather than silently projected;
//! * a retired standing must carry its retirement record with a reason, so the
//!   withholding can always be disclosed with the owner's own words;
//! * the manifest `name` must agree with the directory, as it does on the ground.
//!
//! Only a source registered `--control-ground` reads this contract at all: a
//! directory that merely happens to contain a `skill.json` from some unrelated
//! product is left alone.

use std::fs;
use std::path::Path;

use aikit_core::{AikitError, Result};
use serde::Deserialize;

/// The file name the contract publishes.
pub const SKILL_MANIFEST: &str = "skill.json";
/// The only schema this build understands.
pub const SKILL_MANIFEST_SCHEMA: &str = "central.skill/v1";

/// The published contract surface AIKit reads. Unknown fields are ignored, not
/// preserved: the ground file is the neighbour's document, and the snapshot
/// keeps its own derived metadata rather than re-serializing Central's.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SkillContract {
    pub schema: String,
    pub name: String,
    pub standing: String,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub provenance: Option<String>,
    #[serde(default)]
    pub retirement: Option<Retirement>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Retirement {
    pub retired_by: String,
    pub retired_at_unix_seconds: u64,
    pub retirement_reason: String,
}

/// The standing + provenance that become the capsule's `[metadata.control]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlMetadata {
    pub standing: String,
    pub scope: Option<String>,
    pub provenance: Option<String>,
    pub retired_by: Option<String>,
    pub retired_at_unix_seconds: Option<u64>,
    pub retirement_reason: Option<String>,
}

impl ControlMetadata {
    pub fn unresolved() -> Self {
        Self {
            standing: "unresolved".into(),
            scope: None,
            provenance: None,
            retired_by: None,
            retired_at_unix_seconds: None,
            retirement_reason: None,
        }
    }

    pub fn is_retired(&self) -> bool {
        self.standing == "retired"
    }
}

/// Read the contract beside a skill, or say precisely what is wrong with it.
///
/// `directory_name` is the skill directory on the ground; the contract requires
/// the manifest to name the same skill, and AIKit enforces the same agreement it
/// enforces between its own manifests and their directories.
pub fn read(root: &Path, directory_name: &str) -> Result<Option<ControlMetadata>> {
    let manifest_path = root.join(SKILL_MANIFEST);
    if !manifest_path.is_file() {
        return Ok(None);
    }

    let refusal = |detail: String| {
        AikitError::new("source.control_contract", detail)
            .with("manifest", manifest_path.display().to_string())
    };

    let text = fs::read_to_string(&manifest_path).map_err(|error| {
        refusal(format!(
            "could not read {}: {error}",
            manifest_path.display()
        ))
    })?;
    let contract: SkillContract = serde_json::from_str(&text).map_err(|error| {
        refusal(format!(
            "{} is not a valid {} manifest: {error}",
            manifest_path.display(),
            SKILL_MANIFEST_SCHEMA
        ))
    })?;

    if contract.schema != SKILL_MANIFEST_SCHEMA {
        return Err(refusal(format!(
            "{} declares schema `{}`, but this build understands only `{}`; a neighbour \
             contract version change must be read, not guessed",
            manifest_path.display(),
            contract.schema,
            SKILL_MANIFEST_SCHEMA
        )));
    }
    if contract.name != directory_name {
        return Err(refusal(format!(
            "manifest name `{}` does not match the skill directory `{directory_name}`; the \
             contract requires them to agree",
            contract.name
        )));
    }

    if !matches!(
        contract.scope.as_deref(),
        Some("control-user" | "control-machine" | "projectcentral-user")
    ) {
        return Err(refusal(
            "skill.json must declare a published Central skill scope".into(),
        ));
    }
    if !matches!(
        contract.provenance.as_deref(),
        Some("human-authored" | "adopted")
    ) {
        return Err(refusal(
            "skill.json must declare human-authored or adopted provenance".into(),
        ));
    }

    match contract.standing.as_str() {
        "active" => Ok(Some(ControlMetadata {
            standing: "active".to_string(),
            scope: contract.scope,
            provenance: contract.provenance,
            retired_by: None,
            retired_at_unix_seconds: None,
            retirement_reason: None,
        })),
        "retired" => {
            // The contract makes the retirement record mandatory for a retired
            // standing; without it the withholding would be undisclosable.
            let retirement = contract.retirement.as_ref().ok_or_else(|| {
                refusal(format!(
                    "{} declares standing retired without a retirement record; retirement \
                     records who, when and why so the withholding can say so",
                    manifest_path.display()
                ))
            })?;
            if retirement.retirement_reason.trim().is_empty() {
                return Err(refusal(format!(
                    "{} retires the skill with an empty reason; the reason is what projection \
                     discloses when it withholds",
                    manifest_path.display()
                )));
            }
            Ok(Some(ControlMetadata {
                standing: "retired".to_string(),
                scope: contract.scope,
                provenance: contract.provenance,
                retired_by: Some(retirement.retired_by.clone()),
                retired_at_unix_seconds: Some(retirement.retired_at_unix_seconds),
                retirement_reason: Some(retirement.retirement_reason.clone()),
            }))
        }
        other => Err(refusal(format!(
            "{} declares standing `{other}`, which is neither active nor retired; an \
             unresolved standing must be refused, never projected",
            manifest_path.display()
        ))),
    }
}
