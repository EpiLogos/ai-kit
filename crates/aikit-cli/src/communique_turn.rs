//! Communique delivery at the recipient occupant's turn boundary.
//!
//! The delivery moment is the occupant's next prompt (`UserPromptSubmit`):
//! the undelivered Communiques addressed to its Position ride that turn's
//! context, and only then are they marked delivered — to exactly the
//! generation that received them. Two steps keep that claim honest:
//!
//! 1. [`offer_at_turn_boundary`] (called from the hook dispatcher) verifies
//!    this body is the Position's *current* occupant, reads the inbox, adds the
//!    rendered block to the hook decision and stages the delivery. Nothing is
//!    marked yet.
//! 2. [`commit_staged_delivery`] (called by the hook command after the harness
//!    document is written) marks the staged Communiques delivered, but only if
//!    the written document actually carried the block. A denied prompt, a
//!    harness with no context channel, or a `--json` inspection delivers
//!    nothing and marks nothing.
//!
//! A body that cannot be verified current (superseded, or Actuation
//! unavailable) is never delivered to: its Communiques stay undelivered for
//! the generation that does hold the address. Bodies are quoted line by line
//! so text inside a body can never pose as the envelope around it.

use std::sync::Mutex;

use aikit_adapters::{Communique, GatewayCommand, GatewayResponse};
use aikit_core::hooks::{HookDecision, HookEvent, HookEventKind};
use aikit_core::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::gateway_contact::{now_unix_ms, GatewayAccess, GENERATION_ENV, POSITION_ENV};
use crate::gateway_owners::{ContactOwners, OccupancyVerdict};

/// The occupant a turn belongs to, verified current by Actuation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnOccupant {
    pub position_ref: String,
    pub generation_ref: String,
    /// Where the verified generation stands (its tenure's Workcell, else this
    /// home's), when known; an exact-instance Communique that requires a
    /// Workcell is carried only when this matches.
    pub workcell_ref: Option<String>,
}

/// What one turn will carry and, once written, acknowledge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnDelivery {
    pub occupant: TurnOccupant,
    pub communique_refs: Vec<String>,
    pub text: String,
    pub pending_count: usize,
    pub preview_bytes: usize,
}

/// One aggregate budget, including attribution, retrieval commands and JSON
/// escaping. Token accounting is deliberately conservative: a UTF-8 byte is
/// an upper bound for the ordinary byte-fallback tokenisers used by harnesses.
#[derive(Debug, Clone, Copy)]
pub struct HandoffBudget {
    pub records: usize,
    pub serialized_bytes: usize,
    pub token_upper_bound: usize,
    pub preview_bytes: usize,
}

impl Default for HandoffBudget {
    fn default() -> Self {
        Self {
            records: 8,
            serialized_bytes: 16_384,
            token_upper_bound: 8_192,
            preview_bytes: 512,
        }
    }
}

/// The block's first line; the commit checks the written document for it.
const BLOCK_HEAD: &str = "[gateway/communiques]";

static STAGED: Mutex<Option<TurnDelivery>> = Mutex::new(None);

/// The turn text for `occupant`, or `None` when nothing waits.
pub fn pending_communiques_for_turn(
    occupant: &TurnOccupant,
    gateway: &dyn GatewayAccess,
) -> Result<Option<TurnDelivery>> {
    pending_communiques_with_budget(occupant, gateway, HandoffBudget::default())
}

pub fn pending_communiques_with_budget(
    occupant: &TurnOccupant,
    gateway: &dyn GatewayAccess,
    budget: HandoffBudget,
) -> Result<Option<TurnDelivery>> {
    let records = match gateway.call(GatewayCommand::CommuniqueInbox {
        position_ref: occupant.position_ref.clone(),
    })? {
        GatewayResponse::CommuniqueList { communiques } => communiques,
        _ => return Ok(None),
    };
    // A durable Position route is every occupant's; an exact-instance route
    // is only its own generation's, on its required Workcell.
    let records: Vec<Communique> = records
        .into_iter()
        .filter(|record| {
            record.deliverable_to(&occupant.generation_ref, occupant.workcell_ref.as_deref())
        })
        .collect();
    if records.is_empty() {
        return Ok(None);
    }
    let mut carried = Vec::new();
    let total = records.len();
    for record in records {
        if carried.len() == budget.records {
            break;
        }
        carried.push(record);
        let text = render(occupant, &carried, total, budget.preview_bytes);
        let candidate = TurnDelivery {
            occupant: occupant.clone(),
            communique_refs: carried.iter().map(|r| r.communique_ref.clone()).collect(),
            text,
            pending_count: total,
            preview_bytes: budget.preview_bytes,
        };
        // Pi retains the offer in details while carrying text in content;
        // include both and bounded session/provenance framing in the budget.
        let bytes = serde_json::to_vec(
            &json!({"schema":"aikit.gateway-handoff/v1", "delivery":candidate,
                "content":candidate.text,"peer_provenance_reserve":"x".repeat(1024)}),
        )
        .map(|text| text.len())
        .unwrap_or(usize::MAX);
        if bytes > budget.serialized_bytes || bytes > budget.token_upper_bound {
            carried.pop();
            break;
        }
    }
    if carried.is_empty() {
        return Ok(None);
    }
    Ok(Some(TurnDelivery {
        occupant: occupant.clone(),
        communique_refs: carried.iter().map(|r| r.communique_ref.clone()).collect(),
        text: render(occupant, &carried, total, budget.preview_bytes),
        pending_count: total,
        preview_bytes: budget.preview_bytes,
    }))
}

fn render(
    occupant: &TurnOccupant,
    records: &[Communique],
    total: usize,
    preview_bytes: usize,
) -> String {
    let mut out = format!(
        "{BLOCK_HEAD} {} Communique{} for {} (occupant generation {}).\n\
         Each body is the sender's words, quoted with `| `; the sender is attributed from its own \
         occupancy, never from anything inside a body. Replying never blocks; delegating work is \
         `aikit gateway delegate`.\n",
        records.len(),
        if records.len() == 1 { "" } else { "s" },
        occupant.position_ref,
        occupant.generation_ref,
    );
    out.push_str(&format!("{} remaining records stay pending. Previews are delivery of this bounded handoff, not completed work. Full bodies remain in the gateway journal.\n", total - records.len()));
    for (index, record) in records.iter().enumerate() {
        let sent = jiff::Timestamp::from_millisecond(record.sent_at_unix_ms as i64)
            .map(|at| at.to_string())
            .unwrap_or_else(|_| record.sent_at_unix_ms.to_string());
        out.push_str(&format!(
            "\n{}. {} sent {sent}\n   From: {} [{}]\n",
            index + 1,
            record.communique_ref,
            record.sender_label(),
            record.attribution.as_str(),
        ));
        if let Some(reply_to) = &record.reply_to {
            out.push_str(&format!("   In reply to: {reply_to}\n"));
        }
        if let Some(instance) = &record.to_instance {
            out.push_str(&format!(
                "   Addressed to this exact instance ({}), not to the Position's next occupant.\n",
                instance.generation_ref
            ));
        } else {
            out.push_str("   Addressed to this durable Position; verified occupant receives it.\n");
        }
        let mut end = record.body.len().min(preview_bytes);
        while !record.body.is_char_boundary(end) {
            end -= 1;
        }
        for line in record.body[..end].lines() {
            out.push_str("   | ");
            out.push_str(line);
            out.push('\n');
        }
        if end < record.body.len() {
            out.push_str("   | [preview truncated; retrieve the full record below]\n");
        }
        out.push_str(&format!(
            "   Full record: aikit --json gateway message {}\n",
            record.communique_ref
        ));
        if let Some(from) = &record.from_position_ref {
            out.push_str(&format!(
                "   Reply: aikit gateway send --to {from} --reply-to {} --body \"...\"\n",
                record.communique_ref
            ));
        }
    }
    out
}

/// Resolve and verify this body's occupancy from its launch environment.
/// `Ok(None)` = not an inhabited body (no Position in the environment).
pub fn turn_occupant(
    owners: &dyn ContactOwners,
) -> std::result::Result<Option<TurnOccupant>, String> {
    let position = std::env::var(POSITION_ENV)
        .ok()
        .filter(|v| !v.trim().is_empty());
    let Some(position) = position else {
        return Ok(None);
    };
    let Some(generation) = std::env::var(GENERATION_ENV)
        .ok()
        .filter(|v| !v.trim().is_empty())
    else {
        return Err(format!(
            "Communiques for {position} were not delivered: this body carries {POSITION_ENV} but no {GENERATION_ENV}, so its occupancy cannot be verified"
        ));
    };
    match owners.occupancy_verify(&position, &generation) {
        Ok(OccupancyVerdict::Current(tenure)) => Ok(Some(TurnOccupant {
            position_ref: position,
            generation_ref: generation,
            workcell_ref: crate::gateway_contact::tenure_workcell(&tenure).or_else(|| {
                let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
                crate::gateway_contact::local_workcell(owners, &cwd).0
            }),
        })),
        Ok(OccupancyVerdict::Refused(refusal)) => Err(format!(
            "Communiques for {position} were not delivered to this body: Actuation refuses generation {generation} ({}: {}); they wait for the current occupant",
            refusal.code, refusal.fact
        )),
        Err(unavailable) => Err(format!(
            "Communiques for {position} were not delivered: occupancy could not be verified ({unavailable}); they stay undelivered"
        )),
    }
}

/// Hook-dispatcher step (one call site in `hook::dispatch`). Adds the block
/// to the decision and stages the delivery; never fails the hook.
pub fn offer_at_turn_boundary(decision: &mut HookDecision, event: &HookEvent) {
    if event.kind != HookEventKind::UserPromptSubmit {
        return;
    }
    let owners = crate::gateway_owners::ProcessOwners::from_env();
    let occupant = match turn_occupant(&owners) {
        Ok(Some(occupant)) => occupant,
        Ok(None) => return,
        Err(warning) => {
            decision.warnings.push(warning);
            return;
        }
    };
    let home = match aikit_store::AikitHome::discover() {
        Ok(home) => home,
        Err(error) => {
            decision
                .warnings
                .push(format!("Communiques were not read: {error}"));
            return;
        }
    };
    let gateway = crate::gateway_contact::LocalGateway::default_for(&home);
    match pending_communiques_for_turn(&occupant, &gateway) {
        Ok(Some(delivery)) => {
            decision.injected.push(delivery.text.clone());
            if let Ok(mut staged) = STAGED.lock() {
                *staged = Some(delivery);
            }
        }
        Ok(None) => {}
        Err(error) => decision.warnings.push(format!(
            "Communiques for {} were not read: {error}",
            occupant.position_ref
        )),
    }
}

/// Take whatever [`offer_at_turn_boundary`] staged in this process.
pub fn take_staged_delivery() -> Option<TurnDelivery> {
    STAGED.lock().ok().and_then(|mut staged| staged.take())
}

/// Mark a staged delivery delivered — only when `written` (the document that
/// actually reached the harness) carries its block.
pub fn commit_staged_delivery(
    delivery: &TurnDelivery,
    written: &str,
    gateway: &dyn GatewayAccess,
) -> Result<bool> {
    // The harness document is JSON; the block rides it escaped, so compare
    // against the escaped form of the complete block. A header alone cannot
    // prove that the harness received every record being acknowledged.
    let escaped = serde_json::to_string(&delivery.text).unwrap_or_default();
    let escaped = escaped.trim_matches('"');
    if !written.contains(escaped) {
        return Ok(false);
    }
    gateway.call(GatewayCommand::AcknowledgeCommuniques {
        position_ref: delivery.occupant.position_ref.clone(),
        generation_ref: delivery.occupant.generation_ref.clone(),
        workcell_ref: delivery.occupant.workcell_ref.clone(),
        communique_refs: delivery.communique_refs.clone(),
        delivered_at_unix_ms: now_unix_ms(),
        via: "at the occupant's turn boundary".into(),
    })?;
    Ok(true)
}

/// Pi's native extension uses the same journal and attribution as the turn
/// hook. Offering is read-only; committing follows a successfully persisted
/// peer message and includes its exact carried text. An uncertain extension
/// restart reconciles the retained peer message before starting another turn.
pub fn handoff_current(
    owners: &dyn ContactOwners,
    gateway: &dyn GatewayAccess,
    commit: Option<Value>,
) -> Result<Value> {
    let occupant = turn_occupant(owners)
        .map_err(|message| {
            aikit_core::AikitError::new("gateway.handoff_occupancy_unverified", message)
        })?
        .ok_or_else(|| {
            aikit_core::AikitError::new(
                "gateway.handoff_not_inhabited",
                "A handoff requires this body's current Position and generation",
            )
        })?;
    handoff_for_occupant(&occupant, gateway, commit)
}

pub fn handoff_for_occupant(
    occupant: &TurnOccupant,
    gateway: &dyn GatewayAccess,
    commit: Option<Value>,
) -> Result<Value> {
    let Some(commit) = commit else {
        return Ok(
            json!({"schema":"aikit.gateway-handoff/v1", "delivery":pending_communiques_for_turn(occupant, gateway)?}),
        );
    };
    let delivery: TurnDelivery =
        serde_json::from_value(commit["delivery"].clone()).map_err(|error| {
            aikit_core::AikitError::new("gateway.handoff_invalid", error.to_string())
        })?;
    if &delivery.occupant != occupant
        || commit["carried_text"].as_str() != Some(delivery.text.as_str())
    {
        return Err(aikit_core::AikitError::new(
            "gateway.handoff_basis_changed",
            "The retained handoff must carry the exact current occupant and complete offered text",
        ));
    }
    let bounds = HandoffBudget::default();
    if delivery.communique_refs.len() > bounds.records
        || delivery.preview_bytes > bounds.preview_bytes
        || serde_json::to_vec(&json!({"delivery":delivery,"content":delivery.text,"peer_provenance_reserve":"x".repeat(1024)}))
            .map(|value| value.len()).unwrap_or(usize::MAX) > bounds.serialized_bytes
    {
        return Err(aikit_core::AikitError::new("gateway.handoff_invalid", "Carried handoff exceeds the native aggregate budget"));
    }
    let mut records = Vec::new();
    let mut pending = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for reference in &delivery.communique_refs {
        if !seen.insert(reference) {
            return Err(aikit_core::AikitError::new(
                "gateway.handoff_invalid",
                "A handoff repeats a record reference",
            ));
        }
        let record = match gateway.call(GatewayCommand::ReadCommunique {
            communique_ref: reference.clone(),
        })? {
            GatewayResponse::CommuniqueRecord { communique } => communique,
            _ => {
                return Err(aikit_core::AikitError::new(
                    "gateway.handoff_unreadable",
                    "The exact retained record could not be read",
                ))
            }
        };
        if record.to_position_ref != occupant.position_ref {
            return Err(aikit_core::AikitError::new(
                "gateway.handoff_wrong_recipient",
                "A carried record belongs to another Position",
            ));
        }
        if record.awaits_local_delivery()
            && record.deliverable_to(&occupant.generation_ref, occupant.workcell_ref.as_deref())
        {
            pending.push(reference.clone());
        } else if record.state != aikit_adapters::CommuniqueState::Delivered
            || record.delivered_to_generation_ref.as_deref() != Some(&occupant.generation_ref)
            || !record.to_instance.as_ref().is_none_or(|instance| {
                instance.generation_ref == occupant.generation_ref
                    && instance
                        .required_workcell_ref
                        .as_deref()
                        .is_none_or(|required| occupant.workcell_ref.as_deref() == Some(required))
            })
        {
            return Err(aikit_core::AikitError::new(
                "gateway.handoff_basis_changed",
                "A carried record is no longer deliverable to this exact generation and Workcell",
            ));
        }
        records.push(record);
    }
    if records.is_empty()
        || delivery.pending_count < records.len()
        || render(
            occupant,
            &records,
            delivery.pending_count,
            delivery.preview_bytes,
        ) != delivery.text
    {
        return Err(aikit_core::AikitError::new(
            "gateway.handoff_basis_changed",
            "The retained handoff does not match the exact journal records",
        ));
    }
    if !pending.is_empty() {
        gateway.call(GatewayCommand::AcknowledgeCommuniques {
            position_ref: occupant.position_ref.clone(),
            generation_ref: occupant.generation_ref.clone(),
            workcell_ref: occupant.workcell_ref.clone(),
            communique_refs: pending.clone(),
            delivered_at_unix_ms: now_unix_ms(),
            via: "Pi's retained native peer turn".into(),
        })?;
    }
    Ok(
        json!({"schema":"aikit.gateway-handoff/v1", "occupant":occupant, "acknowledged":pending, "reconciled":delivery.communique_refs, "completed_work":false}),
    )
}

/// Focused full retrieval/export is scoped to the actual current occupant,
/// not to possession of an arbitrary journal reference.
pub fn message_current(
    owners: &dyn ContactOwners,
    gateway: &dyn GatewayAccess,
    reference: &str,
) -> Result<Value> {
    let occupant = turn_occupant(owners)
        .map_err(|message| {
            aikit_core::AikitError::new("gateway.handoff_occupancy_unverified", message)
        })?
        .ok_or_else(|| {
            aikit_core::AikitError::new(
                "gateway.handoff_not_inhabited",
                "Full retrieval requires this body's verified occupancy",
            )
        })?;
    let mut reading = message_for_occupant(&occupant, gateway, reference)?;
    let record: Communique =
        serde_json::from_value(reading["communique"].clone()).map_err(|error| {
            aikit_core::AikitError::new("gateway.handoff_unreadable", error.to_string())
        })?;
    if record.from_position_ref.as_deref() == Some(&occupant.position_ref)
        && record.from_generation_ref.as_deref() == Some(&occupant.generation_ref)
    {
        let home = aikit_store::AikitHome::discover()?;
        reading["remote_delivery"] = match crate::gateway_contact::forwarded_readback(
            &home, &record,
        ) {
            Ok(Some(observed)) => observed,
            Ok(None) => Value::Null,
            Err(error) => {
                json!({"state":"unreadable","error":error.to_string(),"next_action":"Read the recorded accepting gateway again; ingestion is not completed work"})
            }
        };
    }
    Ok(reading)
}

pub fn message_for_occupant(
    occupant: &TurnOccupant,
    gateway: &dyn GatewayAccess,
    reference: &str,
) -> Result<Value> {
    let record = match gateway.call(GatewayCommand::ReadCommunique {
        communique_ref: reference.into(),
    })? {
        GatewayResponse::CommuniqueRecord { communique } => communique,
        _ => {
            return Err(aikit_core::AikitError::new(
                "gateway.handoff_unreadable",
                "The exact record could not be read",
            ))
        }
    };
    let recipient = record.to_position_ref == occupant.position_ref
        && ((record.awaits_local_delivery()
            && record.deliverable_to(&occupant.generation_ref, occupant.workcell_ref.as_deref()))
            || (record.delivered_to_generation_ref.as_deref()
                == Some(occupant.generation_ref.as_str())
                && record.to_instance.as_ref().is_none_or(|instance| {
                    instance.generation_ref == occupant.generation_ref
                        && instance
                            .required_workcell_ref
                            .as_deref()
                            .is_none_or(|required| {
                                occupant.workcell_ref.as_deref() == Some(required)
                            })
                })));
    let sender = record.from_position_ref.as_deref() == Some(occupant.position_ref.as_str())
        && record.from_generation_ref.as_deref() == Some(occupant.generation_ref.as_str());
    if !recipient && !sender {
        return Err(aikit_core::AikitError::new(
            "gateway.message_not_addressed",
            "This exact message is outside the current occupant's sender/recipient relation",
        ));
    }
    Ok(json!({"schema":"aikit.gateway-message/v1", "occupant":occupant, "communique":record}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway_contact::LocalGateway;
    use aikit_adapters::{CommuniqueDraft, CommuniqueState, SenderAttribution};
    use aikit_store::AikitHome;
    fn occupant() -> TurnOccupant {
        TurnOccupant {
            position_ref: "central:position:control:root:recipient".into(),
            generation_ref: "actuation:generation:recipient".into(),
            workcell_ref: Some("workcell:test".into()),
        }
    }
    fn seed(gateway: &LocalGateway, count: usize) {
        for index in 0..count {
            gateway
                .call(GatewayCommand::SendCommunique {
                    draft: Box::new(CommuniqueDraft {
                        communique_ref: format!("aikit:communique:budget-{index:03}"),
                        from_position_ref: Some("central:position:control:root:sender".into()),
                        from_generation_ref: Some("actuation:generation:sender".into()),
                        attribution: SenderAttribution::Verified,
                        attribution_basis: "verified sender occupancy".into(),
                        to_position_ref: occupant().position_ref,
                        to_workcell_ref: None,
                        to_instance: None,
                        instance_hold: None,
                        body: format!(
                            "artifact:exact-{index}\n{}TAIL-{index}",
                            "😀quoted\"\\\n".repeat(200)
                        ),
                        sent_at_unix_ms: 100 + index as u64,
                        state: CommuniqueState::Pending,
                        state_basis: "current".into(),
                        reply_to: None,
                        forward_to_workcell_ref: None,
                        routing: None,
                    }),
                })
                .unwrap();
        }
    }
    #[test]
    fn aggregate_budget_preserves_full_material_and_reconciles_ack_after_restart() {
        let dir = tempfile::tempdir().unwrap();
        let home = AikitHome::at(dir.path());
        let gateway = LocalGateway::default_for(&home);
        seed(&gateway, 20);
        let delivery = pending_communiques_with_budget(
            &occupant(),
            &gateway,
            HandoffBudget {
                records: 3,
                serialized_bytes: 6000,
                token_upper_bound: 6000,
                preview_bytes: 256,
            },
        )
        .unwrap()
        .unwrap();
        assert!(!delivery.communique_refs.is_empty());
        assert!(delivery.communique_refs.len() <= 3);
        let actual = json!({"customType":"aikit-native-peer-handoff","content":delivery.text,"display":true,"details":{"schema":"aikit.pi-peer-handoff/v1","recipient_session_id":"real-session","authority":"peer-only","delivery":delivery}});
        let payload_bytes = serde_json::to_vec(&actual).unwrap().len();
        let source_bytes = (0..20)
            .map(|index| {
                format!(
                    "artifact:exact-{index}\n{}TAIL-{index}",
                    "😀quoted\"\\\n".repeat(200)
                )
                .len()
            })
            .sum::<usize>();
        eprintln!(
            "{}",
            json!({"schema":"aikit.handoff-measurement/v1","source_records":20,"source_body_bytes":source_bytes,"carried_records":delivery.communique_refs.len(),"carried_json_bytes":payload_bytes,"byte_token_upper_bound":payload_bytes,"pending_records":20-delivery.communique_refs.len(),"manual_recovery_actions":0})
        );
        assert!(serde_json::to_vec(&actual).unwrap().len() <= 6000);
        assert!(delivery.text.contains("preview truncated"));
        assert!(!delivery.text.contains("TAIL-0"));
        let reference = &delivery.communique_refs[0];
        let full = message_for_occupant(&occupant(), &gateway, reference).unwrap();
        assert!(full["communique"]["body"]
            .as_str()
            .unwrap()
            .ends_with("TAIL-0"));
        let commit = json!({"delivery":delivery,"carried_text":delivery.text});
        let confirmed = handoff_for_occupant(&occupant(), &gateway, Some(commit.clone())).unwrap();
        assert_eq!(
            confirmed["acknowledged"].as_array().unwrap().len(),
            delivery.communique_refs.len()
        );
        let restarted = LocalGateway::default_for(&home);
        let replay = handoff_for_occupant(&occupant(), &restarted, Some(commit)).unwrap();
        assert_eq!(replay["acknowledged"], json!([]));
        let GatewayResponse::CommuniqueList { communiques } = restarted
            .call(GatewayCommand::CommuniqueInbox {
                position_ref: occupant().position_ref,
            })
            .unwrap()
        else {
            panic!("inbox")
        };
        assert_eq!(communiques.len(), 20 - delivery.communique_refs.len());
        let mut wrong = occupant();
        wrong.position_ref = "central:position:control:root:foreign".into();
        assert_eq!(
            message_for_occupant(&wrong, &restarted, reference)
                .unwrap_err()
                .code(),
            "gateway.message_not_addressed"
        );
        assert!(
            message_for_occupant(&occupant(), &restarted, reference).unwrap()["communique"]["body"]
                .as_str()
                .unwrap()
                .ends_with("TAIL-0")
        );
    }
    #[test]
    fn header_or_interrupted_output_never_acknowledges_carried_batch() {
        let dir = tempfile::tempdir().unwrap();
        let gateway = LocalGateway::default_for(&AikitHome::at(dir.path()));
        seed(&gateway, 2);
        let delivery = pending_communiques_for_turn(&occupant(), &gateway)
            .unwrap()
            .unwrap();
        let header = serde_json::to_string(delivery.text.lines().next().unwrap()).unwrap();
        assert!(!commit_staged_delivery(&delivery, &header, &gateway).unwrap());
        let mut end = delivery.text.len() / 2;
        while !delivery.text.is_char_boundary(end) {
            end -= 1;
        }
        let interrupted = serde_json::to_string(&delivery.text[..end]).unwrap();
        assert!(!commit_staged_delivery(&delivery, &interrupted, &gateway).unwrap());
        let mut tampered = delivery.clone();
        tampered.text.push_str("foreign body");
        assert!(handoff_for_occupant(
            &occupant(),
            &gateway,
            Some(json!({"delivery":tampered,"carried_text":tampered.text}))
        )
        .is_err());
        let GatewayResponse::CommuniqueList { communiques } = gateway
            .call(GatewayCommand::CommuniqueInbox {
                position_ref: occupant().position_ref,
            })
            .unwrap()
        else {
            panic!("inbox")
        };
        assert_eq!(communiques.len(), 2);
        assert!(commit_staged_delivery(
            &delivery,
            &serde_json::to_string(&delivery.text).unwrap(),
            &gateway
        )
        .unwrap());
    }
    #[test]
    fn exact_generation_and_workcell_gate_pending_and_delivered_full_retrieval() {
        let dir = tempfile::tempdir().unwrap();
        let gateway = LocalGateway::default_for(&AikitHome::at(dir.path()));
        gateway
            .call(GatewayCommand::SendCommunique {
                draft: Box::new(CommuniqueDraft {
                    communique_ref: "aikit:communique:exact-retrieval".into(),
                    from_position_ref: None,
                    from_generation_ref: None,
                    attribution: SenderAttribution::Unknown,
                    attribution_basis: "No sender inference".into(),
                    to_position_ref: occupant().position_ref,
                    to_workcell_ref: None,
                    to_instance: Some(aikit_adapters::CommuniqueInstance {
                        generation_ref: occupant().generation_ref,
                        required_workcell_ref: occupant().workcell_ref,
                        agent_session_ref: None,
                        agency_ref: None,
                    }),
                    instance_hold: None,
                    body: "Private exact-generation material".into(),
                    sent_at_unix_ms: 1,
                    state: CommuniqueState::Pending,
                    state_basis: "Exact recipient".into(),
                    reply_to: None,
                    forward_to_workcell_ref: None,
                    routing: None,
                }),
            })
            .unwrap();
        let mut wrong = occupant();
        wrong.workcell_ref = Some("workcell:wrong".into());
        assert!(pending_communiques_for_turn(&wrong, &gateway)
            .unwrap()
            .is_none());
        assert_eq!(
            message_for_occupant(&wrong, &gateway, "aikit:communique:exact-retrieval")
                .unwrap_err()
                .code(),
            "gateway.message_not_addressed"
        );
        wrong = occupant();
        wrong.generation_ref = "actuation:generation:successor".into();
        assert!(pending_communiques_for_turn(&wrong, &gateway)
            .unwrap()
            .is_none());
        assert!(
            message_for_occupant(&wrong, &gateway, "aikit:communique:exact-retrieval").is_err()
        );
        let delivery = pending_communiques_for_turn(&occupant(), &gateway)
            .unwrap()
            .unwrap();
        let commit = json!({"delivery":delivery,"carried_text":delivery.text});
        handoff_for_occupant(&occupant(), &gateway, Some(commit.clone())).unwrap();
        wrong = occupant();
        wrong.workcell_ref = Some("workcell:wrong".into());
        assert!(
            message_for_occupant(&wrong, &gateway, "aikit:communique:exact-retrieval").is_err()
        );
        assert!(handoff_for_occupant(&wrong, &gateway, Some(commit)).is_err());
        assert!(
            message_for_occupant(&occupant(), &gateway, "aikit:communique:exact-retrieval").is_ok()
        );
    }
}
