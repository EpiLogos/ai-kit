//! Communiques: Position-addressed, attributed contact kept in the gateway's
//! own journal.
//!
//! A Communique is the cheap contact plane of World inhabitation
//! (`aikit.communique/v1`): one occupant of a stable World Position addresses
//! another Position, the record is appended durably, and it is delivered at the
//! recipient occupant's next turn boundary. It never waits for a reply, never
//! mints Run ancestry and never becomes obligation-bearing work on its own —
//! escalation into Factory custody is a separate, explicit crossing whose
//! custody ref is recorded here after the owner answered.
//!
//! Why this lives in the gateway kernel rather than beside it: the gateway is
//! already the durable contact plane and already persists its semantic state
//! (`GatewaySnapshot`). Keeping Communiques in that same journal means a
//! conversation is a journal read, a restart restores them with every other
//! gateway fact, and a remote Workcell receives them through the gateway's
//! existing authenticated carrier — there is no second message store.
//!
//! What the kernel refuses to decide: who the sender is (attribution is
//! resolved by the caller from Actuation's occupancy ledger and stored with its
//! plain-words basis), whether a Position exists (Central's), who currently
//! occupies it (Actuation's) and whether a delegation is authorised (Factory's).
//! It records those answers; it never invents them.

use std::collections::BTreeMap;

use aikit_core::{AikitError, Result};
use serde::{Deserialize, Serialize};

pub const COMMUNIQUE_SCHEMA: &str = "aikit.communique/v1";
/// Refs are opaque but namespaced so a Communique can never be mistaken for a
/// Run, custody or AgentSession identity.
pub const COMMUNIQUE_REF_PREFIX: &str = "aikit:communique:";
/// Bodies are contact, not payload transport; a larger artefact is a ref.
pub const MAX_COMMUNIQUE_BODY_BYTES: usize = 64 * 1024;

/// Delivery state. `held` = the recipient Position was vacant when the
/// Communique was accepted; it is delivered to the next occupant that claims
/// the Position. `pending` = an occupant exists (or occupancy could not be
/// read) and delivery waits for that occupant's next turn boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CommuniqueState {
    Held,
    Pending,
    Delivered,
    Escalated,
}

impl CommuniqueState {
    pub fn is_undelivered(self) -> bool {
        matches!(self, Self::Held | Self::Pending)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Held => "held",
            Self::Pending => "pending",
            Self::Delivered => "delivered",
            Self::Escalated => "escalated",
        }
    }
}

/// How the sender identity was established. Attribution is always derived
/// from the sender's own occupancy (never from the body); an unattributable
/// sender is still accepted and labelled `unknown` rather than refused, so a
/// message is never lost to an identity gap (OpenRig P18).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SenderAttribution {
    /// Position and occupant generation were verified current by Actuation.
    Verified,
    /// A Position was named but its occupancy could not be verified.
    Claimed,
    /// No Position could be resolved for the sender.
    Unknown,
}

impl SenderAttribution {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Claimed => "claimed",
            Self::Unknown => "unknown",
        }
    }
}

/// Cross-Workcell relay state of a Communique whose recipient occupant stands
/// on another Workcell. `forwarded` means the remote gateway ingested it; from
/// then on delivery is that gateway's to record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum CommuniqueForward {
    Queued {
        workcell_ref: String,
        attempts: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        last_error: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        last_attempt_at_unix_ms: Option<u64>,
    },
    Forwarded {
        workcell_ref: String,
        remote_gateway_ref: String,
        forwarded_at_unix_ms: u64,
        attempts: u32,
    },
}

impl CommuniqueForward {
    fn attempts(&self) -> u32 {
        match self {
            Self::Queued { attempts, .. } | Self::Forwarded { attempts, .. } => *attempts,
        }
    }
}

/// Why a Communique was routed to another Workcell when this Workcell's own
/// occupancy ledger had no current occupant for the recipient: the remote
/// gateway that reported one, the Workcell it serves, and the generation it
/// named. Recorded once the route is taken, so a relay can always be traced
/// back to the occupancy answer that justified it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommuniqueRouting {
    pub workcell_ref: String,
    pub gateway_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation_ref: Option<String>,
    /// Plain words: what was asked, of whom, and what it answered.
    pub basis: String,
    pub observed_at_unix_ms: u64,
}

/// One state change, appended in order. The journal never rewrites a
/// transition; the record's `state` is always the last entry's state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommuniqueTransition {
    pub at_unix_ms: u64,
    pub state: CommuniqueState,
    pub basis: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation_ref: Option<String>,
}

/// `aikit.communique/v1`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Communique {
    pub schema: String,
    pub communique_ref: String,
    /// Order within this gateway's journal (not a global clock).
    pub sequence: u64,
    #[serde(default)]
    pub from_position_ref: Option<String>,
    #[serde(default)]
    pub from_generation_ref: Option<String>,
    pub attribution: SenderAttribution,
    pub attribution_basis: String,
    pub to_position_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_workcell_ref: Option<String>,
    pub body: String,
    pub sent_at_unix_ms: u64,
    pub state: CommuniqueState,
    #[serde(default)]
    pub delivered_to_generation_ref: Option<String>,
    #[serde(default)]
    pub delivered_at_unix_ms: Option<u64>,
    #[serde(default)]
    pub escalated_custody_ref: Option<String>,
    #[serde(default)]
    pub reply_to: Option<String>,
    /// The gateway that accepted the Communique from its sender.
    pub origin_gateway_ref: String,
    /// Set on a remote gateway: the gateway that relayed it here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub received_from_gateway_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forward: Option<CommuniqueForward>,
    /// Set when the route came from another Workcell's occupancy answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing: Option<CommuniqueRouting>,
    #[serde(default)]
    pub transitions: Vec<CommuniqueTransition>,
}

impl Communique {
    /// Undelivered and still this gateway's to deliver (not relayed away).
    pub fn awaits_local_delivery(&self) -> bool {
        self.state.is_undelivered()
            && !matches!(self.forward, Some(CommuniqueForward::Forwarded { .. }))
    }

    /// The sender identity as it is attributed — never taken from the body.
    pub fn sender_label(&self) -> String {
        match (self.attribution, self.from_position_ref.as_deref()) {
            (SenderAttribution::Unknown, _) | (_, None) => "<unknown sender>".into(),
            (SenderAttribution::Verified, Some(position)) => position.to_owned(),
            (SenderAttribution::Claimed, Some(position)) => {
                format!("{position} (claimed, not verified)")
            }
        }
    }
}

/// What a sender's gateway is asked to accept. The caller has already
/// resolved the recipient Position (Central), its occupancy (Actuation) and
/// the sender's attribution; the kernel checks the draft's internal
/// consistency and appends it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommuniqueDraft {
    pub communique_ref: String,
    #[serde(default)]
    pub from_position_ref: Option<String>,
    #[serde(default)]
    pub from_generation_ref: Option<String>,
    pub attribution: SenderAttribution,
    pub attribution_basis: String,
    pub to_position_ref: String,
    #[serde(default)]
    pub to_workcell_ref: Option<String>,
    pub body: String,
    pub sent_at_unix_ms: u64,
    pub state: CommuniqueState,
    pub state_basis: String,
    #[serde(default)]
    pub reply_to: Option<String>,
    /// Queue for relay to this remote Workcell's gateway.
    #[serde(default)]
    pub forward_to_workcell_ref: Option<String>,
    /// The remote occupancy answer the route was taken on, if any.
    #[serde(default)]
    pub routing: Option<CommuniqueRouting>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "kebab-case")]
pub enum CommuniqueForwardOutcome {
    Forwarded {
        workcell_ref: String,
        remote_gateway_ref: String,
        at_unix_ms: u64,
    },
    Failed {
        workcell_ref: String,
        error: String,
        at_unix_ms: u64,
    },
}

/// Undelivered counts for one Position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommuniqueCount {
    pub position_ref: String,
    pub undelivered: usize,
    pub held: usize,
    pub pending: usize,
}

/// The append-only Communique journal a gateway keeps.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommuniqueJournal {
    records: Vec<Communique>,
    index: BTreeMap<String, usize>,
}

fn invalid(code: &'static str, message: impl Into<String>) -> AikitError {
    AikitError::new(code, message.into())
}

fn non_empty(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() || value != value.trim() || value.contains('\0') {
        return Err(invalid(
            "agency_gateway.communique_invalid",
            format!("Communique {label} must be a non-empty trimmed string"),
        ));
    }
    Ok(())
}

fn check_attribution(
    attribution: SenderAttribution,
    from_position: Option<&str>,
    from_generation: Option<&str>,
) -> Result<()> {
    let consistent = match attribution {
        SenderAttribution::Verified => from_position.is_some() && from_generation.is_some(),
        SenderAttribution::Claimed => from_position.is_some(),
        SenderAttribution::Unknown => from_position.is_none() && from_generation.is_none(),
    };
    if !consistent {
        return Err(invalid(
            "agency_gateway.communique_attribution_inconsistent",
            format!(
                "a {} attribution does not match the sender refs it carries",
                attribution.as_str()
            ),
        ));
    }
    Ok(())
}

fn check_body(body: &str) -> Result<()> {
    if body.trim().is_empty() {
        return Err(invalid(
            "agency_gateway.communique_empty_body",
            "a Communique body must not be empty",
        ));
    }
    if body.len() > MAX_COMMUNIQUE_BODY_BYTES {
        return Err(invalid(
            "agency_gateway.communique_body_too_large",
            format!(
                "a Communique body is {} bytes; the limit is {MAX_COMMUNIQUE_BODY_BYTES} (pass a ref to a larger artefact)",
                body.len()
            ),
        ));
    }
    Ok(())
}

impl CommuniqueJournal {
    pub fn records(&self) -> &[Communique] {
        &self.records
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    fn next_sequence(&self) -> u64 {
        self.records
            .last()
            .map(|record| record.sequence + 1)
            .unwrap_or(1)
    }

    pub fn get(&self, communique_ref: &str) -> Result<&Communique> {
        self.index
            .get(communique_ref)
            .map(|index| &self.records[*index])
            .ok_or_else(|| {
                invalid(
                    "agency_gateway.unknown_communique",
                    format!("this gateway's journal holds no Communique {communique_ref}"),
                )
            })
    }

    fn get_mut(&mut self, communique_ref: &str) -> Result<&mut Communique> {
        match self.index.get(communique_ref) {
            Some(index) => Ok(&mut self.records[*index]),
            None => Err(invalid(
                "agency_gateway.unknown_communique",
                format!("this gateway's journal holds no Communique {communique_ref}"),
            )),
        }
    }

    fn push(&mut self, record: Communique) {
        self.index
            .insert(record.communique_ref.clone(), self.records.len());
        self.records.push(record);
    }

    /// Rebuild from persisted records, refusing any journal whose order or
    /// identity was damaged.
    pub fn restore(records: Vec<Communique>) -> Result<Self> {
        let mut journal = Self::default();
        let mut previous = 0;
        for record in records {
            if record.schema != COMMUNIQUE_SCHEMA {
                return Err(invalid(
                    "agency_gateway.communique_schema",
                    format!(
                        "Communique {} has schema {}, not {COMMUNIQUE_SCHEMA}",
                        record.communique_ref, record.schema
                    ),
                ));
            }
            if record.sequence <= previous {
                return Err(invalid(
                    "agency_gateway.communique_sequence_drift",
                    format!(
                        "Communique {} sequence {} does not follow {previous}",
                        record.communique_ref, record.sequence
                    ),
                ));
            }
            if journal.index.contains_key(&record.communique_ref) {
                return Err(invalid(
                    "agency_gateway.duplicate_communique",
                    format!("journal repeats Communique {}", record.communique_ref),
                ));
            }
            if record.transitions.last().map(|last| last.state) != Some(record.state) {
                return Err(invalid(
                    "agency_gateway.communique_state_drift",
                    format!(
                        "Communique {} state {} is not its last recorded transition",
                        record.communique_ref,
                        record.state.as_str()
                    ),
                ));
            }
            previous = record.sequence;
            journal.push(record);
        }
        Ok(journal)
    }

    /// Accept a sender's draft. Replaying the identical draft answers the
    /// existing record (`replayed == true`); a different draft under the same
    /// ref is an identity rewrite and refused.
    pub fn send(
        &mut self,
        gateway_ref: &str,
        draft: CommuniqueDraft,
    ) -> Result<(Communique, bool)> {
        non_empty("communique_ref", &draft.communique_ref)?;
        if !draft.communique_ref.starts_with(COMMUNIQUE_REF_PREFIX) {
            return Err(invalid(
                "agency_gateway.communique_invalid",
                format!("Communique refs begin with {COMMUNIQUE_REF_PREFIX}"),
            ));
        }
        non_empty("to_position_ref", &draft.to_position_ref)?;
        non_empty("attribution_basis", &draft.attribution_basis)?;
        non_empty("state_basis", &draft.state_basis)?;
        check_body(&draft.body)?;
        check_attribution(
            draft.attribution,
            draft.from_position_ref.as_deref(),
            draft.from_generation_ref.as_deref(),
        )?;
        if !draft.state.is_undelivered() {
            return Err(invalid(
                "agency_gateway.communique_invalid",
                "a Communique is accepted held or pending; delivery and escalation are later transitions",
            ));
        }
        if let Some(reply_to) = &draft.reply_to {
            self.get(reply_to).map_err(|_| {
                invalid(
                    "agency_gateway.communique_unknown_reply",
                    format!(
                        "reply_to names {reply_to}, which this gateway's journal does not hold"
                    ),
                )
            })?;
        }
        if let Some(existing) = self.index.get(&draft.communique_ref) {
            let existing = &self.records[*existing];
            let same = existing.from_position_ref == draft.from_position_ref
                && existing.from_generation_ref == draft.from_generation_ref
                && existing.to_position_ref == draft.to_position_ref
                && existing.body == draft.body
                && existing.sent_at_unix_ms == draft.sent_at_unix_ms
                && existing.reply_to == draft.reply_to;
            if same {
                return Ok((existing.clone(), true));
            }
            return Err(invalid(
                "agency_gateway.communique_identity_rewrite",
                format!(
                    "Communique ref {} already names a different message",
                    draft.communique_ref
                ),
            ));
        }
        let forward =
            draft
                .forward_to_workcell_ref
                .clone()
                .map(|workcell_ref| CommuniqueForward::Queued {
                    workcell_ref,
                    attempts: 0,
                    last_error: None,
                    last_attempt_at_unix_ms: None,
                });
        let record = Communique {
            schema: COMMUNIQUE_SCHEMA.into(),
            communique_ref: draft.communique_ref,
            sequence: self.next_sequence(),
            from_position_ref: draft.from_position_ref,
            from_generation_ref: draft.from_generation_ref,
            attribution: draft.attribution,
            attribution_basis: draft.attribution_basis,
            to_position_ref: draft.to_position_ref,
            to_workcell_ref: draft.to_workcell_ref,
            body: draft.body,
            sent_at_unix_ms: draft.sent_at_unix_ms,
            state: draft.state,
            delivered_to_generation_ref: None,
            delivered_at_unix_ms: None,
            escalated_custody_ref: None,
            reply_to: draft.reply_to,
            origin_gateway_ref: gateway_ref.into(),
            received_from_gateway_ref: None,
            forward,
            routing: draft.routing,
            transitions: vec![CommuniqueTransition {
                at_unix_ms: draft.sent_at_unix_ms,
                state: draft.state,
                basis: draft.state_basis,
                generation_ref: None,
            }],
        };
        self.push(record.clone());
        Ok((record, false))
    }

    /// Accept a Communique relayed by another Workcell's gateway. The record
    /// keeps its identity, sender attribution and origin; this journal gives it
    /// a local sequence and names the relaying gateway.
    pub fn ingest(
        &mut self,
        mut communique: Communique,
        relayed_by: &str,
        at_unix_ms: u64,
    ) -> Result<(Communique, bool)> {
        non_empty("relayed_by", relayed_by)?;
        if communique.schema != COMMUNIQUE_SCHEMA {
            return Err(invalid(
                "agency_gateway.communique_schema",
                format!("relayed record has schema {}", communique.schema),
            ));
        }
        non_empty("communique_ref", &communique.communique_ref)?;
        non_empty("to_position_ref", &communique.to_position_ref)?;
        check_body(&communique.body)?;
        check_attribution(
            communique.attribution,
            communique.from_position_ref.as_deref(),
            communique.from_generation_ref.as_deref(),
        )?;
        if !communique.state.is_undelivered() {
            return Err(invalid(
                "agency_gateway.communique_invalid",
                "only an undelivered Communique can be relayed",
            ));
        }
        if let Some(existing) = self.index.get(&communique.communique_ref) {
            let existing = &self.records[*existing];
            if existing.body == communique.body
                && existing.to_position_ref == communique.to_position_ref
                && existing.from_position_ref == communique.from_position_ref
                && existing.origin_gateway_ref == communique.origin_gateway_ref
            {
                return Ok((existing.clone(), true));
            }
            return Err(invalid(
                "agency_gateway.communique_identity_rewrite",
                format!(
                    "Communique ref {} already names a different message here",
                    communique.communique_ref
                ),
            ));
        }
        communique.sequence = self.next_sequence();
        communique.received_from_gateway_ref = Some(relayed_by.into());
        communique.forward = None;
        communique.transitions.push(CommuniqueTransition {
            at_unix_ms,
            state: communique.state,
            basis: format!("received from gateway {relayed_by}"),
            generation_ref: None,
        });
        self.push(communique.clone());
        Ok((communique, false))
    }

    /// Every Communique this gateway still has to deliver to the occupant of
    /// `position_ref`, in journal order.
    pub fn inbox(&self, position_ref: &str) -> Vec<Communique> {
        self.records
            .iter()
            .filter(|record| record.to_position_ref == position_ref)
            .filter(|record| record.awaits_local_delivery())
            .cloned()
            .collect()
    }

    /// Mark Communiques delivered to one occupant generation. All refs are
    /// checked before any is changed, so a partial acknowledgement never lands.
    pub fn acknowledge(
        &mut self,
        position_ref: &str,
        generation_ref: &str,
        communique_refs: &[String],
        delivered_at_unix_ms: u64,
        via: &str,
    ) -> Result<Vec<Communique>> {
        non_empty("generation_ref", generation_ref)?;
        non_empty("via", via)?;
        for communique_ref in communique_refs {
            let record = self.get(communique_ref)?;
            if record.to_position_ref != position_ref {
                return Err(invalid(
                    "agency_gateway.communique_wrong_recipient",
                    format!(
                        "Communique {communique_ref} is addressed to {}, not {position_ref}",
                        record.to_position_ref
                    ),
                ));
            }
            if !record.awaits_local_delivery() {
                return Err(invalid(
                    "agency_gateway.communique_not_deliverable",
                    format!(
                        "Communique {communique_ref} is {}{}; it cannot be delivered here again",
                        record.state.as_str(),
                        if matches!(record.forward, Some(CommuniqueForward::Forwarded { .. })) {
                            " and was forwarded to another Workcell"
                        } else {
                            ""
                        }
                    ),
                ));
            }
        }
        let mut delivered = Vec::new();
        for communique_ref in communique_refs {
            let record = self.get_mut(communique_ref)?;
            record.state = CommuniqueState::Delivered;
            record.delivered_to_generation_ref = Some(generation_ref.into());
            record.delivered_at_unix_ms = Some(delivered_at_unix_ms);
            // Delivered here: any queued relay is moot.
            if matches!(record.forward, Some(CommuniqueForward::Queued { .. })) {
                record.forward = None;
            }
            record.transitions.push(CommuniqueTransition {
                at_unix_ms: delivered_at_unix_ms,
                state: CommuniqueState::Delivered,
                basis: format!("delivered {via}"),
                generation_ref: Some(generation_ref.into()),
            });
            delivered.push(record.clone());
        }
        Ok(delivered)
    }

    /// Both directions between two Positions, in journal order.
    pub fn conversation(&self, position_ref: &str, with_position_ref: &str) -> Vec<Communique> {
        self.records
            .iter()
            .filter(|record| {
                let from = record.from_position_ref.as_deref();
                (from == Some(position_ref) && record.to_position_ref == with_position_ref)
                    || (from == Some(with_position_ref) && record.to_position_ref == position_ref)
            })
            .cloned()
            .collect()
    }

    /// Record the explicit crossing into Factory custody. The custody ref is
    /// the owner's answer; recording it twice with the same ref is a replay.
    pub fn escalate(
        &mut self,
        communique_ref: &str,
        custody_ref: &str,
        at_unix_ms: u64,
        basis: &str,
    ) -> Result<(Communique, bool)> {
        non_empty("custody_ref", custody_ref)?;
        non_empty("basis", basis)?;
        let record = self.get_mut(communique_ref)?;
        if record.state == CommuniqueState::Escalated {
            if record.escalated_custody_ref.as_deref() == Some(custody_ref) {
                return Ok((record.clone(), true));
            }
            return Err(invalid(
                "agency_gateway.communique_already_escalated",
                format!(
                    "Communique {communique_ref} was already escalated into {}",
                    record.escalated_custody_ref.as_deref().unwrap_or("custody")
                ),
            ));
        }
        record.state = CommuniqueState::Escalated;
        record.escalated_custody_ref = Some(custody_ref.into());
        record.transitions.push(CommuniqueTransition {
            at_unix_ms,
            state: CommuniqueState::Escalated,
            basis: basis.into(),
            generation_ref: None,
        });
        Ok((record.clone(), false))
    }

    /// Undelivered counts per recipient Position (uncapped).
    pub fn counts(&self) -> Vec<CommuniqueCount> {
        let mut counts: BTreeMap<&str, CommuniqueCount> = BTreeMap::new();
        for record in self.records.iter().filter(|r| r.awaits_local_delivery()) {
            let entry = counts
                .entry(record.to_position_ref.as_str())
                .or_insert_with(|| CommuniqueCount {
                    position_ref: record.to_position_ref.clone(),
                    undelivered: 0,
                    held: 0,
                    pending: 0,
                });
            entry.undelivered += 1;
            match record.state {
                CommuniqueState::Held => entry.held += 1,
                CommuniqueState::Pending => entry.pending += 1,
                CommuniqueState::Delivered | CommuniqueState::Escalated => {}
            }
        }
        counts.into_values().collect()
    }

    /// Every Communique a relay pass must re-evaluate: undelivered here and
    /// not yet relayed. The caller reads current occupancy and decides.
    pub fn forward_queue(&self) -> Vec<Communique> {
        self.records
            .iter()
            .filter(|record| record.awaits_local_delivery())
            .cloned()
            .collect()
    }

    /// Record one relay attempt. `routing` names the remote occupancy answer
    /// the attempt was made on, when the route came from one (a Communique
    /// re-resolved by a relay pass); it replaces any earlier routing, because
    /// the latest answer is the one the relay followed.
    pub fn record_forward(
        &mut self,
        communique_ref: &str,
        outcome: CommuniqueForwardOutcome,
        routing: Option<CommuniqueRouting>,
    ) -> Result<Communique> {
        let record = self.get_mut(communique_ref)?;
        if !record.awaits_local_delivery() {
            return Err(invalid(
                "agency_gateway.communique_not_deliverable",
                format!(
                    "Communique {communique_ref} is {}; there is nothing left to relay",
                    record.state.as_str()
                ),
            ));
        }
        let attempts = record.forward.as_ref().map(|f| f.attempts()).unwrap_or(0) + 1;
        let routed_on = routing
            .as_ref()
            .map(|routing| format!(" (routed on: {})", routing.basis))
            .unwrap_or_default();
        if routing.is_some() {
            record.routing = routing;
        }
        match outcome {
            CommuniqueForwardOutcome::Forwarded {
                workcell_ref,
                remote_gateway_ref,
                at_unix_ms,
            } => {
                record.to_workcell_ref = Some(workcell_ref.clone());
                record.transitions.push(CommuniqueTransition {
                    at_unix_ms,
                    state: record.state,
                    basis: format!(
                        "relayed to Workcell {workcell_ref} through gateway {remote_gateway_ref}; delivery is recorded there{routed_on}"
                    ),
                    generation_ref: None,
                });
                record.forward = Some(CommuniqueForward::Forwarded {
                    workcell_ref,
                    remote_gateway_ref,
                    forwarded_at_unix_ms: at_unix_ms,
                    attempts,
                });
            }
            CommuniqueForwardOutcome::Failed {
                workcell_ref,
                error,
                at_unix_ms,
            } => {
                record.to_workcell_ref = Some(workcell_ref.clone());
                record.forward = Some(CommuniqueForward::Queued {
                    workcell_ref,
                    attempts,
                    last_error: Some(error),
                    last_attempt_at_unix_ms: Some(at_unix_ms),
                });
            }
        }
        Ok(record.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "central:position:project:O-I:factory-guardian";
    const B: &str = "central:position:project:O-I:cradle-steward";

    fn draft(
        reference: &str,
        from: Option<&str>,
        to: &str,
        state: CommuniqueState,
    ) -> CommuniqueDraft {
        CommuniqueDraft {
            communique_ref: format!("{COMMUNIQUE_REF_PREFIX}{reference}"),
            from_position_ref: from.map(str::to_owned),
            from_generation_ref: from.map(|_| "actuation:generation:g1".to_owned()),
            attribution: if from.is_some() {
                SenderAttribution::Verified
            } else {
                SenderAttribution::Unknown
            },
            attribution_basis: "fixture".into(),
            to_position_ref: to.into(),
            to_workcell_ref: None,
            body: format!("body {reference}"),
            sent_at_unix_ms: 10,
            state,
            state_basis: "fixture".into(),
            reply_to: None,
            forward_to_workcell_ref: None,
            routing: None,
        }
    }

    #[test]
    fn a_sent_communique_waits_in_the_recipient_inbox_until_acknowledged_by_one_generation() {
        let mut journal = CommuniqueJournal::default();
        let (sent, replayed) = journal
            .send(
                "agency-gateway/a",
                draft("one", Some(A), B, CommuniqueState::Pending),
            )
            .unwrap();
        assert!(!replayed);
        assert_eq!(sent.sequence, 1);
        assert_eq!(journal.inbox(B).len(), 1);
        assert!(journal.inbox(A).is_empty());
        let delivered = journal
            .acknowledge(
                B,
                "actuation:generation:b1",
                std::slice::from_ref(&sent.communique_ref),
                20,
                "at the turn boundary",
            )
            .unwrap();
        assert_eq!(delivered[0].state, CommuniqueState::Delivered);
        assert_eq!(
            delivered[0].delivered_to_generation_ref.as_deref(),
            Some("actuation:generation:b1")
        );
        assert!(journal.inbox(B).is_empty());
        let again = journal.acknowledge(
            B,
            "actuation:generation:b2",
            std::slice::from_ref(&sent.communique_ref),
            30,
            "again",
        );
        assert_eq!(
            again.unwrap_err().code(),
            "agency_gateway.communique_not_deliverable"
        );
        // The earlier delivery stays attributed to the generation that received it.
        assert_eq!(
            journal
                .get(&sent.communique_ref)
                .unwrap()
                .delivered_to_generation_ref
                .as_deref(),
            Some("actuation:generation:b1")
        );
    }

    #[test]
    fn a_partial_acknowledgement_changes_nothing() {
        let mut journal = CommuniqueJournal::default();
        let (first, _) = journal
            .send("g", draft("one", Some(A), B, CommuniqueState::Pending))
            .unwrap();
        let (other, _) = journal
            .send("g", draft("two", Some(B), A, CommuniqueState::Pending))
            .unwrap();
        let refused = journal.acknowledge(
            B,
            "actuation:generation:b1",
            &[first.communique_ref.clone(), other.communique_ref.clone()],
            5,
            "turn",
        );
        assert_eq!(
            refused.unwrap_err().code(),
            "agency_gateway.communique_wrong_recipient"
        );
        assert_eq!(
            journal.get(&first.communique_ref).unwrap().state,
            CommuniqueState::Pending
        );
    }

    #[test]
    fn a_replayed_send_answers_the_record_and_a_rewrite_is_refused() {
        let mut journal = CommuniqueJournal::default();
        journal
            .send("g", draft("one", Some(A), B, CommuniqueState::Held))
            .unwrap();
        let (_, replayed) = journal
            .send("g", draft("one", Some(A), B, CommuniqueState::Held))
            .unwrap();
        assert!(replayed);
        let mut rewrite = draft("one", Some(A), B, CommuniqueState::Held);
        rewrite.body = "a different message".into();
        assert_eq!(
            journal.send("g", rewrite).unwrap_err().code(),
            "agency_gateway.communique_identity_rewrite"
        );
        assert_eq!(journal.len(), 1);
    }

    #[test]
    fn attribution_must_match_the_refs_it_claims() {
        let mut journal = CommuniqueJournal::default();
        let mut forged = draft("one", None, B, CommuniqueState::Pending);
        forged.attribution = SenderAttribution::Verified;
        assert_eq!(
            journal.send("g", forged).unwrap_err().code(),
            "agency_gateway.communique_attribution_inconsistent"
        );
        let unknown = draft("two", None, B, CommuniqueState::Pending);
        let (record, _) = journal.send("g", unknown).unwrap();
        assert_eq!(record.sender_label(), "<unknown sender>");
    }

    #[test]
    fn conversation_reads_both_directions_in_journal_order() {
        let mut journal = CommuniqueJournal::default();
        journal
            .send("g", draft("one", Some(A), B, CommuniqueState::Pending))
            .unwrap();
        journal
            .send(
                "g",
                draft(
                    "noise",
                    Some(A),
                    "central:position:control:root:other",
                    CommuniqueState::Pending,
                ),
            )
            .unwrap();
        let mut reply = draft("two", Some(B), A, CommuniqueState::Pending);
        reply.reply_to = Some(format!("{COMMUNIQUE_REF_PREFIX}one"));
        journal.send("g", reply).unwrap();
        let thread = journal.conversation(A, B);
        assert_eq!(
            thread
                .iter()
                .map(|r| r.communique_ref.as_str())
                .collect::<Vec<_>>(),
            vec!["aikit:communique:one", "aikit:communique:two"]
        );
        assert_eq!(journal.conversation(B, A), thread);
    }

    #[test]
    fn a_forwarded_communique_leaves_the_local_inbox_and_counts() {
        let mut journal = CommuniqueJournal::default();
        let mut remote = draft("one", Some(A), B, CommuniqueState::Pending);
        remote.forward_to_workcell_ref = Some("workcell:omarchy".into());
        let (record, _) = journal.send("g", remote).unwrap();
        assert_eq!(journal.counts()[0].undelivered, 1);
        let failed = journal
            .record_forward(
                &record.communique_ref,
                CommuniqueForwardOutcome::Failed {
                    workcell_ref: "workcell:omarchy".into(),
                    error: "connection refused".into(),
                    at_unix_ms: 11,
                },
                None,
            )
            .unwrap();
        assert!(matches!(
            failed.forward,
            Some(CommuniqueForward::Queued { attempts: 1, .. })
        ));
        let forwarded = journal
            .record_forward(
                &record.communique_ref,
                CommuniqueForwardOutcome::Forwarded {
                    workcell_ref: "workcell:omarchy".into(),
                    remote_gateway_ref: "agency-gateway/omarchy".into(),
                    at_unix_ms: 12,
                },
                None,
            )
            .unwrap();
        assert!(matches!(
            forwarded.forward,
            Some(CommuniqueForward::Forwarded { attempts: 2, .. })
        ));
        assert!(journal.inbox(B).is_empty());
        assert!(journal.counts().is_empty());
        assert_eq!(forwarded.state, CommuniqueState::Pending);
    }

    #[test]
    fn a_relay_taken_on_a_remote_occupancy_answer_records_that_answer() {
        let mut journal = CommuniqueJournal::default();
        let (held, _) = journal
            .send("g", draft("one", Some(A), B, CommuniqueState::Held))
            .unwrap();
        assert!(held.routing.is_none());
        let routing = CommuniqueRouting {
            workcell_ref: "workcell:omarchy".into(),
            gateway_ref: "agency-gateway/omarchy".into(),
            generation_ref: Some("actuation:generation:b1".into()),
            basis: "vacant here; agency-gateway/omarchy reports actuation:generation:b1".into(),
            observed_at_unix_ms: 11,
        };
        let relayed = journal
            .record_forward(
                &held.communique_ref,
                CommuniqueForwardOutcome::Forwarded {
                    workcell_ref: "workcell:omarchy".into(),
                    remote_gateway_ref: "agency-gateway/omarchy".into(),
                    at_unix_ms: 12,
                },
                Some(routing.clone()),
            )
            .unwrap();
        assert_eq!(relayed.routing, Some(routing));
        assert_eq!(relayed.state, CommuniqueState::Held);
        assert!(relayed
            .transitions
            .last()
            .unwrap()
            .basis
            .contains("routed on: vacant here"));
        assert!(journal.inbox(B).is_empty());
        // The record, routing included, survives a restore.
        let restored = CommuniqueJournal::restore(journal.records().to_vec()).unwrap();
        assert_eq!(
            restored.get(&held.communique_ref).unwrap().routing,
            relayed.routing
        );
    }

    #[test]
    fn a_relayed_record_keeps_identity_and_attribution_on_the_receiving_gateway() {
        let mut origin = CommuniqueJournal::default();
        let (sent, _) = origin
            .send(
                "agency-gateway/mac",
                draft("one", Some(A), B, CommuniqueState::Pending),
            )
            .unwrap();
        let mut remote = CommuniqueJournal::default();
        let (received, replayed) = remote
            .ingest(sent.clone(), "agency-gateway/mac", 50)
            .unwrap();
        assert!(!replayed);
        assert_eq!(received.communique_ref, sent.communique_ref);
        assert_eq!(received.from_position_ref.as_deref(), Some(A));
        assert_eq!(received.origin_gateway_ref, "agency-gateway/mac");
        assert_eq!(
            received.received_from_gateway_ref.as_deref(),
            Some("agency-gateway/mac")
        );
        assert_eq!(remote.inbox(B).len(), 1);
        let (_, replayed) = remote.ingest(sent, "agency-gateway/mac", 60).unwrap();
        assert!(replayed);
        assert_eq!(remote.len(), 1);
    }

    #[test]
    fn escalation_records_the_custody_answer_once() {
        let mut journal = CommuniqueJournal::default();
        let (sent, _) = journal
            .send("g", draft("one", Some(A), B, CommuniqueState::Pending))
            .unwrap();
        let (escalated, replayed) = journal
            .escalate(&sent.communique_ref, "factory:custody:1", 9, "delegated")
            .unwrap();
        assert!(!replayed);
        assert_eq!(escalated.state, CommuniqueState::Escalated);
        assert!(journal.inbox(B).is_empty());
        assert!(
            journal
                .escalate(&sent.communique_ref, "factory:custody:1", 9, "again")
                .unwrap()
                .1
        );
        assert_eq!(
            journal
                .escalate(&sent.communique_ref, "factory:custody:2", 9, "other")
                .unwrap_err()
                .code(),
            "agency_gateway.communique_already_escalated"
        );
    }

    #[test]
    fn a_restored_journal_refuses_a_state_that_is_not_its_last_transition() {
        let mut journal = CommuniqueJournal::default();
        let (mut record, _) = journal
            .send("g", draft("one", Some(A), B, CommuniqueState::Pending))
            .unwrap();
        assert!(CommuniqueJournal::restore(vec![record.clone()]).is_ok());
        record.state = CommuniqueState::Delivered;
        assert_eq!(
            CommuniqueJournal::restore(vec![record]).unwrap_err().code(),
            "agency_gateway.communique_state_drift"
        );
    }
}
