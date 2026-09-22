//! The shipped six-product Guardian family declaration and its resolution
//! against machine-local source facts.
//!
//! The family registry (`registry/guardian-family.toml`,
//! `aikit.guardian-family/v1`) declares each product Guardian's repertoire
//! *by reference* to its native owner: every skill body stays in its owner
//! repository, and AIKit stays the resolver. What was missing is the reading
//! that admits those cross-owner references against what a given machine
//! actually has registered. This module provides the declaration types, the
//! embedded authoritative copy, and a pure resolution function over
//! [`SourceFacts`]; the CLI turns live state into those facts.
//!
//! Resolution is honest by construction: a member whose owner repository is
//! not registered reports `SourceMissing` rather than guessing from names,
//! and per-member withholding and trust stay with the existing trust gates —
//! this reading never grants anything.

/// The authoritative family declaration shipped with this workspace.
pub const FAMILY_TOML: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../registry/guardian-family.toml"
));

/// One repertoire member declared by reference to its native owner.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct FamilyMember {
    pub skill_ref: String,
    pub owner_repository: String,
    pub body_path: String,
    #[serde(default)]
    pub classification: Option<String>,
}

/// One product Guardian's declared repertoire.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct Guardian {
    pub product: String,
    pub semantic_ref: String,
    #[serde(default)]
    pub stewardship: Option<String>,
    #[serde(default)]
    pub declared_native_sets: Option<Vec<String>>,
    #[serde(default)]
    pub set_refs: Option<Vec<String>>,
    #[serde(default = "Vec::new")]
    pub members: Vec<FamilyMember>,
}

/// A gap the registry itself declares (recorded, never silently repaired).
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct FamilyGap {
    pub product: String,
    pub gap: String,
    pub owner: String,
    #[serde(default)]
    pub next_action: Option<String>,
}

/// The parsed `aikit.guardian-family/v1` document.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct GuardianFamily {
    pub schema: String,
    #[serde(default)]
    pub stewardship: Option<String>,
    #[serde(default = "Vec::new", rename = "guardian")]
    pub guardians: Vec<Guardian>,
    #[serde(default = "Vec::new")]
    pub gaps: Vec<FamilyGap>,
}

/// Parse a family registry document.
pub fn parse_family(toml_text: &str) -> Result<GuardianFamily, crate::AikitError> {
    let family: GuardianFamily = toml::from_str(toml_text).map_err(|error| {
        crate::AikitError::new(
            "family.invalid_registry",
            format!("guardian family registry is invalid: {error}"),
        )
    })?;
    if family.schema != "aikit.guardian-family/v1" {
        return Err(crate::AikitError::new(
            "family.invalid_registry",
            format!(
                "expected schema aikit.guardian-family/v1, found {}",
                family.schema
            ),
        ));
    }
    Ok(family)
}

/// Parse the embedded authoritative copy shipped with this build.
pub fn embedded_family() -> Result<GuardianFamily, crate::AikitError> {
    parse_family(FAMILY_TOML)
}

/// What the resolver knows about one registered source on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFacts {
    pub source_id: String,
    /// Normalised `owner/repo` from the source checkout's Git remote, when
    /// one could be observed. `None` is honest: the resolver does not guess
    /// ownership from names.
    pub owner_repository: Option<String>,
    /// Skill directory names catalogued in the source's active snapshot.
    pub skill_names: Vec<String>,
}

/// The outcome of resolving one family member against machine facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemberResolution {
    /// The owner checkout is registered and its active snapshot catalogues
    /// the member's skill directory.
    Resolved {
        source_id: String,
        capsule_id: String,
    },
    /// The owner checkout is registered but its active snapshot does not
    /// catalogue the member's skill directory.
    SourcePresentSkillMissing { source_id: String },
    /// No registered source observes the member's owner repository.
    SourceMissing,
}

/// Reduce a Git remote URL to its `owner/repo` tail, lowercased, with any
/// `.git` suffix removed. Accepts `https://`, `ssh://` and `git@` (scp-form)
/// remotes; `host:port/path` is left to the ordinary slash split.
pub fn normalise_remote(remote: &str) -> Option<String> {
    let without_scheme = remote
        .strip_prefix("https://")
        .or_else(|| remote.strip_prefix("http://"))
        .or_else(|| remote.strip_prefix("ssh://"))
        .unwrap_or(remote);
    let without_user = without_scheme
        .split_once('@')
        .map(|(_, rest)| rest)
        .unwrap_or(without_scheme);
    // scp form separates host from path with a colon and no preceding slash;
    // a host:port is not a path separator, so digits after the colon keep the
    // whole string.
    let path = match without_user.find(':') {
        Some(index) if !without_user[..index].contains('/') => {
            let rest = &without_user[index + 1..];
            let first = rest.split('/').next().unwrap_or("");
            if !first.is_empty() && first.chars().all(|c| c.is_ascii_digit()) {
                without_user
            } else {
                rest
            }
        }
        _ => without_user,
    };
    let trimmed = path
        .trim_end_matches('/')
        .strip_suffix(".git")
        .unwrap_or(path.trim_end_matches('/'));
    let mut segments: Vec<&str> = trimmed.split('/').filter(|s| !s.is_empty()).collect();
    if segments.len() < 2 {
        return None;
    }
    let repo = segments.pop()?;
    let owner = segments.pop()?;
    Some(format!(
        "{}/{}",
        owner.to_ascii_lowercase(),
        repo.to_ascii_lowercase()
    ))
}

/// The skill directory a member's `body_path` declares, e.g.
/// `skills/control-maintenance/SKILL.md` → `control-maintenance`.
pub fn member_skill_name(member: &FamilyMember) -> Option<String> {
    let relative = member
        .body_path
        .strip_prefix("skills/")
        .unwrap_or(&member.body_path);
    let name = relative.split('/').next()?;
    if name.is_empty() {
        None
    } else {
        Some(name.to_owned())
    }
}

/// Resolve one member against observed source facts.
pub fn resolve_member(member: &FamilyMember, sources: &[SourceFacts]) -> MemberResolution {
    let wanted = member.owner_repository.to_ascii_lowercase();
    let matches: Vec<&SourceFacts> = sources
        .iter()
        .filter(|source| {
            source
                .owner_repository
                .as_deref()
                .map(|owner| owner == wanted)
                .unwrap_or(false)
        })
        .collect();
    if matches.is_empty() {
        return MemberResolution::SourceMissing;
    }
    let skill_name = match member_skill_name(member) {
        Some(name) => name,
        None => {
            return MemberResolution::SourcePresentSkillMissing {
                source_id: matches[0].source_id.clone(),
            }
        }
    };
    for source in &matches {
        if source.skill_names.iter().any(|name| name == &skill_name) {
            return MemberResolution::Resolved {
                source_id: source.source_id.clone(),
                capsule_id: format!("skill/{}/{}", source.source_id, skill_name),
            };
        }
    }
    MemberResolution::SourcePresentSkillMissing {
        source_id: matches[0].source_id.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_family_parses_with_all_six_guardians() {
        let family = embedded_family().expect("embedded registry parses");
        assert_eq!(family.schema, "aikit.guardian-family/v1");
        let mut products: Vec<&str> = family
            .guardians
            .iter()
            .map(|guardian| guardian.product.as_str())
            .collect();
        products.sort_unstable();
        assert_eq!(
            products,
            vec![
                "AIKit",
                "Actuation",
                "Central",
                "Quaternal Logic",
                "Software Factory",
                "Workcell",
            ]
        );
        let member_count: usize = family.guardians.iter().map(|g| g.members.len()).sum();
        assert_eq!(member_count, 14, "six products' declared members");
        assert_eq!(family.gaps.len(), 2, "the registry's declared gaps");
    }

    #[test]
    fn every_member_names_an_owner_repository_and_skills_body_path() {
        let family = embedded_family().expect("embedded registry parses");
        for guardian in &family.guardians {
            for member in &guardian.members {
                assert!(
                    member.owner_repository.starts_with("EpiLogos/"),
                    "member {} names an EpiLogos owner",
                    member.skill_ref
                );
                assert!(
                    member.body_path.starts_with("skills/"),
                    "member {} names a body under skills/",
                    member.skill_ref
                );
            }
        }
    }

    #[test]
    fn member_skill_name_extracts_the_directory() {
        let family = embedded_family().expect("embedded registry parses");
        let central = family
            .guardians
            .iter()
            .find(|g| g.product == "Central")
            .expect("Central guardian");
        let member = &central.members[0];
        assert_eq!(
            member_skill_name(member).as_deref(),
            Some("control-maintenance")
        );
    }

    #[test]
    fn remote_normalisation_accepts_the_common_forms() {
        assert_eq!(
            normalise_remote("git@github.com:EpiLogos/QL-MEF.git").as_deref(),
            Some("epilogos/ql-mef")
        );
        assert_eq!(
            normalise_remote("https://github.com/EpiLogos/Actuation/").as_deref(),
            Some("epilogos/actuation")
        );
        assert_eq!(
            normalise_remote("ssh://git@github.com/EpiLogos/Workcell.git").as_deref(),
            Some("epilogos/workcell")
        );
        assert_eq!(normalise_remote("just-a-name"), None);
    }

    #[test]
    fn ql_member_resolves_through_its_registered_owner() {
        let family = embedded_family().expect("embedded registry parses");
        let ql = family
            .guardians
            .iter()
            .find(|g| g.product == "Quaternal Logic")
            .expect("QL guardian");
        let sources = vec![SourceFacts {
            source_id: "ql".to_owned(),
            owner_repository: normalise_remote("git@github.com:EpiLogos/QL-MEF.git"),
            skill_names: vec!["ql-foundations".to_owned(), "ql-operation".to_owned()],
        }];
        for member in &ql.members {
            assert_eq!(
                resolve_member(member, &sources),
                MemberResolution::Resolved {
                    source_id: "ql".to_owned(),
                    capsule_id: format!(
                        "skill/ql/{}",
                        member_skill_name(member).expect("body path names a skill")
                    ),
                }
            );
        }
    }

    #[test]
    fn unregistered_owner_reports_source_missing_not_a_guess() {
        let family = embedded_family().expect("embedded registry parses");
        let actuation = family
            .guardians
            .iter()
            .find(|g| g.product == "Actuation")
            .expect("Actuation guardian");
        let sources = vec![SourceFacts {
            source_id: "central".to_owned(),
            owner_repository: Some("epilogos/central".to_owned()),
            skill_names: vec!["control-maintenance".to_owned()],
        }];
        for member in &actuation.members {
            assert_eq!(
                resolve_member(member, &sources),
                MemberResolution::SourceMissing
            );
        }
    }

    #[test]
    fn registered_owner_without_the_skill_reports_the_precise_gap() {
        let family = embedded_family().expect("embedded registry parses");
        let central = family
            .guardians
            .iter()
            .find(|g| g.product == "Central")
            .expect("Central guardian");
        let member = &central.members[0];
        let sources = vec![SourceFacts {
            source_id: "central".to_owned(),
            owner_repository: Some("epilogos/central".to_owned()),
            skill_names: vec!["some-other-skill".to_owned()],
        }];
        assert_eq!(
            resolve_member(member, &sources),
            MemberResolution::SourcePresentSkillMissing {
                source_id: "central".to_owned()
            }
        );
    }
}
