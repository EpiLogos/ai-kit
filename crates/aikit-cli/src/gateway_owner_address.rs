//! The owner is an address on the inter-now communication system.
//!
//! Agents message each other by Position. The person occupies no Position:
//! their one door is Central's receiving ledger, which their Inbox reads
//! (`central.receiving.*`). So `send --to @owner` does not journal a
//! Communique nobody could occupy — it submits a *request* Return to Central:
//! a question (the default) or a proposal, attributed to the sending Position
//! as its declared producer, with the credentialed carrier authenticated as
//! itself. Nothing is copied into a gateway-side inbox.
//!
//! The way back is the ordinary send plane. On each gateway service tick the
//! reply pass reads decided requests whose producer is a Position and sends
//! that Position a Communique carrying the person's decision, attributed
//! `owner`. The reply's ref and `sent_at` derive from the decision itself, so
//! a repeated pass replays in the journal instead of delivering twice — no
//! second store remembers what was already answered.

use sha2::{Digest, Sha256};

use super::*;

/// The owner's address, as agents write it.
pub const OWNER_ADDRESS: &str = "@owner";

pub fn is_owner_address(to: &str) -> bool {
    matches!(to.trim(), OWNER_ADDRESS | "owner")
}

/// What an Agent asks the person to decide. A message with no options is a
/// question; `propose` makes it a proposal of work, optionally for an owner
/// that would carry it out (e.g. `factory`).
#[derive(Debug, Clone, Default)]
pub struct OwnerAsk {
    pub subject: Option<String>,
    pub propose: bool,
    pub proposed_owner: Option<String>,
    pub options: Vec<String>,
    pub now_ref: Option<String>,
    pub evidence_refs: Vec<String>,
}

const SUBJECT_LIMIT: usize = 280;

/// The first line, bounded to Central's one-line subject on a char boundary.
fn subject_of(body: &str) -> String {
    let line = body
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    if line.len() <= SUBJECT_LIMIT {
        return line.to_owned();
    }
    let mut end = SUBJECT_LIMIT - 1;
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &line[..end])
}

pub(super) fn send_to_owner(
    owners: &dyn ContactOwners,
    request: &SendRequest<'_>,
) -> Result<Value> {
    if request.instance.is_some() || request.require_workcell.is_some() {
        return Err(three_part(
            "gateway.owner_has_no_instance",
            "The owner occupies no Position, so there is no generation or Workcell to bind to.",
            nothing_sent(),
            "Send to @owner without --instance or --require-workcell.",
        ));
    }
    let sender = resolve_sender(owners, request.from_position)?;
    let Some(position) = sender.from_position_ref.clone() else {
        return Err(three_part(
            "gateway.owner_needs_attribution",
            "Nothing names the sender (no --from-position and no OI_POSITION_REF in this body's environment), and the person is never asked by an unknown sender.",
            nothing_sent(),
            "Speak as your Position: set OI_POSITION_REF or pass --from-position.",
        ));
    };
    let ask = &request.owner;
    let subject = match &ask.subject {
        Some(subject) => subject.trim().to_owned(),
        None => subject_of(&request.body),
    };
    let body = request.body.trim();
    let mut asked = json!({
        "kind": if ask.propose { "proposal" } else { "question" },
        "subject": subject,
    });
    if body != subject {
        asked["body"] = json!(body);
    }
    if ask.propose {
        if !ask.options.is_empty() {
            return Err(three_part(
                "gateway.owner_proposal_options",
                "A proposal is accepted or declined; offered answers belong to a question.",
                nothing_sent(),
                "Drop --option, or ask it as a question (without --propose).",
            ));
        }
        if let Some(owner) = &ask.proposed_owner {
            asked["proposed_owner_ref"] = json!(owner);
        }
    } else {
        if ask.proposed_owner.is_some() {
            return Err(three_part(
                "gateway.owner_question_owner",
                "--for names who would carry out a proposal; a question has none.",
                nothing_sent(),
                "Add --propose, or drop --for.",
            ));
        }
        if !ask.options.is_empty() {
            asked["options"] = json!(ask.options);
        }
    }
    let attribution = match sender.attribution {
        SenderAttribution::Verified => "verified",
        _ => "claimed",
    };
    let producer_key = format!(
        "gateway-ask:{}",
        ulid::Ulid::generate().to_string().to_ascii_lowercase()
    );
    let mut input = json!({
        "producer_key": producer_key,
        "request": asked,
        "declared_producer": {"ref": position, "actor_kind": "agent", "attribution": attribution},
        "occurred_at_unix_seconds": now_unix_ms() / 1000,
    });
    if let Some(now_ref) = &ask.now_ref {
        input["now_ref"] = json!(now_ref);
    }
    if !ask.evidence_refs.is_empty() {
        input["evidence_refs"] = json!(ask.evidence_refs);
    }
    if let Some(reply_to) = &request.reply_to {
        input["reply_to"] = json!(reply_to);
    }
    let project = request.project_world.and_then(project_input);
    let received = match owners
        .receiving_submit(project.as_deref(), &input)
        .map_err(|unavailable| {
            owner_unavailable_refusal("Central could not receive the request", &unavailable)
        })? {
        Ok(received) => received,
        Err(refusal) => {
            return Err(three_part(
                "gateway.owner_refused",
                format!(
                    "Central refused the request ({}): {}",
                    refusal.code, refusal.fact
                ),
                refusal.consequence,
                refusal.action,
            ))
        }
    };
    Ok(json!({
        "addressed": "owner",
        "return_ref": received["return_ref"],
        "status": received["record"]["status"],
        "project": project,
        "sender": sender,
        "delivery": {
            "fact": format!("The person's Inbox now holds this {} from {position}.", if ask.propose {"proposal"} else {"question"}),
            "consequence": "Nothing more happens until the person decides; the decision is recorded in Central, not guessed here.",
            "action": format!("Their answer arrives as a Communique from the owner at your next turn, and in your NOW reading{}.",
                ask.now_ref.as_deref().map(|now| format!(" ({now})")).unwrap_or_default()),
        },
    }))
}

/// Decisions a reply is owed for: a request, decided, from a Position.
fn decided_by_owner(row: &Value) -> bool {
    row["kind"] == "request"
        && matches!(
            row["status"].as_str(),
            Some("accepted" | "rejected" | "answered" | "included")
        )
        && row["declared_producer"]["ref"]
            .as_str()
            .is_some_and(|producer| producer.starts_with("central:position:"))
}

fn reply_body(record: &Value) -> String {
    let request = &record["request"];
    let review = &record["review"];
    let kind = request["kind"].as_str().unwrap_or("request");
    let subject = request["subject"].as_str().unwrap_or_default();
    let decision = match record["status"].as_str().unwrap_or_default() {
        "answered" => "answered",
        "rejected" => "declined",
        "included" => "accepted and it was carried out",
        _ => "accepted",
    };
    let mut body = format!("The person {decision} your {kind}: \"{subject}\".");
    if let Some(answer) = review["answer"].as_str() {
        body.push_str(&format!("\n\nTheir answer: {answer}"));
    }
    if let Some(note) = review["note"].as_str() {
        body.push_str(&format!("\n\nTheir note: {note}"));
    }
    if let Some(realised) = record["realisation"]["ref"].as_str() {
        let owner = record["realisation"]["owner_ref"]
            .as_str()
            .unwrap_or("its owner");
        body.push_str(&format!("\n\n{owner} made {realised} for it."));
    }
    body.push_str(&format!(
        "\n\n(Central Return {})",
        record["return_ref"].as_str().unwrap_or_default()
    ));
    body
}

/// One reply per decision: the ref and clock derive from the Return and the
/// decision, so a repeated pass is a journal replay, never a second delivery.
fn reply_identity(record: &Value) -> (String, u64) {
    let reviewed = record["review"]["reviewed_at_unix_seconds"]
        .as_u64()
        .unwrap_or_default();
    let digest = Sha256::digest(format!(
        "{}\n{}\n{}\n{}",
        record["return_ref"].as_str().unwrap_or_default(),
        record["status"].as_str().unwrap_or_default(),
        reviewed,
        record["realisation"]["ref"].as_str().unwrap_or_default()
    ));
    let hex: String = digest.iter().take(13).map(|b| format!("{b:02x}")).collect();
    (
        format!("{COMMUNIQUE_REF_PREFIX}owner-{hex}"),
        reviewed * 1000,
    )
}

/// The reply pass: every register Central discloses, every decided request a
/// Position made, one Communique back. Owners that cannot answer are counted
/// and named; a pass never fails the service tick for one register.
pub fn owner_reply_pass(
    home: &AikitHome,
    owners: &dyn ContactOwners,
    gateway: &dyn GatewayAccess,
    cwd: &Path,
) -> Result<Value> {
    let mut registers: Vec<Option<String>> = vec![None];
    let mut unavailable = Vec::new();
    match owners.world_projects() {
        Ok(projects) => registers.extend(projects.into_iter().map(Some)),
        Err(owner) => unavailable.push(owner.to_string()),
    }
    let (local, _) = local_workcell(owners, cwd);
    let (mut sent, mut replayed) = (0u64, 0u64);
    for project in &registers {
        let page = match owners.receiving_list(project.as_deref()) {
            Ok(page) => page,
            Err(owner) => {
                unavailable.push(owner.to_string());
                continue;
            }
        };
        let rows = page["returns"].as_array().cloned().unwrap_or_default();
        for row in rows.iter().filter(|row| decided_by_owner(row)) {
            let Some(return_ref) = row["return_ref"].as_str() else {
                continue;
            };
            let reading = match owners.receiving_read(project.as_deref(), return_ref) {
                Ok(reading) => reading,
                Err(owner) => {
                    unavailable.push(owner.to_string());
                    continue;
                }
            };
            let mut record = reading["record"].clone();
            record["return_ref"] = json!(return_ref);
            let position = row["declared_producer"]["ref"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            let recipient = Recipient {
                position_ref: position.clone(),
                handle: None,
                label: None,
                source: "central.receiving".into(),
                agency_ref: None,
            };
            let routing = match route_to_occupancy(home, owners, local.as_deref(), &recipient) {
                Ok(routing) => routing,
                Err(error) => {
                    unavailable.push(format!("routing {position}: {error}"));
                    continue;
                }
            };
            let (communique_ref, sent_at_unix_ms) = reply_identity(&record);
            let draft = CommuniqueDraft {
                communique_ref,
                from_position_ref: None,
                from_generation_ref: None,
                attribution: SenderAttribution::Owner,
                attribution_basis: format!(
                    "the person's review of Central Return {return_ref}, authenticated by Central"
                ),
                to_position_ref: position,
                to_workcell_ref: routing.occupant_workcell,
                to_instance: None,
                instance_hold: None,
                body: reply_body(&record),
                sent_at_unix_ms,
                state: routing.state,
                state_basis: routing.state_basis,
                reply_to: None,
                forward_to_workcell_ref: routing.remote.as_ref().map(|r| r.workcell_ref.clone()),
                routing: routing.routing,
            };
            let (_, was_replay, _) =
                expect_accepted(gateway.call(GatewayCommand::SendCommunique {
                    draft: Box::new(draft),
                })?)?;
            if was_replay {
                replayed += 1;
            } else {
                sent += 1;
            }
        }
    }
    Ok(json!({
        "owner_replies": {"sent": sent, "already_sent": replayed,
            "registers": registers.len(), "unavailable": unavailable}
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_owner_address_is_reserved_and_subjects_are_one_bounded_line() {
        assert!(is_owner_address("@owner") && is_owner_address(" owner "));
        assert!(!is_owner_address("@oi"));
        assert_eq!(subject_of("\n  First line\nsecond"), "First line");
        let long = "é".repeat(400);
        let subject = subject_of(&long);
        assert!(subject.len() <= SUBJECT_LIMIT + '…'.len_utf8() && subject.ends_with('…'));
    }

    #[test]
    fn only_decided_requests_from_positions_are_owed_a_reply() {
        let row = |kind: &str, status: &str, producer: &str| json!({"kind":kind,"status":status,"declared_producer":{"ref":producer}});
        let position = "central:position:project:O-I:epii";
        assert!(decided_by_owner(&row("request", "answered", position)));
        assert!(decided_by_owner(&row("request", "included", position)));
        assert!(!decided_by_owner(&row("request", "pending", position)));
        assert!(!decided_by_owner(&row(
            "contribution",
            "included",
            position
        )));
        assert!(!decided_by_owner(&row(
            "request",
            "answered",
            "agent-session/x"
        )));
    }

    #[test]
    fn a_reply_is_identified_by_its_decision_so_a_second_pass_replays() {
        let record = json!({"return_ref":"central:return:control:root:ab","status":"answered",
            "request":{"kind":"question","subject":"Omarchy now?"},
            "review":{"reviewed_at_unix_seconds":1790700000,"answer":"Omarchy now."}});
        let (first, at) = reply_identity(&record);
        assert_eq!(reply_identity(&record.clone()), (first.clone(), at));
        assert_eq!(at, 1_790_700_000_000);
        assert!(first.starts_with(COMMUNIQUE_REF_PREFIX));
        let mut realised = record.clone();
        realised["status"] = json!("included");
        assert_ne!(
            reply_identity(&realised).0,
            first,
            "a new decision is a new reply"
        );
        let body = reply_body(&record);
        assert!(body.contains("The person answered your question: \"Omarchy now?\""));
        assert!(body.contains("Their answer: Omarchy now."));
        assert!(body.contains("central:return:control:root:ab"));
    }
}
