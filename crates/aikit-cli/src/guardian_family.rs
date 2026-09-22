//! `aikit family` — resolve the shipped six-product Guardian family
//! declaration (`aikit.guardian-family/v1`) against the sources actually
//! registered on this machine.
//!
//! The family registry declares each product Guardian's repertoire by
//! reference to its native owner; this read model is the missing admission
//! step: it observes the registered sources, their Git remotes and their
//! active-snapshot capsule catalogues, and reports per member whether the
//! reference resolves, and if not, precisely which delivery gap stands. It
//! never grants, registers, trusts or selects anything — degraded state stays
//! visible, and the one command that would change it is named in the output.

use std::collections::BTreeMap;
use std::process::Command;

use aikit_core::guardian_family::{
    embedded_family, member_skill_name, normalise_remote, resolve_member, MemberResolution,
    SourceFacts,
};
use aikit_core::{AikitError, Result};
use aikit_store::home::AikitHome;
use serde_json::{json, Value};

/// Read one loose TOML table so schema drift in unrelated fields cannot
/// break the reading.
fn loose_toml(path: &std::path::Path) -> Option<toml::Table> {
    let text = std::fs::read_to_string(path).ok()?;
    toml::from_str(&text).ok()
}

/// The Git remote of the repository that contains `skills_dir` (a `<repo>/skills`
/// directory), normalised to `owner/repo`. Absent or unborn remotes are honest
/// `None`s, not guesses.
fn observed_owner_repository(skills_dir: &std::path::Path) -> Option<String> {
    let repo_root = skills_dir.parent()?;
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["remote", "get-url", "origin"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let remote = String::from_utf8(output.stdout).ok()?;
    normalise_remote(remote.trim())
}

/// Skill directory names catalogued in the source's active snapshot, with a
/// warning when the active state could not be read.
fn active_skill_names(
    home: &AikitHome,
    source_id: &str,
    warnings: &mut Vec<String>,
) -> Vec<String> {
    let source_dir = home.root().join("sources").join(source_id);
    let state = match loose_toml(&source_dir.join("state.toml")) {
        Some(state) => state,
        None => {
            warnings.push(format!("source `{source_id}` has no readable active state"));
            return Vec::new();
        }
    };
    let active = match state
        .get("active_snapshot")
        .and_then(|value| value.as_str())
    {
        Some(digest) => digest.to_owned(),
        None => {
            warnings.push(format!("source `{source_id}` has no active snapshot"));
            return Vec::new();
        }
    };
    let capsules = source_dir
        .join("snapshots")
        .join(&active)
        .join("registry/capsules/skill")
        .join(source_id);
    let entries = match std::fs::read_dir(&capsules) {
        Ok(entries) => entries,
        Err(_) => {
            warnings.push(format!(
                "source `{source_id}` active snapshot `{active}` exposes no capsule catalogue"
            ));
            return Vec::new();
        }
    };
    let mut names: Vec<String> = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    names.sort_unstable();
    names
}

/// Build the machine-facts list for every registered source.
fn observe_sources(home: &AikitHome, warnings: &mut Vec<String>) -> Vec<SourceFacts> {
    let sources_root = home.root().join("sources");
    let mut facts = Vec::new();
    let entries = match std::fs::read_dir(&sources_root) {
        Ok(entries) => entries,
        Err(error) => {
            warnings.push(format!("no readable sources root: {error}"));
            return facts;
        }
    };
    let mut source_dirs: Vec<_> = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .collect();
    source_dirs.sort_by_key(|entry| entry.file_name());
    for entry in source_dirs {
        let Some(spec) = loose_toml(&entry.path().join("source.toml")) else {
            warnings.push(format!(
                "source directory `{}` has no readable source.toml",
                entry.file_name().to_string_lossy()
            ));
            continue;
        };
        let source_id = spec
            .get("id")
            .and_then(|value| value.as_str())
            .map(str::to_owned)
            .unwrap_or_else(|| entry.file_name().to_string_lossy().into_owned());
        let skills_dir = spec
            .get("path")
            .and_then(|value| value.as_str())
            .map(std::path::PathBuf::from);
        let owner_repository = skills_dir.as_deref().and_then(observed_owner_repository);
        let skill_names = active_skill_names(home, &source_id, warnings);
        facts.push(SourceFacts {
            source_id,
            owner_repository,
            skill_names,
        });
    }
    facts
}

fn resolution_json(
    member: &aikit_core::guardian_family::FamilyMember,
    facts: &[SourceFacts],
) -> Value {
    let skill_name = member_skill_name(member);
    match resolve_member(member, facts) {
        MemberResolution::Resolved {
            source_id,
            capsule_id,
        } => json!({
            "state": "resolved",
            "source_id": source_id,
            "capsule_id": capsule_id,
        }),
        MemberResolution::SourcePresentSkillMissing { source_id } => json!({
            "state": "source_present_skill_missing",
            "source_id": source_id,
            "detail": format!(
                "source `{source_id}` is registered for `{}` but its active snapshot does not catalogue `{}`; re-sync the source (`aikit source sync`) and promote if the skill is new",
                member.owner_repository,
                skill_name.unwrap_or_default(),
            ),
        }),
        MemberResolution::SourceMissing => json!({
            "state": "source_missing",
            "detail": format!(
                "no registered source observes `{}`; register the owner checkout (for example `aikit source add-directory`) and sync it — trust recording stays with the owner review",
                member.owner_repository,
            ),
        }),
    }
}

/// Emit the family read model: the embedded declaration resolved against
/// this machine's registered sources, with the registry's own gaps carried
/// verbatim.
pub fn disclose() -> Result<Value> {
    let mut warnings: Vec<String> = Vec::new();
    let home = AikitHome::discover().map_err(|error: AikitError| error)?;
    let facts = observe_sources(&home, &mut warnings);
    let family = embedded_family()?;

    let mut resolved = 0usize;
    let mut source_missing = 0usize;
    let mut skill_missing = 0usize;
    let guardians: Vec<Value> = family
        .guardians
        .iter()
        .map(|guardian| {
            let members: Vec<Value> = guardian
                .members
                .iter()
                .map(|member| {
                    let resolution = resolution_json(member, &facts);
                    match resolution["state"].as_str() {
                        Some("resolved") => resolved += 1,
                        Some("source_missing") => source_missing += 1,
                        _ => skill_missing += 1,
                    }
                    let mut entry = json!({
                        "skill_ref": member.skill_ref,
                        "owner_repository": member.owner_repository,
                        "body_path": member.body_path,
                    });
                    if let Some(classification) = &member.classification {
                        entry["classification"] = json!(classification);
                    }
                    entry["resolution"] = resolution;
                    entry
                })
                .collect();
            json!({
                "product": guardian.product,
                "semantic_ref": guardian.semantic_ref,
                "declared_native_sets": guardian.declared_native_sets,
                "set_refs": guardian.set_refs,
                "members": members,
            })
        })
        .collect();

    let total = resolved + source_missing + skill_missing;
    Ok(json!({
        "schema": "aikit.guardian-family-reading/v1",
        "registry": {
            "schema": family.schema,
            "stewardship": family.stewardship,
            "carried_by": "embedded authoritative copy (registry/guardian-family.toml) in this build",
        },
        "guardians": guardians,
        "gaps": family.gaps.iter().map(|gap| json!({
            "product": gap.product,
            "gap": gap.gap,
            "owner": gap.owner,
            "next_action": gap.next_action,
        })).collect::<Vec<Value>>(),
        "summary": {
            "members": total,
            "resolved": resolved,
            "source_missing": source_missing,
            "source_present_skill_missing": skill_missing,
            "registered_sources": facts.iter().map(|fact| fact.source_id.clone()).collect::<Vec<String>>(),
        },
        "warnings": warnings,
    }))
}

/// Collated per-source owner facts, exposed for tests and future consumers.
pub fn observed_source_facts() -> Result<BTreeMap<String, Vec<String>>> {
    let mut warnings = Vec::new();
    let home = AikitHome::discover()?;
    let facts = observe_sources(&home, &mut warnings);
    Ok(facts
        .into_iter()
        .map(|fact| (fact.source_id, fact.skill_names))
        .collect())
}
