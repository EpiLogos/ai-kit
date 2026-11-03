//! `aikit decide invoke --instrument <id>`: fill the open heads of one
//! instrument of QL's team space (O-I #843; DLW manual §19.1 Vāk, Generate's
//! Fill heads).
//!
//! The instrument is QL's: AIKit reads the team space whole from the installed
//! `ql` (`ql context-frame team-space --json`, `ql.vak-team-space/v1`) and
//! mirrors none of it. A cell names the heads it has chosen (an empty choice
//! is an abstention, kept as given); every other head of the instrument is
//! open. Each open head becomes one typed Choice question among the labels
//! that head holds, asked of the elected decision provider over the cell's
//! own words and chosen heads, and the answers come back as picks with the
//! label's owner, confidence and distribution. A pick is evidence for the
//! engine to validate (`answer-is-evidence`, `engine-validates`): never a
//! receipt, never admitted here. A cell with no open head makes no call
//! (`zero-heads-zero-calls`). An instrument only the person answers is
//! refused, as is a chosen label its head does not hold.
use crate::cli::DecideInvokeArgs;
use crate::decide::{
    asking_session, invoke_selected, keep_ask, DecisionProviderConfig, DecisionProviderMode,
};
use crate::jev_now::{fail, minted_invocation_ref, read_bytes, read_json};
use aikit_adapters::ql_provider::QlCliClient;
use aikit_core::jev::{Answer, JevRequest, Question};
use aikit_core::{ResourceRef, Result};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::io::Read;

pub const INSTRUMENT_FILL_REQUEST_SCHEMA: &str = "aikit.instrument-fill-request/v1";
pub const INSTRUMENT_FILL_SCHEMA: &str = "aikit.instrument-fill/v1";
const TEAM_SPACE_CONTRACT: &str = "ql.vak-team-space/v1";
const MAX_REQUEST_BYTES: usize = 1024 * 1024;

/// What a cell asks: its own words (any object: the unit's key, concern,
/// returns…), and the labels it has chosen per head id.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FillRequest {
    schema: String,
    subject: Map<String, Value>,
    #[serde(default)]
    chosen: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct Cardinality {
    #[serde(default)]
    max: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct Label {
    id: String,
    #[serde(default)]
    description: Value,
    #[serde(default)]
    owner_ref: Value,
    #[serde(default)]
    owner_revision: Value,
}

#[derive(Debug, Deserialize)]
struct Head {
    id: String,
    #[serde(default)]
    field: Value,
    #[serde(default)]
    ambiguity_policy: Value,
    #[serde(default)]
    cardinality: Option<Cardinality>,
    labels: Vec<Label>,
}

#[derive(Debug, Deserialize)]
struct Instrument {
    id: String,
    #[serde(default)]
    organ: Value,
    #[serde(default)]
    kind: Value,
    #[serde(default)]
    source: Value,
    #[serde(default)]
    answerable_by: Option<String>,
    #[serde(default)]
    heads: Vec<Head>,
    #[serde(default)]
    constraints: Value,
}

fn request_bytes(path: &std::path::Path) -> Result<Vec<u8>> {
    if path.as_os_str() == "-" {
        let mut bytes = Vec::new();
        std::io::stdin()
            .take(MAX_REQUEST_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| {
                fail(
                    "decision.instrument_request_invalid",
                    format!("Fill request: {e}"),
                )
            })?;
        if bytes.len() > MAX_REQUEST_BYTES {
            return Err(fail(
                "decision.instrument_request_invalid",
                "Fill request exceeds 1 MiB",
            ));
        }
        return Ok(bytes);
    }
    read_bytes(path, "Fill request", MAX_REQUEST_BYTES)
}

/// The team space as QL answers it, and the one instrument named.
fn instrument(ql: &QlCliClient, id: &str) -> Result<(Value, Instrument)> {
    let space = ql.team_space().map_err(|failure| {
        fail(
            "decision.team_space_unavailable",
            format!(
                "QL's team space could not be read (`{} context-frame team-space --json`): {}: {}",
                ql.executable().display(),
                failure.code,
                failure.message
            ),
        )
    })?;
    if space.get("contract").and_then(Value::as_str) != Some(TEAM_SPACE_CONTRACT) {
        return Err(fail(
            "decision.team_space_unavailable",
            format!(
                "QL's team space answered {}, not {TEAM_SPACE_CONTRACT}",
                space["contract"]
            ),
        ));
    }
    let instruments = space
        .get("instruments")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let held = instruments
        .iter()
        .filter_map(|i| i.get("id").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join(", ");
    let found = instruments
        .into_iter()
        .find(|i| i.get("id").and_then(Value::as_str) == Some(id))
        .ok_or_else(|| {
            fail(
                "decision.instrument_unknown",
                format!("QL's team space holds no instrument {id}; it holds {held}"),
            )
        })?;
    let found: Instrument = serde_json::from_value(found).map_err(|e| {
        fail(
            "decision.team_space_unavailable",
            format!("QL's instrument {id} is not readable: {e}"),
        )
    })?;
    Ok((space, found))
}

fn elected_model(config: &DecisionProviderConfig) -> Result<String> {
    match config.mode {
        DecisionProviderMode::None => Err(crate::decide::provider_disabled_error()),
        DecisionProviderMode::Hosted => config
            .jev_limits
            .as_ref()
            .map(|l| l.tariff.model_version.clone())
            .ok_or_else(|| fail("decision.config_invalid", "hosted requires JevLimits")),
        DecisionProviderMode::ManagedLocal | DecisionProviderMode::Endpoint => {
            Ok(config.limits()?.model.clone())
        }
    }
}

fn label_word(label: &Label) -> Value {
    json!({"label": label.id, "description": label.description, "owner_ref": label.owner_ref, "owner_revision": label.owner_revision})
}

/// `aikit decide invoke --instrument <id> --request-file <fill.json|->`.
pub fn fill(args: DecideInvokeArgs) -> Result<Value> {
    let id = args.instrument.clone().unwrap_or_default();
    let request: FillRequest = serde_json::from_slice(&request_bytes(&args.request_file)?)
        .map_err(|e| {
            fail(
                "decision.instrument_request_invalid",
                format!("Fill request: {e}; send {INSTRUMENT_FILL_REQUEST_SCHEMA}"),
            )
        })?;
    if request.schema != INSTRUMENT_FILL_REQUEST_SCHEMA {
        return Err(fail(
            "decision.instrument_request_invalid",
            format!(
                "The fill request names {}, not {INSTRUMENT_FILL_REQUEST_SCHEMA}",
                request.schema
            ),
        ));
    }
    let ql = QlCliClient::new(args.ql.clone().unwrap_or_else(|| "ql".into()));
    let (space, instrument) = instrument(&ql, &id)?;
    if instrument.answerable_by.as_deref() == Some("person") {
        return Err(fail(
            "decision.instrument_person_only",
            format!("{id} is answered by the person only; no decision model is asked it"),
        ));
    }
    let heads = instrument
        .heads
        .iter()
        .map(|h| (h.id.as_str(), h))
        .collect::<BTreeMap<_, _>>();
    let mut chosen = Map::new();
    for (head_id, labels) in &request.chosen {
        let head = heads.get(head_id.as_str()).ok_or_else(|| {
            fail(
                "decision.instrument_chosen_invalid",
                format!(
                    "{id} holds no head {head_id}; its heads are {}",
                    heads.keys().cloned().collect::<Vec<_>>().join(", ")
                ),
            )
        })?;
        let max = head
            .cardinality
            .as_ref()
            .and_then(|c| c.max)
            .unwrap_or(usize::MAX);
        if labels.len() > max {
            return Err(fail(
                "decision.instrument_chosen_invalid",
                format!(
                    "{head_id} holds at most {max} label(s); the cell chose {}",
                    labels.len()
                ),
            ));
        }
        let mut words = Vec::new();
        for chosen_label in labels {
            let label = head
                .labels
                .iter()
                .find(|l| &l.id == chosen_label)
                .ok_or_else(|| {
                    fail(
                        "decision.instrument_chosen_invalid",
                        format!(
                            "{head_id} has no label {chosen_label}; its labels are {}",
                            head.labels
                                .iter()
                                .map(|l| l.id.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    )
                })?;
            words.push(label_word(label));
        }
        chosen.insert(head_id.clone(), Value::Array(words));
    }
    let open = instrument
        .heads
        .iter()
        .filter(|h| !request.chosen.contains_key(&h.id))
        .collect::<Vec<_>>();
    let rules = space.get("rules").cloned().unwrap_or(Value::Null);
    let mut reading = json!({
        "schema": INSTRUMENT_FILL_SCHEMA,
        "instrument": {"id": instrument.id, "organ": instrument.organ, "kind": instrument.kind, "source": instrument.source},
        "team_space": TEAM_SPACE_CONTRACT,
        "subject": request.subject,
        "chosen": request.chosen,
        "open": open.iter().map(|h| &h.id).collect::<Vec<_>>(),
        "rules": rules,
        "standing": "evidence",
        "engine": "QL admits or refuses the block (`ql vak compose`); the owner of the composition checks it when it is accepted.",
    });
    if open.is_empty() {
        reading["outcome"] = json!("nothing-open");
        reading["calls"] = json!(0);
        reading["picks"] = json!({});
        return Ok(reading);
    }
    let home = aikit_store::AikitHome::discover()?;
    let asking = asking_session(args.agent_session.as_deref())?;
    let provider_file = crate::local_services_election::resolve_file(
        &home,
        crate::local_services_election::Election::Decision,
        args.provider_file.clone(),
    )?;
    let config: DecisionProviderConfig =
        read_json(&provider_file, "Decision provider config", 256 * 1024)?;
    let questions = open
        .iter()
        .map(|head| {
            let criteria = head.labels.iter().map(|l| (l.id.clone(), l.description.clone())).collect::<BTreeMap<_, _>>();
            (
                head.id.clone(),
                Question::Choice {
                    instructions: json!({
                        "question": format!("Which {} label does this cell take, among the labels its head holds?", head.field.as_str().unwrap_or(&head.id)),
                        "instrument": instrument.id, "head": head.id, "field": head.field,
                        "ambiguity_policy": head.ambiguity_policy,
                    }),
                    criteria,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let jev = JevRequest {
        model: elected_model(&config)?,
        state: json!({"instrument": reading["instrument"], "subject": reading["subject"], "chosen": chosen,
            "constraints": instrument.constraints}),
        questions,
    };
    jev.validate()?;
    let invocation_ref = args
        .invocation_ref
        .as_deref()
        .map(ResourceRef::parse)
        .transpose()?
        .unwrap_or(minted_invocation_ref(&jev)?);
    let mut no_revalidate = || -> Result<()> { Ok(()) };
    let receipt = invoke_selected(
        &config,
        &jev,
        invocation_ref.clone(),
        args.curl.clone(),
        args.allow_env_import,
        &mut no_revalidate,
    );
    if let Some((session_id, agent_session)) = &asking {
        keep_ask(
            &home,
            session_id,
            agent_session.as_ref(),
            &jev,
            &invocation_ref,
            receipt.as_ref(),
        )?;
    }
    let receipt = receipt?;
    reading["invocation_ref"] = json!(invocation_ref.as_str());
    reading["calls"] = json!(1);
    match receipt.answer().filter(|_| receipt.outcome_completed()) {
        Some(answer) => {
            let mut picks = Map::new();
            for head in &open {
                let pick = match answer.answers.get(&head.id) {
                    Some(Answer::Choice {
                        choice,
                        probabilities,
                        confidence,
                    }) => {
                        let label = head.labels.iter().find(|l| &l.id == choice);
                        json!({"field": head.field, "label": choice, "description": label.map(|l| &l.description),
                            "owner_ref": label.map(|l| &l.owner_ref), "owner_revision": label.map(|l| &l.owner_revision),
                            "confidence": confidence, "probabilities": probabilities})
                    }
                    _ => {
                        json!({"field": head.field, "label": null, "why": "The decision model gave this head no choice: it stays open."})
                    }
                };
                picks.insert(head.id.clone(), pick);
            }
            reading["outcome"] = json!("completed");
            reading["answered_by"] = json!(answer.model);
            reading["usage"] = serde_json::to_value(&answer.usage).unwrap_or(Value::Null);
            reading["picks"] = Value::Object(picks);
        }
        None => {
            reading["outcome"] = json!("failed");
            reading["why"] = json!(receipt
                .failure_message()
                .unwrap_or_else(|| "The decision provider returned no answer".into()));
            reading["picks"] = json!({});
        }
    }
    Ok(reading)
}
