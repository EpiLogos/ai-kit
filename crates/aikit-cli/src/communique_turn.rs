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

use crate::gateway_contact::{now_unix_ms, GatewayAccess, GENERATION_ENV, POSITION_ENV};
use crate::gateway_owners::{ContactOwners, OccupancyVerdict};

/// The occupant a turn belongs to, verified current by Actuation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnOccupant {
    pub position_ref: String,
    pub generation_ref: String,
}

/// What one turn will carry and, once written, acknowledge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnDelivery {
    pub occupant: TurnOccupant,
    pub communique_refs: Vec<String>,
    pub text: String,
}

/// The block's first line; the commit checks the written document for it.
const BLOCK_HEAD: &str = "[gateway/communiques]";

static STAGED: Mutex<Option<TurnDelivery>> = Mutex::new(None);

/// The turn text for `occupant`, or `None` when nothing waits.
pub fn pending_communiques_for_turn(
    occupant: &TurnOccupant,
    gateway: &dyn GatewayAccess,
) -> Result<Option<TurnDelivery>> {
    let records = match gateway.call(GatewayCommand::CommuniqueInbox {
        position_ref: occupant.position_ref.clone(),
    })? {
        GatewayResponse::CommuniqueList { communiques } => communiques,
        _ => return Ok(None),
    };
    if records.is_empty() {
        return Ok(None);
    }
    Ok(Some(TurnDelivery {
        occupant: occupant.clone(),
        communique_refs: records.iter().map(|r| r.communique_ref.clone()).collect(),
        text: render(occupant, &records),
    }))
}

fn render(occupant: &TurnOccupant, records: &[Communique]) -> String {
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
        if record.state == aikit_adapters::CommuniqueState::Held {
            out.push_str("   Held while the Position was vacant; you are its next occupant.\n");
        }
        for line in record.body.lines() {
            out.push_str("   | ");
            out.push_str(line);
            out.push('\n');
        }
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
        Ok(OccupancyVerdict::Current(_)) => Ok(Some(TurnOccupant {
            position_ref: position,
            generation_ref: generation,
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
    // against the escaped form of the block's first line.
    let head = delivery.text.lines().next().unwrap_or(BLOCK_HEAD);
    let escaped = serde_json::to_string(head).unwrap_or_default();
    let escaped = escaped.trim_matches('"');
    if !written.contains(escaped) {
        return Ok(false);
    }
    gateway.call(GatewayCommand::AcknowledgeCommuniques {
        position_ref: delivery.occupant.position_ref.clone(),
        generation_ref: delivery.occupant.generation_ref.clone(),
        communique_refs: delivery.communique_refs.clone(),
        delivered_at_unix_ms: now_unix_ms(),
        via: "at the occupant's turn boundary".into(),
    })?;
    Ok(true)
}
