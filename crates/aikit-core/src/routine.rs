//! Proven Method -> authorised Routine application semantics.
//!
//! A Routine is a durable semantic automation intention over an exact, verified
//! Method revision. It is not the Method itself, not a Procedure, not a scheduler
//! job and not a grant of Action authority. Provider jobs remain material evidence
//! attached to the stable RoutineRef.

use serde::{Deserialize, Serialize};

use crate::method::Method;
use crate::resource::{
    ProviderRef, ResourceDescriptor, ResourceKind, ResourceRecord, ResourceRef, SourceRef,
    SourceRevision,
};
use crate::{AikitError, Result};

pub const ROUTINE_VERSION: &str = "aikit.routine/v1";
pub const METHOD_PROOF_VERSION: &str = "aikit.method-proof/v1";
pub const ROUTINE_INVOCATION_EVIDENCE_VERSION: &str = "aikit.routine-invocation-evidence/v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MethodProofInput {
    pub proof_ref: ResourceRef,
    pub context_resolution_ref: ResourceRef,
    #[serde(default)]
    pub activity_refs: Vec<ResourceRef>,
    #[serde(default)]
    pub return_refs: Vec<ResourceRef>,
    #[serde(default)]
    pub evidence_refs: Vec<ResourceRef>,
    #[serde(default)]
    pub verification_refs: Vec<ResourceRef>,
    pub invocation_succeeded: bool,
    pub verification_passed: bool,
}

/// Exact evidence basis under which one Method revision became eligible for
/// explicit automation. A successful invocation alone never creates this value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProvenMethodBasis {
    pub version: String,
    pub method: ResourceRef,
    pub method_revision: SourceRevision,
    pub proof_ref: ResourceRef,
    pub context_resolution_ref: ResourceRef,
    pub activity_refs: Vec<ResourceRef>,
    pub return_refs: Vec<ResourceRef>,
    pub evidence_refs: Vec<ResourceRef>,
    pub verification_refs: Vec<ResourceRef>,
}

impl ProvenMethodBasis {
    pub fn matches_method(&self, method: &Method) -> bool {
        method.id == self.method && method.revision.as_ref() == Some(&self.method_revision)
    }
}

/// Promote returned execution evidence into a proof only when the exact Method
/// revision is known and explicit verification passed.
pub fn prove_method(method: &Method, input: MethodProofInput) -> Result<ProvenMethodBasis> {
    method.validate()?;
    let revision = method.revision.clone().ok_or_else(|| {
        AikitError::new(
            "routine.method_revision_required",
            "a Method must carry an exact source revision before it can be proven for Routine use",
        )
        .with("method", method.id.to_string())
    })?;
    if !input.invocation_succeeded {
        return Err(AikitError::new(
            "routine.invocation_not_successful",
            "failed Method execution cannot be promoted into a proven basis",
        ));
    }
    if !input.verification_passed {
        return Err(AikitError::new(
            "routine.verification_required",
            "successful Method invocation is not proof; explicit verification must pass",
        ));
    }
    if input.activity_refs.is_empty()
        || input.return_refs.is_empty()
        || input.evidence_refs.is_empty()
        || input.verification_refs.is_empty()
    {
        return Err(AikitError::new(
            "routine.proof_evidence_incomplete",
            "Method proof requires Activity, Return, Evidence and verification references",
        ));
    }
    Ok(ProvenMethodBasis {
        version: METHOD_PROOF_VERSION.into(),
        method: method.id.clone(),
        method_revision: revision,
        proof_ref: input.proof_ref,
        context_resolution_ref: input.context_resolution_ref,
        activity_refs: input.activity_refs,
        return_refs: input.return_refs,
        evidence_refs: input.evidence_refs,
        verification_refs: input.verification_refs,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum RoutineTrigger {
    Manual,
    Schedule { schedule_ref: String },
    Event { event_ref: String },
    External { trigger_ref: String },
}

impl RoutineTrigger {
    pub fn is_unattended(&self) -> bool {
        !matches!(self, Self::Manual)
    }

    fn validate(&self) -> Result<()> {
        let value = match self {
            Self::Manual => return Ok(()),
            Self::Schedule { schedule_ref } => schedule_ref,
            Self::Event { event_ref } => event_ref,
            Self::External { trigger_ref } => trigger_ref,
        };
        if value.trim().is_empty() {
            return Err(AikitError::new(
                "routine.trigger_ref_empty",
                "Routine trigger reference must be non-empty",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutineAuthority {
    pub authority_ref: ResourceRef,
    /// Exact revision of the owner-supplied authority relation when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<SourceRevision>,
    pub action_refs: Vec<ResourceRef>,
    pub granted: bool,
    pub unattended: bool,
}

impl RoutineAuthority {
    fn validate_for_method(&self, method: &Method) -> Result<()> {
        if self.action_refs.is_empty() {
            return Err(AikitError::new(
                "routine.action_authority_required",
                "Routine authority must name at least one canonical Method Action",
            ));
        }
        if let Some(action) = self
            .action_refs
            .iter()
            .find(|action| !method.actions.contains(action))
        {
            return Err(AikitError::new(
                "routine.action_not_in_method",
                "Routine authority cannot introduce an Action outside the proven Method",
            )
            .with("action", action.to_string())
            .with("method", method.id.to_string()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RoutineState {
    Draft,
    Enabled,
    Disabled,
    StaleProof,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RoutineSchedulerState {
    Planned,
    Active,
    Degraded,
    Unavailable,
}

/// Material/provider observation attached to a semantic Routine. `provider_job_id`
/// may change without changing Routine identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutineSchedulerBinding {
    pub provider: ProviderRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_job_id: Option<String>,
    pub observed_state: RoutineSchedulerState,
}

impl RoutineSchedulerBinding {
    fn validate(&self) -> Result<()> {
        if self
            .provider_job_id
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(AikitError::new(
                "routine.provider_job_id_empty",
                "provider job id must be non-empty when supplied",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Routine {
    pub id: ResourceRef,
    pub source: SourceRef,
    #[serde(default)]
    pub revision: Option<SourceRevision>,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub method: ResourceRef,
    pub method_revision: SourceRevision,
    pub proof: ProvenMethodBasis,
    pub trigger: RoutineTrigger,
    pub authority: RoutineAuthority,
    /// Opaque Central AgentProfile source relation. It is not an AIKit Profile or
    /// a resolved Context merely because it participates in this Routine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_profile_ref: Option<ResourceRef>,
    #[serde(default)]
    pub context_scope_refs: Vec<ResourceRef>,
    pub state: RoutineState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduler: Option<RoutineSchedulerBinding>,
}

impl Routine {
    fn validate_declared_basis(&self, method: &Method) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(AikitError::new(
                "routine.name_empty",
                "Routine name must be non-empty",
            ));
        }
        method.validate()?;
        if self.method != method.id
            || method.revision.as_ref() != Some(&self.method_revision)
            || !self.proof.matches_method(method)
        {
            return Err(AikitError::new(
                "routine.proof_method_mismatch",
                "Routine proof must match the exact Method identity and revision",
            ));
        }
        if self.proof.version != METHOD_PROOF_VERSION {
            return Err(AikitError::new(
                "routine.unsupported_proof_version",
                format!("Routine proof must use the public {METHOD_PROOF_VERSION} contract"),
            ));
        }
        if self.proof.activity_refs.is_empty()
            || self.proof.return_refs.is_empty()
            || self.proof.evidence_refs.is_empty()
            || self.proof.verification_refs.is_empty()
        {
            return Err(AikitError::new(
                "routine.proof_evidence_incomplete",
                "Method proof requires Activity, Return, Evidence and verification references",
            ));
        }
        ensure_unique_refs(&self.proof.activity_refs, "proof Activity")?;
        ensure_unique_refs(&self.proof.return_refs, "proof Return")?;
        ensure_unique_refs(&self.proof.evidence_refs, "proof Evidence")?;
        ensure_unique_refs(&self.proof.verification_refs, "proof verification")?;
        self.trigger.validate()?;
        self.authority.validate_for_method(method)?;
        ensure_unique_refs(&self.authority.action_refs, "authority Action")?;
        ensure_unique_refs(&self.context_scope_refs, "context scope")?;
        if let Some(binding) = &self.scheduler {
            binding.validate()?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: ResourceRef,
        source: SourceRef,
        revision: Option<SourceRevision>,
        name: impl Into<String>,
        description: impl Into<String>,
        method: &Method,
        proof: ProvenMethodBasis,
        trigger: RoutineTrigger,
        authority: RoutineAuthority,
        agent_profile_ref: Option<ResourceRef>,
        context_scope_refs: Vec<ResourceRef>,
    ) -> Result<Self> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(AikitError::new(
                "routine.name_empty",
                "Routine name must be non-empty",
            ));
        }
        method.validate()?;
        if !proof.matches_method(method) {
            return Err(AikitError::new(
                "routine.proof_method_mismatch",
                "Routine proof must match the exact Method identity and revision",
            ));
        }
        trigger.validate()?;
        authority.validate_for_method(method)?;
        Ok(Self {
            id,
            source,
            revision,
            name,
            description: description.into(),
            method: method.id.clone(),
            method_revision: proof.method_revision.clone(),
            proof,
            trigger,
            authority,
            agent_profile_ref,
            context_scope_refs,
            state: RoutineState::Draft,
            scheduler: None,
        })
    }

    pub fn enable(&mut self, method: &Method) -> Result<()> {
        self.ensure_current_proof(method)?;
        if !self.authority.granted {
            return Err(AikitError::new(
                "routine.authority_not_granted",
                "Routine cannot be enabled without current Action authority",
            ));
        }
        if self.trigger.is_unattended() && !self.authority.unattended {
            return Err(AikitError::new(
                "routine.unattended_not_authorised",
                "unattended Routine trigger requires explicit unattended authority",
            ));
        }
        self.state = RoutineState::Enabled;
        Ok(())
    }

    pub fn disable(&mut self) {
        self.state = RoutineState::Disabled;
    }

    /// Record that the semantic trigger occurred. This is evidence only and does
    /// not itself authorise any Action.
    pub fn observe_trigger(&self, observation_ref: ResourceRef) -> RoutineTriggerObservation {
        RoutineTriggerObservation {
            routine: self.id.clone(),
            observation_ref,
            trigger: self.trigger.clone(),
        }
    }

    /// Resolve one trigger observation into an invocation only after re-checking
    /// exact proof and current authority. This closes the `trigger != authority`
    /// boundary at the point of use.
    pub fn authorised_invocation(
        &mut self,
        method: &Method,
        observation: &RoutineTriggerObservation,
    ) -> Result<RoutineInvocation> {
        if observation.routine != self.id {
            return Err(AikitError::new(
                "routine.trigger_observation_mismatch",
                "trigger observation belongs to another Routine",
            ));
        }
        if self.state != RoutineState::Enabled {
            return Err(AikitError::new(
                "routine.not_enabled",
                "Routine trigger cannot invoke Actions unless the Routine is enabled",
            ));
        }
        self.ensure_current_proof(method)?;
        if !self.authority.granted {
            return Err(AikitError::new(
                "routine.authority_revoked",
                "Routine Action authority is no longer granted",
            ));
        }
        if self.trigger.is_unattended() && !self.authority.unattended {
            return Err(AikitError::new(
                "routine.unattended_authority_revoked",
                "Routine no longer has unattended execution authority",
            ));
        }
        Ok(RoutineInvocation {
            routine: self.id.clone(),
            method: self.method.clone(),
            method_revision: self.method_revision.clone(),
            proof_ref: self.proof.proof_ref.clone(),
            context_resolution_ref: self.proof.context_resolution_ref.clone(),
            authority_ref: self.authority.authority_ref.clone(),
            action_refs: self.authority.action_refs.clone(),
            trigger_observation_ref: observation.observation_ref.clone(),
            agent_profile_ref: self.agent_profile_ref.clone(),
            context_scope_refs: self.context_scope_refs.clone(),
        })
    }

    pub fn set_scheduler_binding(&mut self, binding: RoutineSchedulerBinding) -> Result<()> {
        binding.validate()?;
        self.scheduler = Some(binding);
        Ok(())
    }

    /// Replace the exact proof after Method change. Reproof never silently resumes
    /// automation: the Routine returns Disabled and must be explicitly enabled.
    pub fn reprove(&mut self, method: &Method, proof: ProvenMethodBasis) -> Result<()> {
        if !proof.matches_method(method) {
            return Err(AikitError::new(
                "routine.reproof_method_mismatch",
                "replacement proof must match the exact current Method revision",
            ));
        }
        self.method = method.id.clone();
        self.method_revision = proof.method_revision.clone();
        self.proof = proof;
        self.state = RoutineState::Disabled;
        Ok(())
    }

    pub fn resource_record(&self) -> ResourceRecord {
        let mut descriptor = ResourceDescriptor::new(
            self.id.clone(),
            ResourceKind::Routine,
            self.name.clone(),
            self.description.clone(),
        );
        descriptor
            .annotations
            .insert("routine.version".into(), ROUTINE_VERSION.into());
        descriptor
            .annotations
            .insert("routine.source".into(), self.source.to_string());
        descriptor
            .annotations
            .insert("routine.method".into(), self.method.to_string());
        descriptor.annotations.insert(
            "routine.method-revision".into(),
            self.method_revision.to_string(),
        );
        descriptor
            .annotations
            .insert("routine.proof".into(), self.proof.proof_ref.to_string());
        if let Some(revision) = &self.revision {
            descriptor
                .annotations
                .insert("routine.revision".into(), revision.to_string());
        }
        ResourceRecord::new(descriptor)
    }

    pub fn explain(&self) -> RoutineExplanation {
        RoutineExplanation {
            routine: self.id.clone(),
            method: self.method.clone(),
            method_revision: self.method_revision.clone(),
            proof_ref: self.proof.proof_ref.clone(),
            context_resolution_ref: self.proof.context_resolution_ref.clone(),
            evidence_refs: self.proof.evidence_refs.clone(),
            verification_refs: self.proof.verification_refs.clone(),
            authority_ref: self.authority.authority_ref.clone(),
            trigger: self.trigger.clone(),
            scheduler: self.scheduler.clone(),
            agent_profile_ref: self.agent_profile_ref.clone(),
            state: self.state,
        }
    }

    fn ensure_current_proof(&mut self, method: &Method) -> Result<()> {
        if !self.proof.matches_method(method)
            || self.method != method.id
            || method.revision.as_ref() != Some(&self.method_revision)
        {
            self.state = RoutineState::StaleProof;
            return Err(AikitError::new(
                "routine.proof_stale",
                "Routine proof does not match the current exact Method revision",
            )
            .with("method", method.id.to_string()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutineTriggerObservation {
    pub routine: ResourceRef,
    pub observation_ref: ResourceRef,
    pub trigger: RoutineTrigger,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutineInvocation {
    pub routine: ResourceRef,
    pub method: ResourceRef,
    pub method_revision: SourceRevision,
    pub proof_ref: ResourceRef,
    pub context_resolution_ref: ResourceRef,
    pub authority_ref: ResourceRef,
    pub action_refs: Vec<ResourceRef>,
    pub trigger_observation_ref: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_profile_ref: Option<ResourceRef>,
    pub context_scope_refs: Vec<ResourceRef>,
}

/// Caller-established evidence needed to identify one trigger occurrence.
/// AIKit validates the supplied timestamp but never substitutes its own clock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutineInvocationOccurrence {
    pub invocation_ref: ResourceRef,
    pub trigger_observation: RoutineTriggerObservation,
    pub observed_at: String,
}

/// Owner-supplied receipt provenance at the exact point the invocation is
/// resolved. AIKit revalidates it against the supplied Routine; this value does
/// not claim AIKit independently queried the external authority owner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutineAuthorityValidation {
    pub validation_ref: ResourceRef,
    pub authority_ref: ResourceRef,
    pub authority_revision: SourceRevision,
    pub validated_at: String,
    pub granted: bool,
    pub unattended: bool,
    pub standing: RoutineAuthorityStanding,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RoutineAuthorityStanding {
    /// The caller supplied this owner receipt. AIKit checked its internal
    /// relation to the supplied Routine but did not query the authority owner.
    OwnerAttested,
}

/// Optional material delivery evidence. Provider job and restart identity are
/// provenance only; neither changes the semantic Routine or invocation identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutineProviderDelivery {
    pub provider: ProviderRef,
    pub delivery_ref: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_job_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restart_ref: Option<ResourceRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RoutineProofStanding {
    /// The proof matches the exact Routine and Method bodies supplied to this
    /// admission. No canonical source lookup occurred at this boundary.
    CurrentOnSuppliedBasis,
}

/// Public AIKit-owned evidence for one authorised Routine occurrence. This is
/// an invocation request, not proof that any Action ran or completed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutineInvocationEvidence {
    pub schema: String,
    pub owner: String,
    pub invocation_ref: ResourceRef,
    pub routine_ref: ResourceRef,
    pub routine_source: SourceRef,
    pub routine_revision: SourceRevision,
    pub routine_state: RoutineState,
    pub method_ref: ResourceRef,
    pub method_revision: SourceRevision,
    pub proof_ref: ResourceRef,
    pub proof_standing: RoutineProofStanding,
    pub context_resolution_ref: ResourceRef,
    pub trigger_observation_ref: ResourceRef,
    pub trigger: RoutineTrigger,
    pub trigger_observed_at: String,
    pub authority_validation: RoutineAuthorityValidation,
    pub action_refs: Vec<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_profile_ref: Option<ResourceRef>,
    #[serde(default)]
    pub context_scope_refs: Vec<ResourceRef>,
    #[serde(default)]
    pub provider_deliveries: Vec<RoutineProviderDelivery>,
}

/// Structured application input. The Method and Routine bodies are supplied so
/// the native owner can re-run their real validation rather than trusting a
/// consumer-prepared projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutineInvocationAuthorisationRequest {
    pub routine: Routine,
    pub method: Method,
    pub occurrence: RoutineInvocationOccurrence,
    pub authority_validation: RoutineAuthorityValidation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_delivery: Option<RoutineProviderDelivery>,
}

impl RoutineInvocationAuthorisationRequest {
    pub fn authorise(mut self) -> Result<RoutineInvocationEvidence> {
        self.routine.authorised_invocation_evidence(
            &self.method,
            self.occurrence,
            self.authority_validation,
            self.provider_delivery,
        )
    }
}

impl RoutineInvocationEvidence {
    /// Compare the semantic invocation facts while allowing the delivery ledger
    /// to accumulate retry/restart provenance for the same occurrence.
    pub fn has_same_invocation_basis(&self, other: &Self) -> bool {
        let mut left = self.clone();
        let mut right = other.clone();
        left.provider_deliveries.clear();
        right.provider_deliveries.clear();
        left == right
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema != ROUTINE_INVOCATION_EVIDENCE_VERSION || self.owner != "aikit" {
            return Err(AikitError::new(
                "routine.invalid_invocation_evidence",
                "Routine invocation evidence has an unsupported schema or owner",
            ));
        }
        if self.routine_state != RoutineState::Enabled
            || self.proof_standing != RoutineProofStanding::CurrentOnSuppliedBasis
            || !self.authority_validation.granted
            || self.authority_validation.standing != RoutineAuthorityStanding::OwnerAttested
        {
            return Err(AikitError::new(
                "routine.invalid_invocation_evidence",
                "authorised invocation evidence must retain enabled proof, granted authority and owner-attested receipt standing",
            ));
        }
        let observed = parse_timestamp(&self.trigger_observed_at, "trigger observation")?;
        let validated = parse_timestamp(
            &self.authority_validation.validated_at,
            "authority validation",
        )?;
        if validated < observed {
            return Err(AikitError::new(
                "routine.authority_validation_precedes_trigger",
                "current authority validation cannot precede the trigger observation",
            ));
        }
        ensure_unique_refs(&self.action_refs, "Action")?;
        ensure_unique_refs(&self.context_scope_refs, "context scope")?;
        if self.action_refs.is_empty() {
            return Err(AikitError::new(
                "routine.action_authority_required",
                "Routine invocation evidence must name at least one authorised Action",
            ));
        }
        let mut delivery_refs = std::collections::BTreeSet::new();
        for delivery in &self.provider_deliveries {
            if !delivery_refs.insert(&delivery.delivery_ref) {
                return Err(AikitError::new(
                    "routine.duplicate_provider_delivery",
                    "Routine invocation evidence cannot repeat a delivery_ref",
                ));
            }
            if delivery
                .provider_job_id
                .as_deref()
                .is_some_and(|value| value.trim().is_empty())
            {
                return Err(AikitError::new(
                    "routine.provider_job_id_empty",
                    "provider job id must be non-empty when supplied",
                ));
            }
        }
        Ok(())
    }
}

impl Routine {
    /// Resolve one public invocation envelope from explicit owner evidence.
    /// Disabled, stale, or revoked Routines return an error and no envelope.
    pub fn authorised_invocation_evidence(
        &mut self,
        method: &Method,
        occurrence: RoutineInvocationOccurrence,
        authority_validation: RoutineAuthorityValidation,
        provider_delivery: Option<RoutineProviderDelivery>,
    ) -> Result<RoutineInvocationEvidence> {
        // All bodies arrive through serde at the public boundary, so re-run the
        // same invariants enforced by constructors before trusting any field.
        self.validate_declared_basis(method)?;
        occurrence.trigger_observation.trigger.validate()?;
        let observed_at = parse_timestamp(&occurrence.observed_at, "trigger observation")?;
        let validated_at =
            parse_timestamp(&authority_validation.validated_at, "authority validation")?;
        if validated_at < observed_at {
            return Err(AikitError::new(
                "routine.authority_validation_precedes_trigger",
                "current authority validation cannot precede the trigger observation",
            ));
        }
        let routine_revision = self.revision.clone().ok_or_else(|| {
            AikitError::new(
                "routine.source_revision_required",
                "an authorised invocation envelope requires the exact Routine source revision",
            )
            .with("routine", self.id.to_string())
        })?;
        if occurrence.trigger_observation.trigger != self.trigger {
            return Err(AikitError::new(
                "routine.trigger_observation_mismatch",
                "trigger observation does not match the Routine trigger intent",
            ));
        }
        if authority_validation.authority_ref != self.authority.authority_ref {
            return Err(AikitError::new(
                "routine.authority_validation_mismatch",
                "authority validation receipt belongs to another authority relation",
            ));
        }
        if self.authority.revision.as_ref() != Some(&authority_validation.authority_revision) {
            return Err(AikitError::new(
                "routine.authority_validation_revision_mismatch",
                "authority validation revision does not match the exact Routine authority basis",
            ));
        }
        if authority_validation.granted != self.authority.granted
            || authority_validation.unattended != self.authority.unattended
        {
            return Err(AikitError::new(
                "routine.authority_validation_stale",
                "authority validation receipt does not match current Routine authority state",
            ));
        }
        if authority_validation.standing != RoutineAuthorityStanding::OwnerAttested {
            return Err(AikitError::new(
                "routine.authority_validation_standing_unsupported",
                "authority validation must disclose owner-attested receipt standing",
            ));
        }
        let invocation = self.authorised_invocation(method, &occurrence.trigger_observation)?;
        let mut provider_deliveries = Vec::new();
        if let Some(delivery) = provider_delivery {
            validate_provider_delivery(self, &delivery)?;
            provider_deliveries.push(delivery);
        }
        let evidence = RoutineInvocationEvidence {
            schema: ROUTINE_INVOCATION_EVIDENCE_VERSION.into(),
            owner: "aikit".into(),
            invocation_ref: occurrence.invocation_ref,
            routine_ref: invocation.routine,
            routine_source: self.source.clone(),
            routine_revision,
            routine_state: RoutineState::Enabled,
            method_ref: invocation.method,
            method_revision: invocation.method_revision,
            proof_ref: invocation.proof_ref,
            proof_standing: RoutineProofStanding::CurrentOnSuppliedBasis,
            context_resolution_ref: invocation.context_resolution_ref,
            trigger_observation_ref: invocation.trigger_observation_ref,
            trigger: occurrence.trigger_observation.trigger,
            trigger_observed_at: occurrence.observed_at,
            authority_validation,
            action_refs: invocation.action_refs,
            agent_profile_ref: invocation.agent_profile_ref,
            context_scope_refs: invocation.context_scope_refs,
            provider_deliveries,
        };
        evidence.validate()?;
        Ok(evidence)
    }
}

fn parse_timestamp(value: &str, label: &str) -> Result<jiff::Timestamp> {
    value.parse::<jiff::Timestamp>().map_err(|error| {
        AikitError::new(
            "routine.invalid_timestamp",
            format!("{label} time `{value}` is not an RFC 3339 timestamp: {error}"),
        )
    })
}

fn validate_provider_delivery(routine: &Routine, delivery: &RoutineProviderDelivery) -> Result<()> {
    if delivery
        .provider_job_id
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        return Err(AikitError::new(
            "routine.provider_job_id_empty",
            "provider job id must be non-empty when supplied",
        ));
    }
    let binding = routine.scheduler.as_ref().ok_or_else(|| {
        AikitError::new(
            "routine.provider_delivery_without_binding",
            "provider delivery provenance requires a Routine scheduler binding",
        )
    })?;
    if binding.provider != delivery.provider {
        return Err(AikitError::new(
            "routine.provider_delivery_mismatch",
            "provider delivery does not match the Routine scheduler binding",
        ));
    }
    match (
        binding.provider_job_id.as_deref(),
        delivery.provider_job_id.as_deref(),
    ) {
        (Some(binding_job), Some(delivery_job)) if binding_job != delivery_job => {
            return Err(AikitError::new(
                "routine.provider_delivery_mismatch",
                "provider delivery job does not match the current Routine scheduler binding",
            ));
        }
        (Some(_), None) => {
            return Err(AikitError::new(
                "routine.provider_delivery_job_unavailable",
                "provider delivery must retain the scheduler job identity when the binding supplies one",
            ));
        }
        _ => {}
    }
    Ok(())
}

fn ensure_unique_refs(values: &[ResourceRef], label: &str) -> Result<()> {
    let mut refs = std::collections::BTreeSet::new();
    if values.iter().all(|value| refs.insert(value)) {
        Ok(())
    } else {
        Err(AikitError::new(
            "routine.duplicate_invocation_ref",
            format!("Routine invocation evidence contains duplicate {label} refs"),
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutineExplanation {
    pub routine: ResourceRef,
    pub method: ResourceRef,
    pub method_revision: SourceRevision,
    pub proof_ref: ResourceRef,
    pub context_resolution_ref: ResourceRef,
    pub evidence_refs: Vec<ResourceRef>,
    pub verification_refs: Vec<ResourceRef>,
    pub authority_ref: ResourceRef,
    pub trigger: RoutineTrigger,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduler: Option<RoutineSchedulerBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_profile_ref: Option<ResourceRef>,
    pub state: RoutineState,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::method::{Method, MethodSkillRef};

    fn resource(raw: &str) -> ResourceRef {
        ResourceRef::parse(raw).unwrap()
    }

    fn source(raw: &str) -> SourceRef {
        SourceRef::parse(raw).unwrap()
    }

    fn revision(raw: &str) -> SourceRevision {
        SourceRevision::parse(raw).unwrap()
    }

    fn method(rev: Option<&str>) -> Method {
        Method {
            id: resource("method:verified-research"),
            source: source("source:project-method"),
            revision: rev.map(revision),
            name: "Verified research return".into(),
            description: String::new(),
            focus: vec![],
            project_domain: vec![],
            skills: Vec::<MethodSkillRef>::new(),
            actions: vec![resource("action:research")],
            capabilities: vec![],
            context_sources: vec![],
            verification: vec![resource("verification:research")],
            expected_resolve: None,
            expected_return_forms: vec!["evidence-bearing-return".into()],
        }
    }

    fn proof_input(verified: bool) -> MethodProofInput {
        MethodProofInput {
            proof_ref: resource("proof:research:v1"),
            context_resolution_ref: resource("context-resolution:abc123"),
            activity_refs: vec![resource("activity:research:1")],
            return_refs: vec![resource("return:research:1")],
            evidence_refs: vec![resource("evidence:research:1")],
            verification_refs: vec![resource("verification:research:1")],
            invocation_succeeded: true,
            verification_passed: verified,
        }
    }

    fn authority() -> RoutineAuthority {
        RoutineAuthority {
            authority_ref: resource("authority:routine:research"),
            revision: Some(revision("authority-rev-7")),
            action_refs: vec![resource("action:research")],
            granted: true,
            unattended: true,
        }
    }

    fn routine(method: &Method, proof: ProvenMethodBasis) -> Routine {
        Routine::new(
            resource("routine:daily-research"),
            source("source:control:routines"),
            Some(revision("routine-rev-1")),
            "Daily research",
            "",
            method,
            proof,
            RoutineTrigger::Schedule {
                schedule_ref: "schedule:daily".into(),
            },
            authority(),
            Some(resource("agent-profile:researcher")),
            vec![resource("world:personal")],
        )
        .unwrap()
    }

    #[test]
    fn successful_invocation_without_verification_is_not_proof() {
        let method = method(Some("method-rev-1"));
        let error = prove_method(&method, proof_input(false)).unwrap_err();
        assert_eq!(error.code(), "routine.verification_required");
    }

    #[test]
    fn unrevisioned_method_cannot_be_proven_for_automation() {
        let error = prove_method(&method(None), proof_input(true)).unwrap_err();
        assert_eq!(error.code(), "routine.method_revision_required");
    }

    #[test]
    fn verified_method_yields_exact_proven_basis() {
        let method = method(Some("method-rev-1"));
        let proof = prove_method(&method, proof_input(true)).unwrap();
        assert!(proof.matches_method(&method));
        assert_eq!(
            proof.context_resolution_ref.as_str(),
            "context-resolution:abc123"
        );
    }

    #[test]
    fn trigger_observation_does_not_authorise_disabled_routine() {
        let method = method(Some("method-rev-1"));
        let proof = prove_method(&method, proof_input(true)).unwrap();
        let mut routine = routine(&method, proof);
        let observation = routine.observe_trigger(resource("trigger-observation:1"));
        let error = routine
            .authorised_invocation(&method, &observation)
            .unwrap_err();
        assert_eq!(error.code(), "routine.not_enabled");
    }

    #[test]
    fn authority_revoke_blocks_after_trigger_fired() {
        let method = method(Some("method-rev-1"));
        let proof = prove_method(&method, proof_input(true)).unwrap();
        let mut routine = routine(&method, proof);
        routine.enable(&method).unwrap();
        let observation = routine.observe_trigger(resource("trigger-observation:2"));
        routine.authority.granted = false;
        let error = routine
            .authorised_invocation(&method, &observation)
            .unwrap_err();
        assert_eq!(error.code(), "routine.authority_revoked");
    }

    #[test]
    fn scheduler_job_identity_can_change_without_routine_identity_drift() {
        let method = method(Some("method-rev-1"));
        let proof = prove_method(&method, proof_input(true)).unwrap();
        let mut routine = routine(&method, proof);
        let routine_ref = routine.id.clone();
        routine
            .set_scheduler_binding(RoutineSchedulerBinding {
                provider: ProviderRef::parse("provider:systemd").unwrap(),
                provider_job_id: Some("timer-one".into()),
                observed_state: RoutineSchedulerState::Active,
            })
            .unwrap();
        routine
            .set_scheduler_binding(RoutineSchedulerBinding {
                provider: ProviderRef::parse("provider:systemd").unwrap(),
                provider_job_id: Some("timer-two".into()),
                observed_state: RoutineSchedulerState::Active,
            })
            .unwrap();
        assert_eq!(routine.id, routine_ref);
        assert_eq!(
            routine
                .scheduler
                .as_ref()
                .unwrap()
                .provider_job_id
                .as_deref(),
            Some("timer-two")
        );
    }

    #[test]
    fn method_revision_drift_stales_proof() {
        let original = method(Some("method-rev-1"));
        let proof = prove_method(&original, proof_input(true)).unwrap();
        let mut routine = routine(&original, proof);
        routine.enable(&original).unwrap();
        let changed = method(Some("method-rev-2"));
        let observation = routine.observe_trigger(resource("trigger-observation:3"));
        let error = routine
            .authorised_invocation(&changed, &observation)
            .unwrap_err();
        assert_eq!(error.code(), "routine.proof_stale");
        assert_eq!(routine.state, RoutineState::StaleProof);
    }

    #[test]
    fn reproof_restores_basis_but_requires_explicit_reenable() {
        let original = method(Some("method-rev-1"));
        let proof = prove_method(&original, proof_input(true)).unwrap();
        let mut routine = routine(&original, proof);
        routine.enable(&original).unwrap();

        let changed = method(Some("method-rev-2"));
        let mut input = proof_input(true);
        input.proof_ref = resource("proof:research:v2");
        let replacement = prove_method(&changed, input).unwrap();
        routine.reprove(&changed, replacement).unwrap();
        assert_eq!(routine.state, RoutineState::Disabled);
        routine.enable(&changed).unwrap();
        assert_eq!(routine.state, RoutineState::Enabled);
    }

    #[test]
    fn agent_profile_is_preserved_as_source_relation_not_context_identity() {
        let method = method(Some("method-rev-1"));
        let proof = prove_method(&method, proof_input(true)).unwrap();
        let routine = routine(&method, proof);
        let explanation = routine.explain();
        assert_eq!(
            explanation.agent_profile_ref.as_ref().unwrap().as_str(),
            "agent-profile:researcher"
        );
        assert_eq!(
            explanation.context_resolution_ref.as_str(),
            "context-resolution:abc123"
        );
    }

    #[test]
    fn routine_is_a_distinct_indexed_resource_kind() {
        let method = method(Some("method-rev-1"));
        let proof = prove_method(&method, proof_input(true)).unwrap();
        let routine = routine(&method, proof);
        let record = routine.resource_record();
        assert_eq!(record.descriptor.kind, ResourceKind::Routine);
        assert_eq!(record.descriptor.id.as_str(), "routine:daily-research");
    }

    #[test]
    fn explanation_carries_proof_authority_trigger_and_provider_chain() {
        let method = method(Some("method-rev-1"));
        let proof = prove_method(&method, proof_input(true)).unwrap();
        let mut routine = routine(&method, proof);
        routine
            .set_scheduler_binding(RoutineSchedulerBinding {
                provider: ProviderRef::parse("provider:cron").unwrap(),
                provider_job_id: Some("job-42".into()),
                observed_state: RoutineSchedulerState::Planned,
            })
            .unwrap();
        let explanation = routine.explain();
        assert_eq!(explanation.method.as_str(), "method:verified-research");
        assert_eq!(explanation.proof_ref.as_str(), "proof:research:v1");
        assert_eq!(
            explanation.authority_ref.as_str(),
            "authority:routine:research"
        );
        assert_eq!(
            explanation.scheduler.unwrap().provider.as_str(),
            "provider:cron"
        );
    }

    fn occurrence(routine: &Routine, suffix: &str) -> RoutineInvocationOccurrence {
        RoutineInvocationOccurrence {
            invocation_ref: resource(&format!("routine-invocation:{suffix}")),
            trigger_observation: routine
                .observe_trigger(resource(&format!("trigger-observation:{suffix}"))),
            observed_at: "2026-09-09T09:30:00Z".into(),
        }
    }

    fn validation(routine: &Routine) -> RoutineAuthorityValidation {
        RoutineAuthorityValidation {
            validation_ref: resource("authority-validation:research:1"),
            authority_ref: routine.authority.authority_ref.clone(),
            authority_revision: revision("authority-rev-7"),
            validated_at: "2026-09-09T09:30:01Z".into(),
            granted: routine.authority.granted,
            unattended: routine.authority.unattended,
            standing: RoutineAuthorityStanding::OwnerAttested,
        }
    }

    #[test]
    fn enabled_current_routine_yields_exact_public_invocation_evidence() {
        let method = method(Some("method-rev-1"));
        let proof = prove_method(&method, proof_input(true)).unwrap();
        let mut routine = routine(&method, proof);
        routine.enable(&method).unwrap();
        let occurrence = occurrence(&routine, "one");
        let authority = validation(&routine);
        let evidence = routine
            .authorised_invocation_evidence(&method, occurrence, authority, None)
            .unwrap();

        assert_eq!(evidence.schema, ROUTINE_INVOCATION_EVIDENCE_VERSION);
        assert_eq!(evidence.owner, "aikit");
        assert_eq!(evidence.routine_revision.as_str(), "routine-rev-1");
        assert_eq!(evidence.method_revision.as_str(), "method-rev-1");
        assert_eq!(
            evidence.proof_standing,
            RoutineProofStanding::CurrentOnSuppliedBasis
        );
        assert!(evidence.provider_deliveries.is_empty());
    }

    #[test]
    fn manual_invocation_needs_no_scheduler_or_provider_evidence() {
        let method = method(Some("method-rev-1"));
        let proof = prove_method(&method, proof_input(true)).unwrap();
        let mut routine = Routine::new(
            resource("routine:manual-research"),
            source("source:control:routines"),
            Some(revision("routine-rev-manual")),
            "Manual research",
            "",
            &method,
            proof,
            RoutineTrigger::Manual,
            RoutineAuthority {
                unattended: false,
                ..authority()
            },
            None,
            vec![],
        )
        .unwrap();
        routine.enable(&method).unwrap();
        let occurrence = occurrence(&routine, "manual");
        let authority = validation(&routine);
        let evidence = routine
            .authorised_invocation_evidence(&method, occurrence, authority, None)
            .unwrap();
        assert_eq!(evidence.trigger, RoutineTrigger::Manual);
        assert!(evidence.provider_deliveries.is_empty());
    }

    #[test]
    fn false_or_stale_authority_validation_yields_no_envelope() {
        let method = method(Some("method-rev-1"));
        let proof = prove_method(&method, proof_input(true)).unwrap();
        let mut routine = routine(&method, proof);
        routine.enable(&method).unwrap();
        let occurrence = occurrence(&routine, "denied");
        let mut authority = validation(&routine);
        authority.granted = false;
        let error = routine
            .authorised_invocation_evidence(&method, occurrence, authority, None)
            .unwrap_err();
        assert_eq!(error.code(), "routine.authority_validation_stale");
    }

    #[test]
    fn envelope_rejects_guessed_time() {
        let method = method(Some("method-rev-1"));
        let proof = prove_method(&method, proof_input(true)).unwrap();
        let mut routine = routine(&method, proof);
        routine.enable(&method).unwrap();
        let mut occurrence = occurrence(&routine, "guessed-time");
        let authority = validation(&routine);
        occurrence.observed_at = "now".into();
        let error = routine
            .authorised_invocation_evidence(&method, occurrence, authority, None)
            .unwrap_err();
        assert_eq!(error.code(), "routine.invalid_timestamp");
    }

    #[test]
    fn envelope_requires_exact_routine_source_revision() {
        let method = method(Some("method-rev-1"));
        let proof = prove_method(&method, proof_input(true)).unwrap();
        let mut routine = routine(&method, proof);
        routine.enable(&method).unwrap();
        routine.revision = None;
        let occurrence = occurrence(&routine, "missing-revision");
        let authority = validation(&routine);
        let error = routine
            .authorised_invocation_evidence(&method, occurrence, authority, None)
            .unwrap_err();
        assert_eq!(error.code(), "routine.source_revision_required");
    }

    #[test]
    fn provider_delivery_must_match_an_actual_binding() {
        let method = method(Some("method-rev-1"));
        let proof = prove_method(&method, proof_input(true)).unwrap();
        let mut routine = routine(&method, proof);
        routine.enable(&method).unwrap();
        let occurrence = occurrence(&routine, "provider");
        let authority = validation(&routine);
        let delivery = RoutineProviderDelivery {
            provider: ProviderRef::parse("provider:cron").unwrap(),
            delivery_ref: resource("provider-delivery:cron:1"),
            provider_job_id: Some("job-1".into()),
            restart_ref: None,
        };
        let error = routine
            .authorised_invocation_evidence(&method, occurrence, authority, Some(delivery))
            .unwrap_err();
        assert_eq!(error.code(), "routine.provider_delivery_without_binding");
    }
}
