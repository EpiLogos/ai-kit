//! Skill-sets: a folder of capabilities you point a harness at.
//!
//! ```text
//! profile : resolution  ::  skill-set : projection
//! ```
//!
//! A **profile** answers *what should be active here* — it is a patch, it can
//! disable, it carries config, it composes by precedence, it feeds the resolver. A
//! **skill-set** answers *what do I hand to this harness* — it is a set, it can
//! only add, it carries nothing, it composes by union, and it feeds projection.
//! Conflating the two is how you get a plugin system too heavy to make one of
//! casually and too opaque to point at precisely.
//!
//! ## A set is a directory
//!
//! Its members are what is in it; nesting gives sub-sets; there is no other rule.
//! `mkdir` is a legitimate way to create one. A manifest is optional and exists
//! only when the folder cannot say something (an out-of-registry member, a
//! presentation order). If writing one ever feels like an architectural act, this
//! module has failed.
//!
//! ## The two invariants this module owns
//!
//! 1. **Withholding.** *Projecting a set projects only those members that pass
//!    their own gates.* A set has no trust of its own and must not be able to
//!    acquire any, because otherwise aggregation is a trust-laundering path:
//!    bundle a reviewed skill with an unreviewed hook, point a harness at the
//!    bundle, and the bundle's reputation carries the hook in. A set is a
//!    *request*, not an authority — so it reports what it dropped and why.
//!
//! 2. **Union, and only union.** No `exclude`, no precedence, no override. If you
//!    want to subtract, you want a profile — that is what profiles are for, and
//!    they already do it with full explainability. Giving sets subtraction would
//!    recreate the resolver inside the projection layer with none of its
//!    guarantees.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::id::CapsuleId;
use crate::resolve::{ResolvedView, UnavailableReason};

/// Where a set came from, and therefore who may write to it.
///
/// The three behave identically at the point of use; the distinction only shows up
/// in who may write to them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "provenance", rename_all = "kebab-case")]
pub enum SetProvenance {
    /// A real directory that already exists on the machine (`~/.hermes/skills/nara/`).
    /// Indexed and pointable-at; read-only until adopted.
    Observed { path: std::path::PathBuf },
    /// A virtual directory AIKit builds from capsules across registries and
    /// materializes into a generation. The dynamic case.
    Composed,
    /// `<repo>/.aikit/sets/<name>/` — committed, shared with the team.
    Project,
}

impl SetProvenance {
    pub fn as_str(&self) -> &'static str {
        match self {
            SetProvenance::Observed { .. } => "observed",
            SetProvenance::Composed => "composed",
            SetProvenance::Project => "project",
        }
    }

    /// Whether AIKit may write to this set in place.
    pub fn is_writable(&self) -> bool {
        !matches!(self, SetProvenance::Observed { .. })
    }

    /// `@` marks an observed set, so the origin of membership is visible at the
    /// point of use rather than requiring a lookup.
    pub fn sigil(&self) -> &'static str {
        match self {
            SetProvenance::Observed { .. } => "@",
            _ => "",
        }
    }
}

/// How a member got into a set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "membership", rename_all = "kebab-case")]
pub enum SetMembership {
    /// Named explicitly, or expanded from a glob at authoring time.
    Explicit,
    /// Found in an observed directory.
    Observed,
}

/// A folder of capabilities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillSet {
    pub name: String,
    pub provenance: SetProvenance,
    #[serde(default)]
    pub description: String,
    /// Members, in a stable order. A `BTreeMap` because a set is a *set*: naming
    /// the same member twice is idempotent, not a duplicate.
    pub members: BTreeMap<CapsuleId, SetMembership>,
    /// Nested sets, giving sub-sets for free.
    #[serde(default)]
    pub children: Vec<SkillSet>,
    /// The glob a `--match` authoring step expanded, retained as provenance only.
    ///
    /// Globs expand at **authoring** time, never at resolution time: if sets
    /// matched dynamically, syncing a registry would silently change what a
    /// harness sees, which is precisely the failure Part I rule 6 exists to
    /// prevent. A newly catalogued capsule matching a retained pattern raises an
    /// inbox item; it never joins by itself.
    #[serde(default)]
    pub patterns: Vec<String>,
}

impl SkillSet {
    pub fn new(name: impl Into<String>, provenance: SetProvenance) -> Self {
        Self {
            name: name.into(),
            provenance,
            description: String::new(),
            members: BTreeMap::new(),
            children: Vec::new(),
            patterns: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_member(mut self, id: CapsuleId, membership: SetMembership) -> Self {
        self.members.insert(id, membership);
        self
    }

    #[must_use]
    pub fn with_child(mut self, child: SkillSet) -> Self {
        self.children.push(child);
        self
    }

    /// The display name, carrying the observed sigil.
    pub fn label(&self) -> String {
        format!("{}{}", self.provenance.sigil(), self.name)
    }

    /// Every member of this set and of every nested set, deduplicated.
    ///
    /// Nesting is the only composition a folder needs: point a harness at the
    /// parent and it gets everything; point it at a child and it gets the subtree.
    pub fn all_members(&self) -> BTreeSet<CapsuleId> {
        let mut out: BTreeSet<CapsuleId> = self.members.keys().cloned().collect();
        for child in &self.children {
            out.extend(child.all_members());
        }
        out
    }

    pub fn len(&self) -> usize {
        self.all_members().len()
    }

    pub fn is_empty(&self) -> bool {
        self.all_members().is_empty()
    }
}

/// Why a set did not get a member it asked for.
///
/// Deliberately wider than [`UnavailableReason`]: resolution answers "could this
/// activate", but a set also asks for things no scope selects here, and reporting
/// that as "not present in any registry" would send a user looking for a missing
/// file that is sitting right there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "withheld", rename_all = "kebab-case")]
pub enum WithheldReason {
    /// Catalogued, and perfectly fine — but no scope enables it in this context.
    /// A set is a projection request, not an activation, so it cannot enable it
    /// either (rule 6).
    NotSelected,
    /// Resolution refused it, with its own reason.
    Unavailable(UnavailableReason),
}

impl WithheldReason {
    pub fn describe(&self) -> String {
        match self {
            WithheldReason::NotSelected => {
                "catalogued, but no scope enables it in this context".to_string()
            }
            WithheldReason::Unavailable(reason) => reason.describe(),
        }
    }

    fn short(&self) -> &'static str {
        match self {
            WithheldReason::NotSelected => "not enabled here",
            WithheldReason::Unavailable(reason) => short_reason(reason),
        }
    }
}

/// One member a set asked for and did not get.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Withheld {
    pub capsule: CapsuleId,
    pub reason: WithheldReason,
}

impl Withheld {
    /// The sentence the palette and `aikit set show` print.
    pub fn describe(&self) -> String {
        format!("{} — {}", self.capsule, self.reason.describe())
    }
}

/// Why one member did not project, decided from the view.
fn withheld_reason(view: &ResolvedView, capsule: &CapsuleId) -> WithheldReason {
    match view.unavailable_reason(capsule) {
        Some(reason) => WithheldReason::Unavailable(reason.clone()),
        // In the catalogue but not refused: nothing selects it here.
        None if view.catalog_index.contains_key(capsule) => WithheldReason::NotSelected,
        None => WithheldReason::Unavailable(UnavailableReason::NotInCatalog),
    }
}

/// The reply to a set's request: what will project, and what will not.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetProjection {
    pub projected: Vec<CapsuleId>,
    pub withheld: Vec<Withheld>,
}

impl SetProjection {
    pub fn is_complete(&self) -> bool {
        self.withheld.is_empty()
    }

    /// `sets/nara/paśyantī — 6 members, 4 projected, 2 withheld (unreviewed)`.
    ///
    /// One line, because that is what a screen reader gets and what an agent gets,
    /// and those being the same thing is not a coincidence.
    pub fn summarize(&self, label: &str) -> String {
        let total = self.projected.len() + self.withheld.len();
        let mut line = format!(
            "{label} — {total} member{}, {} projected",
            if total == 1 { "" } else { "s" },
            self.projected.len()
        );
        if !self.withheld.is_empty() {
            let mut reasons: Vec<String> = self
                .withheld
                .iter()
                .map(|w| w.reason.short().to_string())
                .collect();
            reasons.sort();
            reasons.dedup();
            line.push_str(&format!(
                ", {} withheld ({})",
                self.withheld.len(),
                reasons.join(", ")
            ));
        }
        line
    }
}

fn short_reason(reason: &UnavailableReason) -> &'static str {
    match reason {
        UnavailableReason::NotInCatalog => "not installed",
        UnavailableReason::DeniedByPolicy => "denied by policy",
        UnavailableReason::PlatformUnsupported => "wrong platform",
        UnavailableReason::NoSupportedTarget => "no supported target",
        UnavailableReason::TrustRequired => "unreviewed",
        UnavailableReason::Quarantined => "quarantined",
        UnavailableReason::Blocked => "blocked",
        UnavailableReason::DependencyUnavailable { .. } => "dependency unavailable",
    }
}

/// Project a set against a resolved view.
///
/// **This is the safety property.** Each member is checked against its own gates,
/// exactly as it would be alone: a set confers nothing. A member that is not active
/// in the view is withheld with the view's own reason, so the answer a user gets is
/// the same answer `aikit explain` would give — there is no second opinion about
/// availability living in the projection layer.
pub fn project(set: &SkillSet, view: &ResolvedView) -> SetProjection {
    let mut projection = SetProjection::default();

    for capsule in set.all_members() {
        if view.is_active(&capsule) {
            projection.projected.push(capsule);
            continue;
        }
        // The view already knows why, in the same words `explain` uses.
        let reason = withheld_reason(view, &capsule);
        projection.withheld.push(Withheld { capsule, reason });
    }

    projection
}

/// Compose several sets by union, and only union.
///
/// There is no ordering question to answer because there is no precedence: a
/// capability is in the union or it is not. That is the whole reason union is the
/// only operation — anything else would need a conflict rule, and a conflict rule
/// in the projection layer is the resolver rebuilt without its guarantees.
pub fn union(sets: &[&SkillSet]) -> BTreeSet<CapsuleId> {
    let mut out = BTreeSet::new();
    for set in sets {
        out.extend(set.all_members());
    }
    out
}

/// Project the union of several sets, reporting every withholding once.
pub fn project_union(sets: &[&SkillSet], view: &ResolvedView) -> SetProjection {
    let mut projection = SetProjection::default();
    for capsule in union(sets) {
        if view.is_active(&capsule) {
            projection.projected.push(capsule);
        } else {
            let reason = withheld_reason(view, &capsule);
            projection.withheld.push(Withheld { capsule, reason });
        }
    }
    projection
}

/// A capsule that matches a set's retained pattern but is not a member.
///
/// New matches are **proposed, never joined** — the inbox item is the whole
/// mechanism, and it is what lets a glob be a convenience without becoming a
/// dynamic membership rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetCandidate {
    pub set: String,
    pub capsule: CapsuleId,
    pub pattern: String,
}

/// Find capsules that match a set's retained patterns but are not members.
pub fn candidates(set: &SkillSet, catalogued: &[CapsuleId]) -> Vec<SetCandidate> {
    let members = set.all_members();
    let mut out = Vec::new();
    for pattern in &set.patterns {
        for id in catalogued {
            if !members.contains(id) && glob_matches(pattern, &id.to_string()) {
                out.push(SetCandidate {
                    set: set.name.clone(),
                    capsule: id.clone(),
                    pattern: pattern.clone(),
                });
            }
        }
    }
    out.sort_by(|a, b| (&a.capsule, &a.pattern).cmp(&(&b.capsule, &b.pattern)));
    out.dedup();
    out
}

/// A deliberately small glob: `*` matches within a path segment, `**` across them.
///
/// Written by hand rather than pulled in as a dependency because the whole grammar
/// is two cases and a crate would buy features this never uses (STANDARDS §7).
pub fn glob_matches(pattern: &str, value: &str) -> bool {
    fn matches(pattern: &[u8], value: &[u8]) -> bool {
        if pattern.is_empty() {
            return value.is_empty();
        }
        if pattern[0] == b'*' {
            // `**` crosses `/`; a single `*` does not.
            let crosses = pattern.len() > 1 && pattern[1] == b'*';
            let rest = if crosses {
                &pattern[2..]
            } else {
                &pattern[1..]
            };
            let mut index = 0;
            loop {
                if matches(rest, &value[index..]) {
                    return true;
                }
                if index >= value.len() {
                    return false;
                }
                if !crosses && value[index] == b'/' {
                    return false;
                }
                index += 1;
            }
        }
        if value.is_empty() || pattern[0] != value[0] {
            return false;
        }
        matches(&pattern[1..], &value[1..])
    }
    matches(pattern.as_bytes(), value.as_bytes())
}
