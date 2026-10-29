//! One turn of a resident agent session, taken over the resident encounter
//! owner's own socket: the same `Read` / `Draft` / `Prompt` acts every client
//! submits a turn with, then the session's journal read until its
//! `TurnEnded`. The Day's agent (`day_move_runner.rs`) and a take's body
//! (`encounter_take_body.rs`) both take their turns through here, so the
//! resident lifecycle has one client path, not one per caller.
//!
//! What the agent said is its `agent-message-chunk` text; what the turn cost
//! is the provider's own usage on its `completed` signal, never estimated. A
//! turn that asks for a tool's permission is cancelled — no caller here
//! answers the provider's consent on the person's behalf — and a turn that
//! does not end within its time is cancelled at the timeout.
use crate::encounter_service::EncounterRequest;
use aikit_core::ResourceRef;
use serde_json::Value;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// The owner's refusal, in its own code and words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerError {
    pub code: String,
    pub message: String,
}

/// Which act of a submission the owner refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitStage {
    /// Reading the session's journal and draft.
    Read,
    /// Setting the draft the prompt submits.
    Draft,
    /// Submitting the draft as the turn's prompt.
    Prompt,
}

/// The words a caller's turn speaks in: what its session is called and what
/// the turn is (`"the Day's session"`, `"a Day move"`).
#[derive(Debug, Clone, Copy)]
pub struct TurnWords<'a> {
    pub session: &'a str,
    pub act: &'a str,
}

/// What a finished turn gave: the agent's text and the provider's own usage
/// report, when it sent one.
#[derive(Debug, Clone, PartialEq)]
pub struct TurnAnswer {
    pub said: String,
    pub usage: Option<Value>,
}

/// The resident encounter owner at its socket.
#[derive(Debug, Clone)]
pub struct ResidentOwner {
    pub socket: PathBuf,
}

#[cfg(unix)]
impl ResidentOwner {
    pub fn at(socket: PathBuf) -> Self {
        Self { socket }
    }

    /// One request; the owner's refusal comes back as an `OwnerError`.
    pub fn call(&self, request: &EncounterRequest) -> Result<Value, OwnerError> {
        let response =
            crate::encounter_service::request(&self.socket, request).map_err(|error| {
                OwnerError {
                    code: error.code().to_owned(),
                    message: error.message().to_owned(),
                }
            })?;
        if response["ok"] != true {
            return Err(OwnerError {
                code: response["error"]["code"]
                    .as_str()
                    .unwrap_or("encounter.refused")
                    .to_owned(),
                message: response["error"]["message"]
                    .as_str()
                    .unwrap_or("the encounter owner refused the request")
                    .to_owned(),
            });
        }
        Ok(response["data"].clone())
    }

    /// The journal's last cursor now: a turn's events are the ones after it.
    pub fn tail(&self, session: &ResourceRef) -> Result<u64, OwnerError> {
        let mut after = 0;
        loop {
            let page = self.call(&EncounterRequest::Read {
                agent_session: session.clone(),
                after,
                limit: 256,
            })?;
            after = page["next_cursor"].as_u64().unwrap_or(after);
            if page["more"] != true {
                return Ok(after);
            }
        }
    }

    /// Set `text` as the session's draft against the draft revision that
    /// stands, and prompt it. Answers the cursor the turn's events follow.
    pub fn submit(
        &self,
        session: &ResourceRef,
        text: &str,
    ) -> Result<u64, (SubmitStage, OwnerError)> {
        let baseline = self
            .tail(session)
            .map_err(|error| (SubmitStage::Read, error))?;
        let page = self
            .call(&EncounterRequest::Read {
                agent_session: session.clone(),
                after: baseline,
                limit: 1,
            })
            .map_err(|error| (SubmitStage::Read, error))?;
        let drafted = self
            .call(&EncounterRequest::Draft {
                agent_session: session.clone(),
                basis: page["draft"]["revision"].as_u64().unwrap_or(0),
                text: text.to_owned(),
            })
            .map_err(|error| (SubmitStage::Draft, error))?;
        let revision = drafted["revision"].as_u64().ok_or_else(|| {
            (
                SubmitStage::Draft,
                OwnerError {
                    code: "encounter.draft_revision".into(),
                    message: "the encounter owner did not answer a draft revision".into(),
                },
            )
        })?;
        self.call(&EncounterRequest::Prompt {
            agent_session: session.clone(),
            draft_revision: revision,
            interact: None,
        })
        .map_err(|error| (SubmitStage::Prompt, error))?;
        Ok(baseline)
    }

    /// Read the session's journal from `after` until the turn ends. A turn
    /// that asks for a tool's permission, or does not end within `timeout`,
    /// is cancelled and refused in the caller's words.
    pub fn await_turn(
        &self,
        session: &ResourceRef,
        after: u64,
        timeout: Duration,
        words: TurnWords<'_>,
    ) -> Result<TurnAnswer, String> {
        let deadline = Instant::now() + timeout;
        let cancel = |reason: String| {
            let _ = self.call(&EncounterRequest::Cancel {
                agent_session: session.clone(),
                reason: Some(reason),
            });
        };
        let mut cursor = after;
        let mut said = String::new();
        let mut usage = None;
        loop {
            let page = self
                .call(&EncounterRequest::Read {
                    agent_session: session.clone(),
                    after: cursor,
                    limit: 256,
                })
                .map_err(|error| {
                    format!("{} could not be read: {}", words.session, error.message)
                })?;
            for row in page["events"].as_array().into_iter().flatten() {
                cursor = row["cursor"].as_u64().unwrap_or(cursor);
                let kind = &row["event"]["event"]["Signal"]["kind"];
                match kind["kind"].as_str() {
                    Some("agent-message-chunk") => {
                        said.push_str(kind["text"].as_str().unwrap_or_default())
                    }
                    Some("completed") => {
                        if !kind["usage"].is_null() {
                            usage = Some(kind["usage"].clone());
                        }
                    }
                    Some("permission-requested") => {
                        cancel(format!("{} takes no tool", words.act));
                        return Err(format!(
                            "the agent asked for permission to use a tool, and {} takes none; the turn was cancelled",
                            words.act
                        ));
                    }
                    _ => {}
                }
                if let Some(ended) = row["event"]["event"].get("TurnEnded") {
                    let stop = &ended["stop"];
                    return if stop.get("Completed").is_some() {
                        Ok(TurnAnswer { said, usage })
                    } else if let Some(failed) = stop.get("Failed") {
                        Err(format!(
                            "the agent's turn failed: {}",
                            failed["reason"].as_str().unwrap_or("no reason given")
                        ))
                    } else if stop.get("OperationalLimit").is_some() {
                        Err("the agent's turn hit the harness's signal limit".into())
                    } else {
                        Err("the agent's turn was cancelled".into())
                    };
                }
            }
            if page["more"] != true {
                if Instant::now() >= deadline {
                    cancel(format!("{} timed out", words.act));
                    return Err(format!(
                        "the agent did not finish within {} seconds; the turn was cancelled",
                        timeout.as_secs()
                    ));
                }
                std::thread::sleep(Duration::from_millis(150));
            }
        }
    }
}

/// The total tokens a provider's own usage report names: its total, else
/// its input and output together. None when it names neither.
pub fn total_tokens(usage: &Value) -> Option<u64> {
    let field = |names: &[&str]| names.iter().find_map(|name| usage[*name].as_u64());
    field(&["totalTokens", "total_tokens"]).or_else(|| {
        let input = field(&["inputTokens", "input_tokens"])?;
        let output = field(&["outputTokens", "output_tokens"])?;
        input.checked_add(output)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn total_tokens_reads_the_providers_total_else_its_input_and_output() {
        assert_eq!(total_tokens(&json!({"totalTokens": 12})), Some(12));
        assert_eq!(total_tokens(&json!({"total_tokens": 3})), Some(3));
        assert_eq!(
            total_tokens(&json!({"inputTokens": 5, "outputTokens": 2})),
            Some(7)
        );
        assert_eq!(total_tokens(&json!({"inputTokens": 5})), None);
        assert_eq!(total_tokens(&Value::Null), None);
    }
}
