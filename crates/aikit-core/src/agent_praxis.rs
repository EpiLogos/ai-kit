//! Agent praxis self-disclosure: the Agent read as one relational whole.
//!
//! ```text
//! 0/1  AGENT        the situated whole
//! #0   INTENT       why am I here?              profile purpose + retained intent expression
//! #1   SKILL        what can I do?              unprefixed Skills
//! #2   METHOD       how do I act?               `METHOD:` Skills
//! #3   METHODOLOGY  how do I orient?            `METHODOLOGY:` Skills
//! #4   SKILLSET     what repertoire do I carry? authored SkillSet refs, resolved
//! #5   WORLD        where am I?                 world refs (O:I composes participation)
//! 5→0  RETURN       what changed, and where does it go back?
//! ```
//!
//! This is a read model over sources other owners hold: Central's AgentProfile
//! (identity, intent, authored repertoire), AIKit's resolved SkillSets and
//! catalogue (classification, availability, projection), and evidence of what
//! actually happened in an act (loaded / invoked / relied upon / verified). It
//! never stores anything and never rewrites a source.
//!
//! The disclosure keeps constitutive involvement apart from attribution. Every
//! rung of a Skill's [`Involvement`] ladder is `Some(true)`, `Some(false)` or
//! `None` (not observed), and a rung is only `true` with the evidence that
//! establishes it. Carrying a Skill does not mean it was loaded; loading does
//! not mean it was invoked; success does not mean it is generally fit.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::id::CapsuleId;
use crate::method::{praxis_form, praxis_payload, PraxisForm};
use crate::skillset::SkillSet;

pub const AGENT_PRAXIS_DISCLOSURE_SCHEMA: &str = "aikit.agent-praxis-disclosure/v1";
pub const PRAXIS_ACTIVITY_SCHEMA: &str = "aikit.praxis-activity/v1";

/// The subset of Central's `central.agent-profile/v1` a disclosure reads.
/// Unknown fields are ignored: Central owns the schema, AIKit only reads it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentProfileFacts {
    #[serde(default)]
    pub schema: String,
    #[serde(rename = "ref")]
    pub profile_ref: String,
    #[serde(default)]
    pub revision: String,
    pub agent_ref: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub world_ref: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub purpose: Option<String>,
    #[serde(default)]
    pub skill_refs: Vec<String>,
    #[serde(default)]
    pub skill_set_refs: Vec<String>,
    #[serde(default)]
    pub method_refs: Vec<String>,
    #[serde(default)]
    pub ratified_world_refs: Vec<String>,
    #[serde(default)]
    pub governance_refs: Vec<String>,
    #[serde(default)]
    pub knowledge_source_refs: Vec<String>,
    #[serde(default)]
    pub intent_provenance: Option<IntentProvenanceFacts>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntentProvenanceFacts {
    #[serde(default)]
    pub intent_expression: String,
    #[serde(default)]
    pub origin_action: String,
    #[serde(default)]
    pub authorship: String,
    #[serde(default)]
    pub recognition: String,
}

/// Evidence of what actually happened in an act (`aikit.praxis-activity/v1`),
/// supplied by the harness, a hook reading, a Run or a Return. Each list names
/// Skill ids for which that rung is established.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PraxisActivity {
    #[serde(default)]
    pub schema: String,
    #[serde(default)]
    pub loaded: Vec<String>,
    #[serde(default)]
    pub invoked: Vec<String>,
    #[serde(default)]
    pub relied_upon: Vec<String>,
    #[serde(default)]
    pub succeeded: Vec<String>,
    #[serde(default)]
    pub verified: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub return_destinations: Vec<String>,
    #[serde(default)]
    pub activity_refs: Vec<String>,
}

/// What the resolved catalogue and the current context say about one Skill.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillFacts {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub revision: Option<String>,
    /// Passes its own trust/policy/standing gates here (`None` if unknown).
    #[serde(default)]
    pub available: Option<bool>,
    /// Materialised into the harness tree for this context (`None` if unknown).
    #[serde(default)]
    pub projected: Option<bool>,
    #[serde(default)]
    pub withheld_reason: Option<String>,
}

/// One authored SkillSet ref and what reading it returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetReading {
    pub reference: String,
    pub result: std::result::Result<SkillSet, String>,
    /// Where the set was read from (`home`, a registry root, …).
    pub origin: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DisclosureInput {
    pub profile: AgentProfileFacts,
    pub sets: Vec<SetReading>,
    pub catalogue: BTreeMap<CapsuleId, SkillFacts>,
    pub activity: Option<PraxisActivity>,
    /// Skills explicitly selected for the current act (a Method, a Focus binding).
    pub selected: Vec<String>,
    pub context_id: Option<String>,
    /// The NOW location the acting session stands in, when disclosed.
    pub now_location: Option<NowLocationFacts>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentPraxisDisclosure {
    pub schema: String,
    pub agent: AgentIdentity,
    pub expression: Expression,
    pub repertoire: Repertoire,
    pub praxis: Vec<PraxisEntry>,
    pub world: WorldRefs,
    pub operative: Operative,
    #[serde(rename = "return")]
    pub return_relation: ReturnRelation,
    pub answers: Answers,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentIdentity {
    pub agent_ref: String,
    pub profile_ref: String,
    pub profile_revision: String,
    pub name: Option<String>,
}

/// `#0` — the originating expression, with its standing kept honest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Expression {
    pub purpose: Option<String>,
    pub role: Option<String>,
    pub intent_expression: Option<String>,
    pub authorship: Option<String>,
    pub recognition: Option<String>,
    pub governance_refs: Vec<String>,
}

/// `#4` — the authored repertoire and what it resolves to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repertoire {
    pub authored_skill_sets: Vec<AuthoredSet>,
    /// Every set reached, authored ones and their children, by reference.
    pub effective_skill_sets: Vec<String>,
    pub direct_skill_refs: Vec<String>,
    pub method_refs: Vec<String>,
    pub unresolved: Vec<Unresolved>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthoredSet {
    #[serde(rename = "ref")]
    pub reference: String,
    pub resolved: bool,
    pub origin: String,
    pub description: String,
    pub revision: Option<String>,
    pub members: Vec<String>,
    pub children: Vec<String>,
    pub withheld: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unresolved {
    #[serde(rename = "ref")]
    pub reference: String,
    pub reason: String,
}

/// The involvement ladder. `None` means not observed — never "false".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Involvement {
    pub carried: Option<bool>,
    pub catalogued: Option<bool>,
    pub available: Option<bool>,
    pub selected: Option<bool>,
    pub projected: Option<bool>,
    pub loaded: Option<bool>,
    pub invoked: Option<bool>,
    pub relied_upon: Option<bool>,
    pub succeeded: Option<bool>,
    pub verified: Option<bool>,
}

/// `#1–#3` — one Skill identity with its classification and involvement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PraxisEntry {
    pub id: String,
    pub name: String,
    pub form: PraxisForm,
    pub position: u8,
    pub payload: String,
    pub revision: Option<String>,
    pub via: Vec<String>,
    pub involvement: Involvement,
    pub withheld_reason: Option<String>,
}

/// `#5` — the World refs this Agent's profile names. The full participation
/// (occupancy, authority, material presence, citizenship) is composed by O:I
/// from its owners; AIKit only discloses the refs it was given.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldRefs {
    pub world_ref: Option<String>,
    pub ratified_world_refs: Vec<String>,
    pub scope: Option<String>,
    pub knowledge_source_refs: Vec<String>,
}

/// The NOW location of the acting session — the owner's "where": a Workcell
/// seat on a machine, in a register. The Workcell IS the NOW location; the
/// bounded worktrees of a project are workcells on its machine. The caller
/// supplies these from native records (the machine's Workcell binding in
/// `Control/machines/current.json`, the seat's own checkout state); the
/// disclosure composes them into the where-am-I answer verbatim and
/// invents nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NowLocationFacts {
    /// The Workcell the session stands in (e.g. `workcell:mac`, or a bounded
    /// seat worktree on it, named as the owner names it).
    pub workcell_ref: String,
    /// The machine the Workcell runs on, when disclosed.
    pub machine_ref: Option<String>,
    /// Which register this session's work lands in (`project:<name>` or the
    /// root register), when disclosed.
    pub register: Option<String>,
    /// The checkout root the seat occupies.
    pub checkout_root: Option<String>,
    /// The branch the seat stands on.
    pub branch: Option<String>,
    /// True when this seat is the project's primary checkout standing on main.
    pub primary_on_main: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Operative {
    pub context_id: Option<String>,
    pub selected: Vec<String>,
    pub loaded: Vec<String>,
    pub invoked: Vec<String>,
    pub evidence_refs: Vec<String>,
}

/// `5→0` — where returned reality goes. Disclosure never rewrites source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReturnRelation {
    pub destinations: Vec<String>,
    pub activity_refs: Vec<String>,
    pub source_rewritten: bool,
}

/// The short human questions, answered from the reading above.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Answers {
    pub why_am_i_here: String,
    pub what_can_i_do: String,
    pub how_do_i_work: String,
    pub how_do_i_orient: String,
    pub what_do_i_carry: String,
    pub where_am_i: String,
    pub what_is_operative_now: String,
    pub what_changed: String,
    pub where_does_it_go_back: String,
}

/// Assemble the disclosure. Pure: every fact comes from `input`.
pub fn disclose_agent_praxis(input: &DisclosureInput) -> AgentPraxisDisclosure {
    let profile = &input.profile;
    let activity = input.activity.clone().unwrap_or_default();

    // #4 — authored sets, their subtrees, and which Skill came through which set.
    let mut via: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut authored = Vec::new();
    let mut effective = Vec::new();
    let mut unresolved = Vec::new();
    for reading in &input.sets {
        match &reading.result {
            Ok(set) => {
                for node in set.subtree() {
                    let reference = node.reference().to_string();
                    if !effective.contains(&reference) {
                        effective.push(reference.clone());
                    }
                    for member in node.members.keys() {
                        via.entry(member.to_string())
                            .or_default()
                            .insert(format!("set:{reference}"));
                    }
                }
                let members: Vec<String> =
                    set.all_members().iter().map(|id| id.to_string()).collect();
                let withheld = members
                    .iter()
                    .filter(|id| {
                        CapsuleId::parse(id.as_str())
                            .ok()
                            .and_then(|capsule| input.catalogue.get(&capsule))
                            .is_none_or(|facts| facts.available != Some(true))
                    })
                    .cloned()
                    .collect();
                authored.push(AuthoredSet {
                    reference: reading.reference.clone(),
                    resolved: true,
                    origin: reading.origin.clone(),
                    description: set.description.clone(),
                    revision: set.revision.clone(),
                    members,
                    children: set
                        .children
                        .iter()
                        .map(|c| c.reference().to_string())
                        .collect(),
                    withheld,
                });
            }
            Err(reason) => {
                authored.push(AuthoredSet {
                    reference: reading.reference.clone(),
                    resolved: false,
                    origin: reading.origin.clone(),
                    description: String::new(),
                    revision: None,
                    members: Vec::new(),
                    children: Vec::new(),
                    withheld: Vec::new(),
                });
                unresolved.push(Unresolved {
                    reference: reading.reference.clone(),
                    reason: reason.clone(),
                });
            }
        }
    }
    for skill in &profile.skill_refs {
        via.entry(skill.clone())
            .or_default()
            .insert("direct".into());
    }
    for method in &profile.method_refs {
        via.entry(method.clone())
            .or_default()
            .insert("method-ref".into());
    }

    let has = |list: &[String], id: &str| list.iter().any(|value| value == id);
    let observed = |list: &[String], id: &str, any_evidence: bool| {
        if has(list, id) {
            Some(true)
        } else if any_evidence {
            // Evidence for this rung exists for the act and does not name this
            // Skill: it was observed not to be involved at this rung.
            Some(false)
        } else {
            None
        }
    };
    let activity_present = input.activity.is_some();

    // #1–#3 — every carried Skill, classified from its ordinary identity.
    let mut praxis = Vec::new();
    for (id, routes) in &via {
        let facts = CapsuleId::parse(id.as_str())
            .ok()
            .and_then(|capsule| input.catalogue.get(&capsule));
        let (name, description, revision) = match facts {
            Some(facts) => (
                facts.name.clone(),
                facts.description.clone(),
                facts.revision.clone(),
            ),
            None => (id.clone(), String::new(), None),
        };
        let form = praxis_form(&description);
        if routes.contains("method-ref") && form != PraxisForm::Method {
            unresolved.push(Unresolved {
                reference: id.clone(),
                reason: if facts.is_none() {
                    "method_ref names a Skill absent from the catalogue".into()
                } else {
                    "method_ref names a Skill whose description lacks the METHOD: prefix".into()
                },
            });
        }
        let involvement = Involvement {
            carried: Some(true),
            catalogued: Some(facts.is_some()),
            available: facts.and_then(|facts| facts.available),
            selected: Some(has(&input.selected, id)),
            projected: facts.and_then(|facts| facts.projected),
            loaded: observed(
                &activity.loaded,
                id,
                activity_present && !activity.loaded.is_empty(),
            ),
            invoked: observed(
                &activity.invoked,
                id,
                activity_present && !activity.invoked.is_empty(),
            ),
            relied_upon: observed(
                &activity.relied_upon,
                id,
                activity_present && !activity.relied_upon.is_empty(),
            ),
            succeeded: has(&activity.succeeded, id).then_some(true),
            verified: has(&activity.verified, id).then_some(true),
        };
        let withheld_reason = match facts {
            None => Some("not in the resolved catalogue".to_string()),
            Some(facts) if facts.available == Some(false) => facts
                .withheld_reason
                .clone()
                .or_else(|| Some("unavailable in this context".into())),
            Some(_) => None,
        };
        praxis.push(PraxisEntry {
            id: id.clone(),
            name,
            form,
            position: form.position(),
            payload: praxis_payload(&description).to_string(),
            revision,
            via: routes.iter().cloned().collect(),
            involvement,
            withheld_reason,
        });
    }
    praxis.sort_by(|a, b| (a.position, &a.id).cmp(&(b.position, &b.id)));

    let world = WorldRefs {
        world_ref: profile.world_ref.clone(),
        ratified_world_refs: profile.ratified_world_refs.clone(),
        scope: profile.scope.clone(),
        knowledge_source_refs: profile.knowledge_source_refs.clone(),
    };
    let operative = Operative {
        context_id: input.context_id.clone(),
        selected: input.selected.clone(),
        loaded: activity.loaded.clone(),
        invoked: activity.invoked.clone(),
        evidence_refs: activity.evidence_refs.clone(),
    };
    let return_relation = ReturnRelation {
        destinations: activity.return_destinations.clone(),
        activity_refs: activity.activity_refs.clone(),
        source_rewritten: false,
    };
    let expression = Expression {
        purpose: profile.purpose.clone(),
        role: profile.role.clone(),
        intent_expression: profile
            .intent_provenance
            .as_ref()
            .map(|intent| intent.intent_expression.clone()),
        authorship: profile
            .intent_provenance
            .as_ref()
            .map(|intent| intent.authorship.clone()),
        recognition: profile
            .intent_provenance
            .as_ref()
            .map(|intent| intent.recognition.clone()),
        governance_refs: profile.governance_refs.clone(),
    };
    let repertoire = Repertoire {
        authored_skill_sets: authored,
        effective_skill_sets: effective,
        direct_skill_refs: profile.skill_refs.clone(),
        method_refs: profile.method_refs.clone(),
        unresolved,
    };
    let answers = answer(
        profile,
        &expression,
        &repertoire,
        &praxis,
        &world,
        &operative,
        &return_relation,
        input.now_location.as_ref(),
    );

    AgentPraxisDisclosure {
        schema: AGENT_PRAXIS_DISCLOSURE_SCHEMA.into(),
        agent: AgentIdentity {
            agent_ref: profile.agent_ref.clone(),
            profile_ref: profile.profile_ref.clone(),
            profile_revision: profile.revision.clone(),
            name: profile.name.clone(),
        },
        expression,
        repertoire,
        praxis,
        world,
        operative,
        return_relation,
        answers,
    }
}

fn names(praxis: &[PraxisEntry], form: PraxisForm) -> Vec<&str> {
    praxis
        .iter()
        .filter(|entry| entry.form == form)
        .map(|entry| entry.name.as_str())
        .collect()
}

fn list(items: &[&str], empty: &str) -> String {
    match items.len() {
        0 => empty.to_string(),
        1..=6 => items.join(", "),
        n => format!("{} and {} more", items[..6].join(", "), n - 6),
    }
}

#[allow(clippy::too_many_arguments)]
fn answer(
    profile: &AgentProfileFacts,
    expression: &Expression,
    repertoire: &Repertoire,
    praxis: &[PraxisEntry],
    world: &WorldRefs,
    operative: &Operative,
    returned: &ReturnRelation,
    now_location: Option<&NowLocationFacts>,
) -> Answers {
    let why = match (&expression.purpose, &expression.recognition) {
        (Some(purpose), Some(recognition)) if recognition != "recognised" => {
            format!("{purpose} (intent standing: {recognition})")
        }
        (Some(purpose), _) => purpose.clone(),
        (None, _) => "no purpose is recorded on the profile".into(),
    };
    let skills = names(praxis, PraxisForm::Skill);
    let methods = names(praxis, PraxisForm::Method);
    let methodologies = names(praxis, PraxisForm::Methodology);
    let sets: Vec<&str> = repertoire
        .authored_skill_sets
        .iter()
        .map(|set| set.reference.as_str())
        .collect();
    let carry = if sets.is_empty() && repertoire.direct_skill_refs.is_empty() {
        "no SkillSets or Skills are assigned".to_string()
    } else {
        let mut parts = Vec::new();
        if !sets.is_empty() {
            parts.push(format!("SkillSets {}", sets.join(", ")));
        }
        if !repertoire.direct_skill_refs.is_empty() {
            parts.push(format!(
                "{} directly assigned Skill(s)",
                repertoire.direct_skill_refs.len()
            ));
        }
        if !repertoire.unresolved.is_empty() {
            parts.push(format!("{} unresolved", repertoire.unresolved.len()));
        }
        parts.join("; ")
    };
    let world_part = match &world.world_ref {
        Some(world_ref) => {
            let others: Vec<&str> = world
                .ratified_world_refs
                .iter()
                .filter(|value| *value != world_ref)
                .map(String::as_str)
                .collect();
            if others.is_empty() {
                format!(
                    "{world_ref} ({})",
                    world.scope.as_deref().unwrap_or("scope unknown")
                )
            } else {
                format!(
                    "{world_ref} ({}); also ratified for {}",
                    world.scope.as_deref().unwrap_or("scope unknown"),
                    others.join(", ")
                )
            }
        }
        None => "no World is named on the profile".into(),
    };
    let where_am_i = match now_location {
        Some(now) => {
            let mut parts = vec![world_part];
            let mut location = format!("NOW location {}", now.workcell_ref);
            if let Some(machine) = &now.machine_ref {
                location.push_str(&format!(" on machine {machine}"));
            }
            parts.push(location);
            if let Some(register) = &now.register {
                parts.push(format!("work lands in the {register} register"));
            }
            match (&now.checkout_root, &now.branch, now.primary_on_main) {
                (Some(root), Some(branch), Some(true)) => {
                    parts.push(format!("seated at {root} on {branch} (the project's primary checkout)"))
                }
                (Some(root), Some(branch), _) => parts.push(format!(
                    "seated at {root} on {branch} (a development seat; a landed lane releases its seat)"
                )),
                (None, Some(branch), _) => parts.push(format!("standing on branch {branch}")),
                (Some(root), None, _) => parts.push(format!("seated at {root}")),
                (None, None, None) => {}
                (None, None, Some(primary)) => parts.push(format!(
                    "primary checkout on main: {primary}"
                )),
            }
            parts.join("; ")
        }
        None => world_part,
    };
    let operative_now = if operative.selected.is_empty()
        && operative.loaded.is_empty()
        && operative.invoked.is_empty()
    {
        "nothing is observed as selected, loaded or invoked in this act; carried praxis is only available".to_string()
    } else {
        format!(
            "selected {}; loaded {}; invoked {}",
            operative.selected.len(),
            operative.loaded.len(),
            operative.invoked.len()
        )
    };
    let changed = if returned.activity_refs.is_empty() && operative.evidence_refs.is_empty() {
        "no activity or evidence was supplied for this reading".to_string()
    } else {
        format!(
            "{} activity ref(s), {} evidence ref(s); no source was rewritten",
            returned.activity_refs.len(),
            operative.evidence_refs.len()
        )
    };
    let back = if returned.destinations.is_empty() {
        format!(
            "returns go to their native owners; none is named for this act yet (profile {})",
            profile.profile_ref
        )
    } else {
        returned.destinations.join(", ")
    };
    Answers {
        why_am_i_here: why,
        what_can_i_do: list(&skills, "no ordinary Skills are carried"),
        how_do_i_work: list(&methods, "no Methods are carried"),
        how_do_i_orient: list(&methodologies, "no Methodology is carried"),
        what_do_i_carry: carry,
        where_am_i,
        what_is_operative_now: operative_now,
        what_changed: changed,
        where_does_it_go_back: back,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skillset::{SetMembership, SetProvenance};

    fn id(value: &str) -> CapsuleId {
        CapsuleId::parse(value).unwrap()
    }

    fn facts(name: &str, description: &str, available: bool) -> SkillFacts {
        SkillFacts {
            name: name.into(),
            description: description.into(),
            revision: Some(format!("rev-{name}")),
            available: Some(available),
            projected: Some(available),
            withheld_reason: (!available).then(|| "trust required".to_string()),
        }
    }

    fn documentation_set() -> SkillSet {
        let mut accounts = SkillSet::new("account-authoring", SetProvenance::Composed)
            .with_member(id("skill/aikit/html-account"), SetMembership::Explicit);
        accounts.semantic_ref = Some("aikit:account-authoring".into());
        accounts.attached_by = Some("aikit:account-authoring".into());
        let mut docs = SkillSet::new("documentation", SetProvenance::Composed)
            .with_member(
                id("skill/central/docs-methodology"),
                SetMembership::Explicit,
            )
            .with_member(
                id("skill/central/vision-authoring"),
                SetMembership::Explicit,
            )
            .with_member(id("skill/central/ui-development"), SetMembership::Explicit)
            .with_child(accounts);
        docs.semantic_ref = Some("central:documentation".into());
        docs
    }

    fn core_set() -> SkillSet {
        let mut core = SkillSet::new("core-development", SetProvenance::Composed)
            .with_member(id("skill/personal/wayfinder"), SetMembership::Explicit)
            .with_member(id("skill/personal/grilling"), SetMembership::Explicit)
            .with_member(
                id("skill/mattpocock/engineering/research"),
                SetMembership::Explicit,
            );
        core.semantic_ref = Some("central:core-development".into());
        core
    }

    fn catalogue() -> BTreeMap<CapsuleId, SkillFacts> {
        BTreeMap::from([
            (
                id("skill/central/docs-methodology"),
                facts(
                    "docs-methodology",
                    "METHODOLOGY: orient the documentation field",
                    true,
                ),
            ),
            (
                id("skill/central/vision-authoring"),
                facts("vision-authoring", "Author a Vision account.", true),
            ),
            (
                id("skill/central/ui-development"),
                facts("ui-development", "METHOD: UI development path", true),
            ),
            (
                id("skill/aikit/html-account"),
                facts("html-account", "Self-contained HTML accounts.", false),
            ),
            (
                id("skill/personal/wayfinder"),
                facts(
                    "wayfinder",
                    "METHODOLOGY: chart the developmental field",
                    true,
                ),
            ),
            (
                id("skill/personal/grilling"),
                facts("grilling", "METHOD: grill a plan", true),
            ),
            (
                id("skill/mattpocock/engineering/research"),
                facts("research", "Investigate against primary sources.", true),
            ),
        ])
    }

    fn profile() -> AgentProfileFacts {
        AgentProfileFacts {
            schema: "central.agent-profile/v1".into(),
            profile_ref: "profile/factory-builder".into(),
            revision: "r2".into(),
            agent_ref: "agent/factory-builder".into(),
            name: Some("Factory builder".into()),
            scope: Some("project".into()),
            world_ref: Some("project:Factory".into()),
            purpose: Some("Develop Factory capabilities from authored intent".into()),
            skill_set_refs: vec![
                "central:core-development".into(),
                "central:documentation".into(),
            ],
            ratified_world_refs: vec!["project:Factory".into(), "project:O-I".into()],
            intent_provenance: Some(IntentProvenanceFacts {
                intent_expression: "Build what the owner intends.".into(),
                origin_action: "agent-profile.express".into(),
                authorship: "generated-proposal".into(),
                recognition: "unrecognised".into(),
            }),
            ..AgentProfileFacts::default()
        }
    }

    fn input() -> DisclosureInput {
        DisclosureInput {
            now_location: None,
            profile: profile(),
            sets: vec![
                SetReading {
                    reference: "central:core-development".into(),
                    result: Ok(core_set()),
                    origin: "registry".into(),
                },
                SetReading {
                    reference: "central:documentation".into(),
                    result: Ok(documentation_set()),
                    origin: "registry".into(),
                },
            ],
            catalogue: catalogue(),
            activity: None,
            selected: vec![],
            context_id: Some("ctx_test".into()),
        }
    }

    #[test]
    fn now_location_answers_where_am_i_in_the_owner_s_terms() {
        let mut input = input();
        input.now_location = Some(NowLocationFacts {
            workcell_ref: "worktrees/env-2/o-i".into(),
            machine_ref: Some("workcell:mac machine Admins-MacBook-Pro-3".into()),
            register: Some("project:O-I".into()),
            checkout_root: Some("/Users/admin/Central/worktrees/env-2/o-i".into()),
            branch: Some("feat/document-surface-20260925".into()),
            primary_on_main: None,
        });
        let disclosure = disclose_agent_praxis(&input);
        let answer = &disclosure.answers.where_am_i;
        assert!(answer.contains("NOW location worktrees/env-2/o-i"), "{answer}");
        assert!(answer.contains("machine"), "{answer}");
        assert!(answer.contains("project:O-I"), "{answer}");
        assert!(answer.contains("development seat"), "{answer}");
        assert!(answer.contains("a landed lane releases its seat"), "{answer}");

        input.now_location.as_mut().unwrap().primary_on_main = Some(true);
        input.now_location.as_mut().unwrap().branch = Some("main".into());
        let disclosure = disclose_agent_praxis(&input);
        assert!(disclosure.answers.where_am_i.contains("the project's primary checkout"),
            "{}", disclosure.answers.where_am_i);

        // Without a NOW location the answer stays the world-only reading.
        input.now_location = None;
        let disclosure = disclose_agent_praxis(&input);
        assert!(!disclosure.answers.where_am_i.contains("NOW location"));
    }

    fn entry<'a>(disclosure: &'a AgentPraxisDisclosure, id: &str) -> &'a PraxisEntry {
        disclosure
            .praxis
            .iter()
            .find(|entry| entry.id == id)
            .unwrap()
    }

    #[test]
    fn profile_to_skillsets_to_classified_praxis() {
        let disclosure = disclose_agent_praxis(&input());
        assert_eq!(disclosure.schema, AGENT_PRAXIS_DISCLOSURE_SCHEMA);
        // Two authored sets, one nested child reached by reference.
        assert_eq!(disclosure.repertoire.authored_skill_sets.len(), 2);
        assert!(disclosure
            .repertoire
            .effective_skill_sets
            .contains(&"aikit:account-authoring".to_string()));
        // Core development and documentation coexist; Wayfinder and the
        // Documentation Methodology are both disclosed at #3.
        let methodologies: Vec<&str> = disclosure
            .praxis
            .iter()
            .filter(|entry| entry.form == PraxisForm::Methodology)
            .map(|entry| entry.name.as_str())
            .collect();
        assert_eq!(methodologies, vec!["docs-methodology", "wayfinder"]);
        assert_eq!(entry(&disclosure, "skill/personal/grilling").position, 2);
        assert_eq!(
            entry(&disclosure, "skill/central/vision-authoring").form,
            PraxisForm::Skill
        );
        assert_eq!(
            entry(&disclosure, "skill/aikit/html-account").via,
            vec!["set:aikit:account-authoring".to_string()]
        );
        assert!(disclosure.answers.how_do_i_orient.contains("wayfinder"));
    }

    #[test]
    fn carried_repertoire_is_not_loaded_or_invoked() {
        let disclosure = disclose_agent_praxis(&input());
        for entry in &disclosure.praxis {
            assert_eq!(entry.involvement.carried, Some(true));
            assert_eq!(entry.involvement.selected, Some(false));
            assert_eq!(entry.involvement.loaded, None, "{}", entry.id);
            assert_eq!(entry.involvement.invoked, None, "{}", entry.id);
            assert_eq!(entry.involvement.succeeded, None);
            assert_eq!(entry.involvement.verified, None);
        }
        assert!(disclosure
            .answers
            .what_is_operative_now
            .starts_with("nothing is observed"));
    }

    #[test]
    fn withheld_members_stay_withheld_with_their_reason() {
        let disclosure = disclose_agent_praxis(&input());
        let html = entry(&disclosure, "skill/aikit/html-account");
        assert_eq!(html.involvement.available, Some(false));
        assert_eq!(html.withheld_reason.as_deref(), Some("trust required"));
        let docs = disclosure
            .repertoire
            .authored_skill_sets
            .iter()
            .find(|set| set.reference == "central:documentation")
            .unwrap();
        assert!(docs
            .withheld
            .contains(&"skill/aikit/html-account".to_string()));
    }

    #[test]
    fn activity_distinguishes_selected_loaded_invoked_and_never_rewrites_source() {
        let mut input = input();
        input.selected = vec!["skill/central/ui-development".into()];
        input.activity = Some(PraxisActivity {
            schema: PRAXIS_ACTIVITY_SCHEMA.into(),
            loaded: vec![
                "skill/central/ui-development".into(),
                "skill/central/vision-authoring".into(),
            ],
            invoked: vec!["skill/central/ui-development".into()],
            relied_upon: vec![],
            succeeded: vec!["skill/central/ui-development".into()],
            verified: vec![],
            evidence_refs: vec!["factory:evidence:run-1:walk".into()],
            return_destinations: vec!["central:documentation-field:ui-design".into()],
            activity_refs: vec!["factory:run:run-1".into()],
        });
        let disclosure = disclose_agent_praxis(&input);
        let ui = entry(&disclosure, "skill/central/ui-development");
        assert_eq!(ui.involvement.selected, Some(true));
        assert_eq!(ui.involvement.loaded, Some(true));
        assert_eq!(ui.involvement.invoked, Some(true));
        assert_eq!(ui.involvement.relied_upon, None);
        assert_eq!(ui.involvement.succeeded, Some(true));
        assert_eq!(ui.involvement.verified, None);
        let vision = entry(&disclosure, "skill/central/vision-authoring");
        assert_eq!(vision.involvement.loaded, Some(true));
        assert_eq!(vision.involvement.invoked, Some(false));
        let wayfinder = entry(&disclosure, "skill/personal/wayfinder");
        assert_eq!(wayfinder.involvement.loaded, Some(false));
        assert!(!disclosure.return_relation.source_rewritten);
        assert_eq!(
            disclosure.answers.where_does_it_go_back,
            "central:documentation-field:ui-design"
        );
    }

    #[test]
    fn method_refs_stay_a_compatibility_relation_and_are_checked() {
        let mut input = input();
        input.profile.method_refs = vec![
            "skill/central/ui-development".into(),
            "skill/central/vision-authoring".into(),
        ];
        let disclosure = disclose_agent_praxis(&input);
        let ui = entry(&disclosure, "skill/central/ui-development");
        assert!(ui.via.contains(&"method-ref".to_string()));
        assert!(ui.via.contains(&"set:central:documentation".to_string()));
        assert_eq!(disclosure.repertoire.unresolved.len(), 1);
        assert!(disclosure.repertoire.unresolved[0]
            .reason
            .contains("lacks the METHOD: prefix"));
    }

    #[test]
    fn unresolved_sets_are_named_not_dropped() {
        let mut input = input();
        input.sets.push(SetReading {
            reference: "central:missing".into(),
            result: Err("no registry declares the set `central:missing`".into()),
            origin: "registry".into(),
        });
        let disclosure = disclose_agent_praxis(&input);
        assert_eq!(
            disclosure.repertoire.unresolved[0].reference,
            "central:missing"
        );
        assert!(disclosure.answers.what_do_i_carry.contains("1 unresolved"));
    }

    #[test]
    fn small_subset_agent_carries_only_what_it_is_given() {
        let mut input = input();
        input.profile.skill_set_refs = vec![];
        input.sets = vec![];
        input.profile.skill_refs = vec!["skill/mattpocock/engineering/research".into()];
        let disclosure = disclose_agent_praxis(&input);
        assert_eq!(disclosure.praxis.len(), 1);
        assert_eq!(
            disclosure.answers.how_do_i_orient,
            "no Methodology is carried"
        );
        assert_eq!(disclosure.answers.how_do_i_work, "no Methods are carried");
    }

    #[test]
    fn unrecognised_intent_keeps_its_standing_visible() {
        let disclosure = disclose_agent_praxis(&input());
        assert!(disclosure.answers.why_am_i_here.contains("unrecognised"));
        assert_eq!(
            disclosure.expression.authorship.as_deref(),
            Some("generated-proposal")
        );
    }
}
