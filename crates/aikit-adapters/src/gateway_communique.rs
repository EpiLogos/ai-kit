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
    /// The person themself: a decision recorded by Central receiving under
    /// their authenticated human review, carried back to the asking Position.
    /// It names no Position or generation — the owner occupies none.
    Owner,
}

impl SenderAttribution {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Claimed => "claimed",
            Self::Unknown => "unknown",
            Self::Owner => "owner",
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

/// An exact-instance binding on a Communique's target. A Communique without
/// one is a durable Position route: it follows succession and is delivered to
/// whichever generation occupies the Position when it is delivered. A
/// Communique with one is addressed to one occupancy generation (Actuation's
/// `generation_ref`, e.g. `actuation:generation:<id>`) and, when
/// `required_workcell_ref` is set, only while that generation stands on that
/// Workcell. It is never delivered to a successor, to a same-named occupant on
/// another Workcell, nor to the right generation on the wrong Workcell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommuniqueInstance {
    pub generation_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_workcell_ref: Option<String>,
    /// The AgentSession Actuation's tenure named for that generation when the
    /// sender resolved it. Informative: delivery is decided on the generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_session_ref: Option<String>,
    /// The Agency Actuation's tenure named for that generation, likewise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agency_ref: Option<String>,
}

/// Why an exact-instance Communique is held rather than awaiting its
/// instance's turn. Recorded from the owners' answers (Actuation's verify, a
/// remote gateway's occupancy reading); the kernel never decides it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CommuniqueInstanceHold {
    /// No ledger asked records the generation as a current occupant (the
    /// Position is vacant, or held only by other generations elsewhere, or
    /// the Workcell that might hold it could not be asked).
    InstanceAbsent,
    /// Actuation records the generation as ended: the address has moved on.
    InstanceSuperseded,
    /// The generation is current, but not on the required Workcell.
    WorkcellMismatch,
    /// Actuation could not say whether the generation is current.
    InstanceUnverified,
}

impl CommuniqueInstanceHold {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InstanceAbsent => "instance-absent",
            Self::InstanceSuperseded => "instance-superseded",
            Self::WorkcellMismatch => "workcell-mismatch",
            Self::InstanceUnverified => "instance-unverified",
        }
    }
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
    /// Present on an exact-instance route; absent on a durable Position route.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_instance: Option<CommuniqueInstance>,
    /// Why an exact-instance Communique is currently held (state `held`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance_hold: Option<CommuniqueInstanceHold>,
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

    /// Whether an occupant generation standing on `workcell_ref` may receive
    /// this Communique: always for a durable Position route; for an
    /// exact-instance route only the named generation, and only on the
    /// required Workcell when one is required.
    pub fn deliverable_to(&self, generation_ref: &str, workcell_ref: Option<&str>) -> bool {
        match &self.to_instance {
            None => true,
            Some(instance) => {
                instance.generation_ref == generation_ref
                    && instance
                        .required_workcell_ref
                        .as_deref()
                        .is_none_or(|required| workcell_ref == Some(required))
            }
        }
    }

    /// The sender identity as it is attributed — never taken from the body.
    pub fn sender_label(&self) -> String {
        match (self.attribution, self.from_position_ref.as_deref()) {
            (SenderAttribution::Owner, _) => "the owner (decision recorded in Central)".into(),
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
    /// An exact-instance route; `None` is a durable Position route.
    #[serde(default)]
    pub to_instance: Option<CommuniqueInstance>,
    /// Why the exact instance is held at acceptance (requires state `held`).
    #[serde(default)]
    pub instance_hold: Option<CommuniqueInstanceHold>,
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
        SenderAttribution::Unknown | SenderAttribution::Owner => {
            from_position.is_none() && from_generation.is_none()
        }
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

/// An exact-instance binding names a generation, and a hold reason exists
/// only on a held exact-instance Communique.
fn check_instance(
    instance: Option<&CommuniqueInstance>,
    hold: Option<CommuniqueInstanceHold>,
    state: CommuniqueState,
) -> Result<()> {
    if let Some(instance) = instance {
        non_empty("to_instance.generation_ref", &instance.generation_ref)?;
        if let Some(workcell) = &instance.required_workcell_ref {
            non_empty("to_instance.required_workcell_ref", workcell)?;
        }
    }
    match (instance, hold, state) {
        (None, Some(_), _) => Err(invalid(
            "agency_gateway.communique_invalid",
            "an instance hold is recorded only on an exact-instance Communique",
        )),
        (Some(_), Some(_), CommuniqueState::Held) | (_, None, _) => Ok(()),
        (Some(_), Some(hold), state) => Err(invalid(
            "agency_gateway.communique_invalid",
            format!(
                "an exact-instance Communique held for {} must be in state held, not {}",
                hold.as_str(),
                state.as_str()
            ),
        )),
    }
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
            check_instance(
                record.to_instance.as_ref(),
                record.instance_hold,
                record.state,
            )?;
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
        check_instance(draft.to_instance.as_ref(), draft.instance_hold, draft.state)?;
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
                && existing.to_instance == draft.to_instance
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
            to_instance: draft.to_instance,
            instance_hold: draft.instance_hold,
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
        check_instance(
            communique.to_instance.as_ref(),
            communique.instance_hold,
            communique.state,
        )?;
        if let Some(existing) = self.index.get(&communique.communique_ref) {
            let existing = &self.records[*existing];
            if existing.body == communique.body
                && existing.to_position_ref == communique.to_position_ref
                && existing.to_instance == communique.to_instance
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

    /// Mark Communiques delivered to one occupant generation standing on
    /// `workcell_ref` (when known). All refs are checked before any is
    /// changed, so a partial acknowledgement never lands. An exact-instance
    /// Communique is refused to any other generation, and to its generation
    /// on any Workcell but the one it requires.
    pub fn acknowledge(
        &mut self,
        position_ref: &str,
        generation_ref: &str,
        workcell_ref: Option<&str>,
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
            if let Some(instance) = &record.to_instance {
                if instance.generation_ref != generation_ref {
                    return Err(invalid(
                        "agency_gateway.communique_wrong_instance",
                        format!(
                            "Communique {communique_ref} is addressed to the exact instance {}, not {generation_ref}; it is never delivered to another occupant of {position_ref}",
                            instance.generation_ref
                        ),
                    ));
                }
                if let Some(required) = &instance.required_workcell_ref {
                    if workcell_ref != Some(required.as_str()) {
                        return Err(invalid(
                            "agency_gateway.communique_workcell_mismatch",
                            format!(
                                "Communique {communique_ref} requires its instance on Workcell {required}, and this delivery stands on {}",
                                workcell_ref.unwrap_or("an unknown Workcell")
                            ),
                        ));
                    }
                }
            }
        }
        let mut delivered = Vec::new();
        for communique_ref in communique_refs {
            let record = self.get_mut(communique_ref)?;
            record.state = CommuniqueState::Delivered;
            record.delivered_to_generation_ref = Some(generation_ref.into());
            record.delivered_at_unix_ms = Some(delivered_at_unix_ms);
            record.instance_hold = None;
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

    /// Record a changed standing of an exact-instance Communique that is still
    /// this gateway's to deliver: `pending` when its instance is current where
    /// required, `held` with the reason when it is not. The caller read the
    /// owners; the kernel only appends the transition. An unchanged standing
    /// records nothing (`changed == false`).
    pub fn restand(
        &mut self,
        communique_ref: &str,
        state: CommuniqueState,
        hold: Option<CommuniqueInstanceHold>,
        at_unix_ms: u64,
        basis: &str,
    ) -> Result<(Communique, bool)> {
        non_empty("basis", basis)?;
        let record = self.get_mut(communique_ref)?;
        if record.to_instance.is_none() {
            return Err(invalid(
                "agency_gateway.communique_invalid",
                format!("Communique {communique_ref} is a durable Position route; only an exact-instance route is re-stood"),
            ));
        }
        if !record.awaits_local_delivery() || !state.is_undelivered() {
            return Err(invalid(
                "agency_gateway.communique_not_deliverable",
                format!(
                    "Communique {communique_ref} is {}; its standing is no longer this gateway's to change",
                    record.state.as_str()
                ),
            ));
        }
        check_instance(record.to_instance.as_ref(), hold, state)?;
        if record.state == state && record.instance_hold == hold {
            return Ok((record.clone(), false));
        }
        record.state = state;
        record.instance_hold = hold;
        record.transitions.push(CommuniqueTransition {
            at_unix_ms,
            state,
            basis: basis.into(),
            generation_ref: None,
        });
        Ok((record.clone(), true))
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
            to_instance: None,
            instance_hold: None,
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
                None,
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
            None,
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
            None,
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

    fn exact(
        reference: &str,
        generation: &str,
        workcell: Option<&str>,
        state: CommuniqueState,
        hold: Option<CommuniqueInstanceHold>,
    ) -> CommuniqueDraft {
        CommuniqueDraft {
            to_instance: Some(CommuniqueInstance {
                generation_ref: generation.into(),
                required_workcell_ref: workcell.map(str::to_owned),
                agent_session_ref: None,
                agency_ref: None,
            }),
            instance_hold: hold,
            ..draft(reference, Some(A), B, state)
        }
    }

    #[test]
    fn a_durable_route_is_delivered_to_whichever_generation_holds_the_position() {
        let mut journal = CommuniqueJournal::default();
        let (sent, _) = journal
            .send("g", draft("one", Some(A), B, CommuniqueState::Pending))
            .unwrap();
        assert!(sent.to_instance.is_none());
        assert!(sent.deliverable_to("actuation:generation:b2", Some("workcell:z")));
        let delivered = journal
            .acknowledge(
                B,
                "actuation:generation:b2",
                None,
                std::slice::from_ref(&sent.communique_ref),
                20,
                "successor turn",
            )
            .unwrap();
        assert_eq!(
            delivered[0].delivered_to_generation_ref.as_deref(),
            Some("actuation:generation:b2")
        );
    }

    #[test]
    fn an_exact_route_is_refused_to_a_successor_and_to_the_wrong_workcell() {
        let mut journal = CommuniqueJournal::default();
        let (sent, _) = journal
            .send(
                "g",
                exact(
                    "one",
                    "actuation:generation:b1",
                    Some("workcell:b"),
                    CommuniqueState::Pending,
                    None,
                ),
            )
            .unwrap();
        let refs = std::slice::from_ref(&sent.communique_ref);
        assert!(!sent.deliverable_to("actuation:generation:b2", Some("workcell:b")));
        assert!(!sent.deliverable_to("actuation:generation:b1", Some("workcell:a")));
        assert!(!sent.deliverable_to("actuation:generation:b1", None));
        assert!(sent.deliverable_to("actuation:generation:b1", Some("workcell:b")));
        let successor = journal.acknowledge(
            B,
            "actuation:generation:b2",
            Some("workcell:b"),
            refs,
            5,
            "turn",
        );
        assert_eq!(
            successor.unwrap_err().code(),
            "agency_gateway.communique_wrong_instance"
        );
        let elsewhere = journal.acknowledge(
            B,
            "actuation:generation:b1",
            Some("workcell:a"),
            refs,
            5,
            "turn",
        );
        assert_eq!(
            elsewhere.unwrap_err().code(),
            "agency_gateway.communique_workcell_mismatch"
        );
        let unknown = journal.acknowledge(B, "actuation:generation:b1", None, refs, 5, "turn");
        assert_eq!(
            unknown.unwrap_err().code(),
            "agency_gateway.communique_workcell_mismatch"
        );
        // Nothing was marked by the refusals; the instance itself receives it.
        assert_eq!(journal.inbox(B).len(), 1);
        let delivered = journal
            .acknowledge(
                B,
                "actuation:generation:b1",
                Some("workcell:b"),
                refs,
                6,
                "turn",
            )
            .unwrap();
        assert_eq!(delivered[0].state, CommuniqueState::Delivered);
        assert_eq!(
            delivered[0].delivered_to_generation_ref.as_deref(),
            Some("actuation:generation:b1")
        );
    }

    #[test]
    fn an_exact_route_is_held_with_its_reason_and_re_stood_truthfully() {
        let mut journal = CommuniqueJournal::default();
        // A hold reason belongs to a held exact route only.
        let mut durable_hold = draft("x", Some(A), B, CommuniqueState::Held);
        durable_hold.instance_hold = Some(CommuniqueInstanceHold::InstanceAbsent);
        assert_eq!(
            journal.send("g", durable_hold).unwrap_err().code(),
            "agency_gateway.communique_invalid"
        );
        assert_eq!(
            journal
                .send(
                    "g",
                    exact(
                        "y",
                        "actuation:generation:b1",
                        None,
                        CommuniqueState::Pending,
                        Some(CommuniqueInstanceHold::InstanceAbsent),
                    )
                )
                .unwrap_err()
                .code(),
            "agency_gateway.communique_invalid"
        );
        let (held, _) = journal
            .send(
                "g",
                exact(
                    "one",
                    "actuation:generation:b1",
                    None,
                    CommuniqueState::Held,
                    Some(CommuniqueInstanceHold::InstanceAbsent),
                ),
            )
            .unwrap();
        assert_eq!(
            held.instance_hold,
            Some(CommuniqueInstanceHold::InstanceAbsent)
        );
        let (pending, changed) = journal
            .restand(
                &held.communique_ref,
                CommuniqueState::Pending,
                None,
                11,
                "current on workcell:b",
            )
            .unwrap();
        assert!(changed);
        assert_eq!(pending.state, CommuniqueState::Pending);
        assert!(pending.instance_hold.is_none());
        let (_, changed) = journal
            .restand(
                &held.communique_ref,
                CommuniqueState::Pending,
                None,
                12,
                "again",
            )
            .unwrap();
        assert!(!changed, "an unchanged standing appends nothing");
        let (superseded, _) = journal
            .restand(
                &held.communique_ref,
                CommuniqueState::Held,
                Some(CommuniqueInstanceHold::InstanceSuperseded),
                13,
                "actuation: b1 superseded by b2",
            )
            .unwrap();
        assert_eq!(
            superseded
                .transitions
                .iter()
                .map(|t| t.state)
                .collect::<Vec<_>>(),
            vec![
                CommuniqueState::Held,
                CommuniqueState::Pending,
                CommuniqueState::Held
            ]
        );
        // A durable route is never re-stood.
        let (durable, _) = journal
            .send("g", draft("two", Some(A), B, CommuniqueState::Held))
            .unwrap();
        assert!(journal
            .restand(
                &durable.communique_ref,
                CommuniqueState::Pending,
                None,
                1,
                "x"
            )
            .is_err());
        // The standing survives a restore.
        let restored = CommuniqueJournal::restore(journal.records().to_vec()).unwrap();
        assert_eq!(
            restored.get(&held.communique_ref).unwrap().instance_hold,
            Some(CommuniqueInstanceHold::InstanceSuperseded)
        );
    }

    #[test]
    fn a_duplicate_exact_send_or_relay_stays_single_and_a_retargeted_one_is_refused() {
        let mut journal = CommuniqueJournal::default();
        let one = || {
            exact(
                "one",
                "actuation:generation:b1",
                Some("workcell:b"),
                CommuniqueState::Pending,
                None,
            )
        };
        let (sent, _) = journal.send("g", one()).unwrap();
        assert!(journal.send("g", one()).unwrap().1);
        let retargeted = exact(
            "one",
            "actuation:generation:b2",
            None,
            CommuniqueState::Pending,
            None,
        );
        assert_eq!(
            journal.send("g", retargeted).unwrap_err().code(),
            "agency_gateway.communique_identity_rewrite"
        );
        // A durable draft under the same ref is also a different message.
        assert_eq!(
            journal
                .send("g", draft("one", Some(A), B, CommuniqueState::Pending))
                .unwrap_err()
                .code(),
            "agency_gateway.communique_identity_rewrite"
        );
        assert_eq!(journal.len(), 1);

        let mut remote = CommuniqueJournal::default();
        assert!(!remote.ingest(sent.clone(), "g", 20).unwrap().1);
        assert!(remote.ingest(sent.clone(), "g", 21).unwrap().1);
        let mut widened = sent.clone();
        widened.to_instance = None;
        assert_eq!(
            remote.ingest(widened, "g", 22).unwrap_err().code(),
            "agency_gateway.communique_identity_rewrite"
        );
        assert_eq!(remote.len(), 1);
        assert_eq!(remote.records()[0].to_instance, sent.to_instance);
    }

    #[test]
    fn out_of_order_relays_keep_the_receiving_journal_order_and_the_senders_times() {
        let mut origin = CommuniqueJournal::default();
        let mut first = exact(
            "one",
            "actuation:generation:b1",
            None,
            CommuniqueState::Pending,
            None,
        );
        first.sent_at_unix_ms = 100;
        let mut second = exact(
            "two",
            "actuation:generation:b1",
            None,
            CommuniqueState::Pending,
            None,
        );
        second.sent_at_unix_ms = 200;
        let (first, _) = origin.send("g", first).unwrap();
        let (second, _) = origin.send("g", second).unwrap();

        let mut remote = CommuniqueJournal::default();
        // The later one arrives first.
        remote.ingest(second.clone(), "g", 300).unwrap();
        remote.ingest(first.clone(), "g", 400).unwrap();
        let inbox = remote.inbox(B);
        assert_eq!(
            inbox
                .iter()
                .map(|r| (r.communique_ref.as_str(), r.sequence, r.sent_at_unix_ms))
                .collect::<Vec<_>>(),
            vec![
                (second.communique_ref.as_str(), 1, 200),
                (first.communique_ref.as_str(), 2, 100)
            ]
        );
        // Each record keeps its origin's transitions and adds its arrival.
        assert_eq!(inbox[0].transitions.last().unwrap().at_unix_ms, 300);
        assert_eq!(inbox[1].transitions.last().unwrap().at_unix_ms, 400);
        assert!(CommuniqueJournal::restore(remote.records().to_vec()).is_ok());
    }
}
