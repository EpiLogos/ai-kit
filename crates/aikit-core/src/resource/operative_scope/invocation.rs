//! Native one-Action invocation evidence for source-qualified Resolve.
//!
//! This module deliberately does not execute processes, schedule workflows, or
//! create another permission/resource store. It fixes the immutable identity of
//! a qualified invocation, its retries/attempts and independently arriving
//! Returns so the application layer can pass the already-admitted Action to the
//! existing AIKit runner and retain what actually happened.

use serde::{Deserialize, Serialize};

use crate::{AikitError, Result};

use super::{digest, reference, require, ObservedExpressionScope, ScopedContextResolution};
use crate::resource::{ActionRef, ResolvedActionCandidate, ResourceRef, SourceRevision};

pub const SCOPED_ACTION_INVOCATION_VERSION: &str = "aikit.scoped-action-invocation/v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopedActionAuthority {
    pub authority_ref: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authority_revision: Option<SourceRevision>,
    pub action: ActionRef,
    pub subject: ResourceRef,
    pub granted: bool,
    pub unattended: bool,
    #[serde(default)]
    pub evidence: Vec<ResourceRef>,
}

impl ScopedActionAuthority {
    pub fn validate_for(&self, action: &ActionRef, subject: &ResourceRef) -> Result<()> {
        reference(self.authority_ref.as_str())?;
        if let Some(revision) = &self.authority_revision {
            reference(revision.as_str())?;
        }
        require(
            self.action == *action && self.subject == *subject,
            "resolve.scoped_action_authority_mismatch",
            "Action authority must identify the exact admitted Action and subject",
        )?;
        require(
            self.granted,
            "resolve.scoped_action_authority_denied",
            "Action authority does not grant this invocation",
        )?;
        require(
            !self.evidence.is_empty() && self.evidence.len() <= 4096,
            "resolve.scoped_action_authority_evidence",
            "Action authority requires bounded owner evidence",
        )?;
        for evidence in &self.evidence {
            reference(evidence.as_str())?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ScopedInputDisposition {
    NewInput,
    Replay { original_invocation: ResourceRef },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopedActionInput {
    pub input_ref: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<SourceRevision>,
    /// Exact content digest supplied by the native input owner. AIKit preserves
    /// it; it does not reinterpret the input in order to mint a second identity.
    pub digest: String,
    pub disposition: ScopedInputDisposition,
}

impl ScopedActionInput {
    pub fn validate(&self) -> Result<()> {
        reference(self.input_ref.as_str())?;
        if let Some(revision) = &self.revision {
            reference(revision.as_str())?;
        }
        require(
            self.digest.len() == 64
                && self
                    .digest
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "resolve.scoped_action_input_digest",
            "scoped invocation input requires an exact lowercase 64-character digest",
        )?;
        if let ScopedInputDisposition::Replay {
            original_invocation,
        } = &self.disposition
        {
            reference(original_invocation.as_str())?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopedActionInvocation {
    pub version: String,
    pub invocation_ref: ResourceRef,
    pub resolve_path_identity: String,
    pub action: ActionRef,
    pub subject: ResourceRef,
    /// Every nested provider binding that was current at admission. The Return
    /// carries this unchanged even when later observations are stale or absent.
    #[serde(default)]
    pub original_scopes: Vec<ObservedExpressionScope>,
    pub authority: ScopedActionAuthority,
    pub input: ScopedActionInput,
}

impl ScopedActionInvocation {
    pub fn admit(
        scoped: &ScopedContextResolution,
        candidate: &ResolvedActionCandidate,
        subject: ResourceRef,
        authority: ScopedActionAuthority,
        input: ScopedActionInput,
    ) -> Result<Self> {
        scoped.require_current()?;
        require(
            candidate.available_in_context,
            "resolve.scoped_action_unavailable",
            "the Action is not available in the native ContextResolution",
        )?;
        authority.validate_for(&candidate.action, &subject)?;
        input.validate()?;

        let original_scopes = scoped.observations().to_vec();
        if !original_scopes.is_empty() {
            require(
                original_scopes
                    .iter()
                    .any(|observed| observed.scope.binding.subject == subject),
                "resolve.scoped_action_subject_outside_scope",
                "the invocation subject is not named by any admitted source-qualified scope",
            )?;
        }
        let resolve_path_identity = scoped.path().native().identity.clone();
        reference(&resolve_path_identity)?;
        let identity = (
            SCOPED_ACTION_INVOCATION_VERSION,
            &resolve_path_identity,
            &candidate.action,
            &subject,
            &original_scopes,
            &authority,
            &input,
        );
        let invocation_ref = ResourceRef::parse(format!(
            "invocation/scoped/{}",
            digest("aikit.scoped-action-invocation.identity", &identity)?
        ))?;
        Ok(Self {
            version: SCOPED_ACTION_INVOCATION_VERSION.into(),
            invocation_ref,
            resolve_path_identity,
            action: candidate.action.clone(),
            subject,
            original_scopes,
            authority,
            input,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum ScopedActionAttemptOutcome {
    Completed {
        status: i32,
        detached: bool,
        result_digest: String,
    },
    Failed {
        code: String,
        message: String,
    },
    Cancelled {
        reason: String,
    },
}

impl ScopedActionAttemptOutcome {
    fn validate(&self) -> Result<()> {
        match self {
            Self::Completed { result_digest, .. } => require(
                result_digest.len() == 64
                    && result_digest
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
                "resolve.scoped_action_result_digest",
                "completed Action attempt requires an exact lowercase 64-character result digest",
            ),
            Self::Failed { code, message } => require(
                !code.trim().is_empty() && !message.trim().is_empty(),
                "resolve.scoped_action_failure_empty",
                "failed Action attempt requires an error code and message",
            ),
            Self::Cancelled { reason } => require(
                !reason.trim().is_empty(),
                "resolve.scoped_action_cancel_reason_empty",
                "cancelled Action attempt requires a reason",
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopedActionAttemptEvidence {
    pub version: String,
    pub attempt_ref: ResourceRef,
    pub invocation_ref: ResourceRef,
    /// Retries keep the invocation identity and advance only this ordinal.
    pub ordinal: u32,
    pub started_at_unix_ms: u64,
    pub finished_at_unix_ms: u64,
    pub outcome: ScopedActionAttemptOutcome,
}

impl ScopedActionAttemptEvidence {
    pub fn finish(
        invocation: &ScopedActionInvocation,
        ordinal: u32,
        started_at_unix_ms: u64,
        finished_at_unix_ms: u64,
        outcome: ScopedActionAttemptOutcome,
    ) -> Result<Self> {
        require(
            ordinal > 0,
            "resolve.scoped_action_attempt_ordinal",
            "Action attempt ordinals start at one",
        )?;
        require(
            finished_at_unix_ms >= started_at_unix_ms,
            "resolve.scoped_action_attempt_time",
            "Action attempt completion cannot precede its start observation",
        )?;
        outcome.validate()?;
        let attempt_ref = ResourceRef::parse(format!(
            "attempt/scoped/{}/{}",
            invocation.invocation_ref, ordinal
        ))?;
        Ok(Self {
            version: SCOPED_ACTION_INVOCATION_VERSION.into(),
            attempt_ref,
            invocation_ref: invocation.invocation_ref.clone(),
            ordinal,
            started_at_unix_ms,
            finished_at_unix_ms,
            outcome,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopedReturnArrival {
    pub observed_at_unix_ms: u64,
    /// Optional producer order, when the native owner supplies one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producer_sequence: Option<u64>,
    /// Receiver order is retained independently, so reordered delivery remains
    /// evidence rather than being normalised into producer order.
    pub delivery_sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopedActionReturnEvidence {
    pub version: String,
    pub return_ref: ResourceRef,
    pub invocation_ref: ResourceRef,
    pub attempt_ref: ResourceRef,
    pub action: ActionRef,
    pub subject: ResourceRef,
    #[serde(default)]
    pub original_scopes: Vec<ObservedExpressionScope>,
    #[serde(default)]
    pub completion_scopes: Vec<ObservedExpressionScope>,
    pub arrival: ScopedReturnArrival,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_digest: Option<String>,
    #[serde(default)]
    pub evidence: Vec<ResourceRef>,
}

impl ScopedActionReturnEvidence {
    pub fn observe(
        invocation: &ScopedActionInvocation,
        attempt: &ScopedActionAttemptEvidence,
        completion_scopes: Vec<ObservedExpressionScope>,
        arrival: ScopedReturnArrival,
        result_digest: Option<String>,
        evidence: Vec<ResourceRef>,
    ) -> Result<Self> {
        require(
            attempt.invocation_ref == invocation.invocation_ref,
            "resolve.scoped_action_return_attempt_mismatch",
            "Return attempt does not belong to this invocation",
        )?;
        require(
            arrival.delivery_sequence > 0,
            "resolve.scoped_action_return_sequence",
            "Return delivery sequence starts at one",
        )?;
        if let Some(result_digest) = &result_digest {
            require(
                result_digest.len() == 64
                    && result_digest
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
                "resolve.scoped_action_return_digest",
                "Return result digest must be an exact lowercase 64-character digest",
            )?;
        }
        require(
            !evidence.is_empty() && evidence.len() <= 4096,
            "resolve.scoped_action_return_evidence",
            "Return requires bounded native evidence",
        )?;
        for item in &evidence {
            reference(item.as_str())?;
        }
        let identity = (
            SCOPED_ACTION_INVOCATION_VERSION,
            &invocation.invocation_ref,
            &attempt.attempt_ref,
            &arrival,
            &result_digest,
            &evidence,
        );
        let return_ref = ResourceRef::parse(format!(
            "return/scoped/{}",
            digest("aikit.scoped-action-return.identity", &identity)?
        ))?;
        Ok(Self {
            version: SCOPED_ACTION_INVOCATION_VERSION.into(),
            return_ref,
            invocation_ref: invocation.invocation_ref.clone(),
            attempt_ref: attempt.attempt_ref.clone(),
            action: invocation.action.clone(),
            subject: invocation.subject.clone(),
            original_scopes: invocation.original_scopes.clone(),
            completion_scopes,
            arrival,
            result_digest,
            evidence,
        })
    }
}
