//! `aikit praxis` and `aikit a2a` — praxis-form listing, Agent praxis
//! disclosure and the published A2A Agent Card projection.
//!
//! Everything here reads: the resolved catalogue, home and registry
//! SkillSets, a Central AgentProfile the caller hands over, and optional
//! activity evidence. Nothing is enabled, trusted, loaded or rewritten.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use aikit_core::a2a_card::{project_a2a_agent_card, projection_input_from_participation};
use aikit_core::agent_praxis::{
    decide_methodology_instantiation, disclose_agent_praxis, AgentProfileFacts, DisclosureInput,
    NowLocationFacts, PraxisActivity, SetReading, SkillFacts, SkillInvocationFacts,
};
use aikit_core::id::CapsuleId;
use aikit_core::method::{praxis_form, praxis_payload, PraxisForm};
use aikit_core::resolve::ResolvedView;
use aikit_core::trust::TrustState;
use aikit_core::Kind;
use aikit_core::{AikitError, Result};
use aikit_store::home::AikitHome;

/// Read inline JSON, `@path`, or a bare path to an existing file.
pub fn read_json(raw: &str, label: &str) -> Result<Value> {
    let path = raw.strip_prefix('@').map(str::to_string).or_else(|| {
        let candidate = std::path::Path::new(raw);
        (!raw.trim_start().starts_with('{') && candidate.is_file()).then(|| raw.to_string())
    });
    let text = match path {
        Some(path) => std::fs::read_to_string(&path).map_err(|error| {
            AikitError::new(
                "praxis.json_unreadable",
                format!("could not read {label} from {path}: {error}"),
            )
        })?,
        None => raw.to_string(),
    };
    serde_json::from_str(&text).map_err(|error| {
        AikitError::new(
            "praxis.json_invalid",
            format!("invalid {label} JSON: {error}"),
        )
    })
}

/// `aikit praxis list [--form F] [FILTER]`.
pub fn list(view: &ResolvedView, form: Option<&str>, filter: Option<&str>) -> Result<Value> {
    let form = match form {
        Some(raw) => Some(PraxisForm::parse(raw).ok_or_else(|| {
            AikitError::new(
                "praxis.form_unknown",
                format!("`{raw}` is not a praxis form; expected skill, method or methodology"),
            )
        })?),
        None => None,
    };
    let filter = filter.map(str::to_lowercase);
    let mut rows: Vec<Value> = view
        .catalog_index
        .values()
        .filter(|entry| entry.kind == Kind::Skill)
        .filter_map(|entry| {
            let entry_form = praxis_form(&entry.description);
            if form.is_some_and(|wanted| wanted != entry_form) {
                return None;
            }
            let payload = praxis_payload(&entry.description);
            if let Some(filter) = &filter {
                if !format!("{} {}", entry.name, payload)
                    .to_lowercase()
                    .contains(filter)
                {
                    return None;
                }
            }
            Some(json!({
                "id": entry.id.to_string(),
                "name": entry.name,
                "form": entry_form.as_str(),
                "position": entry_form.position(),
                "payload": payload,
                "revision": entry.revision.as_ref().map(ToString::to_string),
                "active": view.is_active(&entry.id),
                "declared": view.is_declared_enabled(&entry.id),
            }))
        })
        .collect();
    rows.sort_by(|a, b| {
        (a["position"].as_u64(), a["name"].as_str())
            .cmp(&(b["position"].as_u64(), b["name"].as_str()))
    });
    let mut counts = BTreeMap::new();
    for row in &rows {
        *counts
            .entry(row["form"].as_str().unwrap_or_default().to_string())
            .or_insert(0_u64) += 1;
    }
    Ok(json!({
        "prefixes": {
            "method": aikit_core::method::METHOD_DESCRIPTION_PREFIX,
            "methodology": aikit_core::method::METHODOLOGY_DESCRIPTION_PREFIX,
        },
        "count": rows.len(),
        "counts": counts,
        "praxis": rows,
    }))
}

/// Central action envelopes wrap the profile; accept either shape.
fn unwrap_profile(value: Value) -> Value {
    for pointer in ["/data/profile", "/profile", "/data"] {
        if let Some(inner) = value.pointer(pointer) {
            if inner.get("agent_ref").is_some() {
                return inner.clone();
            }
        }
    }
    value
}

/// `aikit praxis disclose`.
pub fn disclose(
    home: &AikitHome,
    view: &ResolvedView,
    profile_json: &str,
    activity_json: Option<&str>,
    select: &[String],
    now: Option<NowLocationFacts>,
) -> Result<Value> {
    let profile_value = unwrap_profile(read_json(profile_json, "AgentProfile")?);
    let profile: AgentProfileFacts = serde_json::from_value(profile_value).map_err(|error| {
        AikitError::new(
            "praxis.profile_invalid",
            format!("the AgentProfile is not a readable central.agent-profile/v1 record: {error}"),
        )
    })?;
    let activity: Option<PraxisActivity> = match activity_json {
        Some(raw) => Some(
            serde_json::from_value(read_json(raw, "praxis activity")?).map_err(|error| {
                AikitError::new(
                    "praxis.activity_invalid",
                    format!("the activity evidence is not aikit.praxis-activity/v1: {error}"),
                )
            })?,
        ),
        None => None,
    };

    let sets = profile
        .skill_set_refs
        .iter()
        .map(|reference| {
            let origin = if aikit_store::registry_skillsets::is_semantic_ref(reference) {
                "registry"
            } else {
                "home"
            };
            SetReading {
                reference: reference.clone(),
                result: aikit_store::skillsets::load(home, reference)
                    .map_err(|error| error.message().to_string()),
                origin: origin.to_string(),
            }
        })
        .collect::<Vec<_>>();

    let mut wanted: Vec<String> = profile.skill_refs.clone();
    wanted.extend(profile.method_refs.iter().cloned());
    for reading in &sets {
        if let Ok(set) = &reading.result {
            wanted.extend(set.all_members().iter().map(ToString::to_string));
        }
    }
    let mut catalogue = BTreeMap::new();
    for raw in wanted {
        let Ok(id) = CapsuleId::parse(&raw) else {
            continue;
        };
        let Some(entry) = view.catalog_index.get(&id) else {
            continue;
        };
        let unavailable = view.unavailable.get(&id);
        let available = if unavailable.is_some() {
            Some(false)
        } else if view.is_active(&id) || entry.trust == TrustState::Trusted {
            Some(true)
        } else {
            None
        };
        catalogue.insert(
            id.clone(),
            SkillFacts {
                name: entry.name.clone(),
                description: entry.description.clone(),
                revision: entry.revision.as_ref().map(ToString::to_string),
                available,
                projected: Some(view.is_active(&id)),
                withheld_reason: unavailable.map(|reason| reason.describe()),
            },
        );
    }

    let disclosure = disclose_agent_praxis(&DisclosureInput {
        profile,
        sets,
        catalogue,
        activity,
        selected: select.to_vec(),
        context_id: Some(view.context.context_id.to_string()),
        now_location: now,
    });
    serde_json::to_value(disclosure).map_err(|error| {
        AikitError::new(
            "praxis.encode_failed",
            format!("could not encode the disclosure: {error}"),
        )
    })
}

/// `aikit praxis instantiate-check` — Jev's standing invocation-time
/// question, answered from the invocation facts the caller hands over: does a
/// carried Methodology need to be instantiated here, or does the Skill carry
/// enough context? Reads nothing else and loads nothing.
pub fn instantiate_check(invocation_json: &str) -> Result<Value> {
    let facts: SkillInvocationFacts =
        serde_json::from_value(read_json(invocation_json, "skill invocation")?).map_err(|error| {
            AikitError::new(
                "praxis.invocation_invalid",
                format!("the invocation facts do not satisfy aikit.methodology-instantiation/v1 input: {error}"),
            )
        })?;
    let decision = decide_methodology_instantiation(&facts);
    serde_json::to_value(decision).map_err(|error| {
        AikitError::new(
            "praxis.encode_failed",
            format!("could not encode the instantiation decision: {error}"),
        )
    })
}

/// `aikit a2a card`.
pub fn a2a_card(
    participation_json: &str,
    interface_url: &str,
    out: Option<&std::path::Path>,
) -> Result<Value> {
    let participation = read_json(participation_json, "World participation")?;
    let input = projection_input_from_participation(&participation, interface_url)?;
    let card = project_a2a_agent_card(&input)?;
    if let Some(path) = out {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                AikitError::new(
                    "a2a_card.write_failed",
                    format!("could not create {}: {error}", parent.display()),
                )
            })?;
        }
        let text = serde_json::to_string_pretty(&card).unwrap_or_default();
        std::fs::write(path, text).map_err(|error| {
            AikitError::new(
                "a2a_card.write_failed",
                format!("could not write {}: {error}", path.display()),
            )
        })?;
    }
    Ok(json!({
        "spec_version": aikit_core::a2a_card::A2A_CARD_SPEC_VERSION,
        "well_known_path": aikit_core::a2a_card::A2A_WELL_KNOWN_AGENT_CARD_PATH,
        "written_to": out.map(|path| path.display().to_string()),
        "public_skill_count": card["skills"].as_array().map_or(0, Vec::len),
        "card": card,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instantiate_check_returns_the_typed_decision() {
        let raw = serde_json::json!({
            "skill_id": "skill/demo/html-account",
            "skill_description": "Author a self-contained HTML account.",
            "carried_methodologies": [
                {"id": "skill/demo/docs-methodology",
                 "description": "METHODOLOGY: orient the documentation field"}
            ],
            "undertaking_hints": ["write the documentation walk"]
        })
        .to_string();
        let value = instantiate_check(&raw).unwrap();
        assert_eq!(value["schema"], "aikit.methodology-instantiation/v1");
        assert_eq!(value["decisions"][0]["decision"], "instantiate");
        assert_eq!(value["decisions"][0]["reason"], "field-matches-undertaking");
    }

    #[test]
    fn instantiate_check_refuses_unreadable_invocation_facts() {
        let error = instantiate_check("{\"skill_id\":\"x\"}").unwrap_err();
        assert_eq!(error.code(), "praxis.invocation_invalid");
    }
}
