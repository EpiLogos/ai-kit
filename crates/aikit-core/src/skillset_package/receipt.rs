//! `aikit.skillset-package-receipt/v1` and the provenance diff.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::model::PortableSkillPackage;
use super::target::{Finding, PackagePlan, PlanClass, RenderedFile, Severity, GENERATOR};

pub const RECEIPT_SCHEMA: &str = "aikit.skillset-package-receipt/v1";
pub const DIFF_SCHEMA: &str = "aikit.skillset-package-diff/v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiptMember {
    pub id: String,
    pub name: String,
    pub form: String,
    pub revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportedFile {
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Relation {
    pub relation: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnsupportedRelation {
    pub relation: String,
    pub reason: String,
}

/// Outcome of one validation step. `unavailable` is never success.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CheckStatus {
    Passed,
    Failed,
    Unavailable,
    NotRequested,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeValidation {
    pub status: CheckStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit: Option<i32>,
    pub summary: String,
}

impl NativeValidation {
    pub fn not_requested() -> Self {
        Self {
            status: CheckStatus::NotRequested,
            command: None,
            exit: None,
            summary: "native validation not requested (pass --native)".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Validation {
    pub structural: CheckStatus,
    pub findings: Vec<Finding>,
    pub native: NativeValidation,
}

impl Validation {
    pub fn from_findings(findings: Vec<Finding>) -> Self {
        let failed = findings.iter().any(|f| f.severity == Severity::Error);
        Self {
            structural: if failed {
                CheckStatus::Failed
            } else {
                CheckStatus::Passed
            },
            findings,
            native: NativeValidation::not_requested(),
        }
    }

    /// Structural passed and native passed or was not requested.
    pub fn ok(&self) -> bool {
        self.structural == CheckStatus::Passed
            && matches!(
                self.native.status,
                CheckStatus::Passed | CheckStatus::NotRequested
            )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Discovery {
    pub status: CheckStatus,
    pub method: String,
    /// Skill names the host reported discovering.
    #[serde(default)]
    pub discovered_skills: Vec<String>,
    /// Exported Skill names the host did NOT report.
    #[serde(default)]
    pub missing_skills: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub evidence: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    pub schema: String,
    pub generator: String,
    pub skillset_ref: String,
    pub source_revision: String,
    pub package: BTreeMap<String, String>,
    pub target: String,
    pub format_version: String,
    pub members: Vec<ReceiptMember>,
    pub exported_files: Vec<ExportedFile>,
    pub portable: Vec<Relation>,
    pub translated: Vec<Relation>,
    pub target_additions: Vec<Relation>,
    pub unsupported: Vec<UnsupportedRelation>,
    pub validation: Validation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discovery: Option<Discovery>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub out_dir: Option<String>,
    /// The canonical SkillSet was compared before and after export.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_unchanged: Option<bool>,
}

impl Receipt {
    pub fn new(
        pkg: &PortableSkillPackage,
        plan: &PackagePlan,
        files: &[RenderedFile],
        validation: Validation,
    ) -> Self {
        let mut exported: Vec<ExportedFile> = files
            .iter()
            .map(|f| ExportedFile {
                path: f.path.clone(),
                sha256: f.sha256.clone(),
            })
            .collect();
        exported.sort_by(|a, b| a.path.cmp(&b.path));
        let relation = |e: &super::target::PlanEntry| Relation {
            relation: e.relation.clone(),
            paths: e.paths.clone(),
            detail: e.detail.clone(),
        };
        let mut package = BTreeMap::new();
        package.insert("name".to_string(), pkg.identity.name.clone());
        package.insert("version".to_string(), pkg.version.clone());
        Self {
            schema: RECEIPT_SCHEMA.to_string(),
            generator: GENERATOR.to_string(),
            skillset_ref: pkg.skillset_ref.clone(),
            source_revision: pkg.source_revision.clone(),
            package,
            target: plan.target.as_str().to_string(),
            format_version: plan.format_version.clone(),
            members: pkg
                .members
                .iter()
                .map(|m| ReceiptMember {
                    id: m.id.clone(),
                    name: m.name.clone(),
                    form: m.form.as_str().to_string(),
                    revision: m.revision.clone(),
                })
                .collect(),
            exported_files: exported,
            portable: plan
                .entries
                .iter()
                .filter(|e| e.class == PlanClass::Portable)
                .map(relation)
                .collect(),
            translated: plan
                .entries
                .iter()
                .filter(|e| e.class == PlanClass::Translated)
                .map(relation)
                .collect(),
            target_additions: plan
                .entries
                .iter()
                .filter(|e| e.class == PlanClass::TargetAddition)
                .map(relation)
                .collect(),
            unsupported: plan
                .entries
                .iter()
                .filter_map(|e| match &e.class {
                    PlanClass::Unsupported { reason } => Some(UnsupportedRelation {
                        relation: e.relation.clone(),
                        reason: reason.clone(),
                    }),
                    _ => None,
                })
                .collect(),
            validation,
            discovery: None,
            out_dir: None,
            source_unchanged: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangedMember {
    pub id: String,
    pub exported: String,
    pub current: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageDiff {
    pub schema: String,
    pub skillset_ref: String,
    pub target: String,
    pub exported_source_revision: String,
    pub current_source_revision: String,
    /// True when the exported tree reflects the current source exactly.
    pub current: bool,
    pub changed: Vec<ChangedMember>,
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

/// Compare a previously exported tree's provenance against the current source.
pub fn diff_provenance(provenance: &Value, pkg: &PortableSkillPackage) -> PackageDiff {
    let exported: BTreeMap<String, String> = provenance["members"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            Some((
                m["id"].as_str()?.to_string(),
                m["revision"].as_str()?.to_string(),
            ))
        })
        .collect();
    let current: BTreeMap<String, String> = pkg
        .members
        .iter()
        .map(|m| (m.id.clone(), m.revision.clone()))
        .collect();
    let changed = current
        .iter()
        .filter_map(|(id, rev)| {
            exported
                .get(id)
                .filter(|old| *old != rev)
                .map(|old| ChangedMember {
                    id: id.clone(),
                    exported: old.clone(),
                    current: rev.clone(),
                })
        })
        .collect::<Vec<_>>();
    let added = current
        .keys()
        .filter(|id| !exported.contains_key(*id))
        .cloned()
        .collect::<Vec<_>>();
    let removed = exported
        .keys()
        .filter(|id| !current.contains_key(*id))
        .cloned()
        .collect::<Vec<_>>();
    let exported_rev = provenance["source_revision"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    PackageDiff {
        schema: DIFF_SCHEMA.to_string(),
        skillset_ref: pkg.skillset_ref.clone(),
        target: provenance["target"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        current: exported_rev == pkg.source_revision
            && changed.is_empty()
            && added.is_empty()
            && removed.is_empty(),
        exported_source_revision: exported_rev,
        current_source_revision: pkg.source_revision.clone(),
        changed,
        added,
        removed,
    }
}
