//! Recognition -> `= name` -> native Skill promotion -> Method proof.
//!
//! `=` remains an operative relation only. This module never writes a Method
//! record or mutates the Resource index. Recognised material enters AIKit through
//! the existing Inbox scanner/dedup pipeline, is promoted into the ordinary
//! personal registry as a Skill, and only then becomes Method-classified by the
//! existing `METHOD:` description convention.

use std::fs;

use aikit_core::method::{method_payload, resolve_method, Method};
use aikit_core::resource::operative_scope::invocation::{
    ScopedActionAttemptEvidence, ScopedActionAttemptOutcome, ScopedActionInvocation,
    ScopedActionReturnEvidence,
};
use aikit_core::resource::{
    prove_method, MethodProofInput, ProvenMethodBasis, RelationOp, ResolveExpression, ResourceRef,
    SourceRef, SourceRevision,
};
use aikit_core::{AikitError, Capsule, CapsuleId, Kind, Maturity, Result};
use aikit_store::inbox::{Capture, Inbox, PromotedCapsule, PromotionEdits};
use aikit_store::Index;
use aikit_tui::backend::PaletteBackend;

use crate::app::Service;
use crate::scoped_invocation::{current_scoped_invocation_context, NATIVE_CAPABILITY_RUN_ACTION};

#[derive(Debug, Clone)]
pub struct RecognisedPraxisRequest {
    /// Human naming act. It is retained as the right-hand side of the native
    /// `=` relation but does not itself create any resource.
    pub name: String,
    /// Skill material to place through the existing capture/promotion path.
    pub skill_body: String,
    /// Optional exact Skill id. Without one, AIKit derives a validated personal
    /// `skill/recognised/<slug>` id from the human name before promotion.
    pub skill_id: Option<CapsuleId>,
    pub invocation: ScopedActionInvocation,
    pub attempt: ScopedActionAttemptEvidence,
    pub returned: ScopedActionReturnEvidence,
    /// Evidence that the actor/human recognised this performed praxis as worth
    /// naming. These are evidence references, never a source of Resource identity.
    pub recognition_refs: Vec<ResourceRef>,
    /// Explicit verification required before a successful performance may become
    /// a ProvenMethodBasis for later reuse.
    pub verification_refs: Vec<ResourceRef>,
    pub verification_passed: bool,
}

#[derive(Debug, Clone)]
pub struct RecognisedPraxisReceipt {
    pub naming_expression: ResolveExpression,
    pub promoted: PromotedCapsule,
    pub method: Method,
    pub proof: ProvenMethodBasis,
    pub candidate_ref: ResourceRef,
    pub source_revision: SourceRevision,
}

pub trait RecognisedPraxisApplication {
    fn recognise_praxis(
        &mut self,
        request: RecognisedPraxisRequest,
    ) -> Result<RecognisedPraxisReceipt>;
}

impl RecognisedPraxisApplication for Service {
    fn recognise_praxis(
        &mut self,
        request: RecognisedPraxisRequest,
    ) -> Result<RecognisedPraxisReceipt> {
        validate_execution_basis(&request)?;
        let name = request.name.trim();
        if name.is_empty() {
            return Err(AikitError::new(
                "praxis.recognition_name_empty",
                "recognised praxis requires a non-empty human name",
            ));
        }
        if request.skill_body.trim().is_empty() {
            return Err(AikitError::new(
                "praxis.recognition_body_empty",
                "recognised praxis requires non-empty Skill material",
            ));
        }
        if request.recognition_refs.is_empty() {
            return Err(AikitError::new(
                "praxis.recognition_evidence_required",
                "Recognition must carry at least one native evidence reference",
            ));
        }
        if request.verification_refs.is_empty() {
            return Err(AikitError::new(
                "routine.proof_evidence_incomplete",
                "recognised Method proof requires explicit verification evidence",
            ));
        }

        let home = self.home().clone();
        let index = Index::open(&home.database())?;
        let inbox = Inbox::new(&home, &index);

        let mut capture = Capture::new(name, request.skill_body.clone());
        capture.suggested_kind = Some(Kind::Skill);
        capture.project_root = self.context().project_root.clone();
        let captured = inbox.capture(capture)?;
        if captured.duplicate_of.is_some() {
            return Err(AikitError::new(
                "praxis.recognition_duplicate",
                "the recognised Skill material already exists in the Inbox; reuse or explicitly revise that source instead of silently minting another Method name",
            )
            .with("candidate", captured.candidate.id));
        }

        let skill_id = match request.skill_id {
            Some(id) => {
                if id.kind() != Kind::Skill {
                    return Err(AikitError::new(
                        "praxis.recognition_id_not_skill",
                        "recognised praxis must be promoted through an existing Skill identity",
                    )
                    .with("id", id.to_string()));
                }
                id
            }
            None => CapsuleId::parse(&format!("skill/recognised/{}", slug(name)))?,
        };
        let description = format!("METHOD: {name}");
        let edits = PromotionEdits::new(skill_id.clone(), description)
            .with_name(name)
            .with_tags(["recognised-praxis"])
            .with_maturity(Maturity::Draft);
        let promoted = inbox.promote(&captured.candidate.id, &edits, &home.registry("personal"))?;

        // Re-read what the native writer actually materialised. The naming act is
        // not accepted as proof that a Skill or Method now exists.
        let manifest = fs::read_to_string(&promoted.manifest_path).map_err(|error| {
            AikitError::new(
                "praxis.promoted_manifest_unreadable",
                format!("could not read promoted Skill manifest: {error}"),
            )
        })?;
        let body = fs::read_to_string(&promoted.payload_path).map_err(|error| {
            AikitError::new(
                "praxis.promoted_payload_unreadable",
                format!("could not read promoted Skill payload: {error}"),
            )
        })?;
        let capsule = Capsule::from_toml_str(&manifest)?;
        if capsule.id != skill_id || capsule.kind != Kind::Skill {
            return Err(AikitError::new(
                "praxis.promoted_skill_identity_mismatch",
                "native promotion did not materialise the exact named Skill identity",
            ));
        }
        if method_payload(&capsule.description).is_none() {
            return Err(AikitError::new(
                "praxis.promoted_skill_not_method",
                "promoted Skill is not METHOD:-classified after native materialisation",
            ));
        }

        let source_revision = promoted_revision(&manifest, &body)?;
        let method_id = ResourceRef::parse(skill_id.to_string())?;
        let naming_expression = ResolveExpression::Binary {
            op: RelationOp::Express,
            left: Box::new(ResolveExpression::subject(
                request.invocation.subject.to_string(),
            )),
            right: Box::new(ResolveExpression::subject(name)),
        };
        let method = Method {
            id: method_id.clone(),
            source: SourceRef::parse(format!("source/aikit/personal-registry/{skill_id}"))?,
            revision: Some(source_revision.clone()),
            name: name.into(),
            description: method_payload(&capsule.description)
                .unwrap_or_default()
                .into(),
            focus: Vec::new(),
            project_domain: Vec::new(),
            skills: Vec::new(),
            actions: vec![ResourceRef::parse(NATIVE_CAPABILITY_RUN_ACTION)?],
            capabilities: vec![request.invocation.subject.clone()],
            context_sources: Vec::new(),
            verification: Vec::new(),
            expected_resolve: Some(ResolveExpression::Binary {
                op: RelationOp::Relate,
                left: Box::new(ResolveExpression::subject(
                    request.invocation.subject.to_string(),
                )),
                right: Box::new(ResolveExpression::subject(NATIVE_CAPABILITY_RUN_ACTION)),
            }),
            expected_return_forms: vec!["run-result".into(), "return-evidence".into()],
        };
        method.validate()?;

        let context_resolution_ref =
            ResourceRef::parse(request.invocation.resolve_path_identity.clone())?;
        let proof_ref = proof_ref(
            &method_id,
            &source_revision,
            &request.returned.return_ref,
            &request.recognition_refs,
            &request.verification_refs,
        )?;
        let mut evidence_refs = request.returned.evidence.clone();
        evidence_refs.extend(request.recognition_refs.iter().cloned());
        dedup_refs(&mut evidence_refs);
        let proof = prove_method(
            &method,
            MethodProofInput {
                proof_ref,
                context_resolution_ref,
                activity_refs: vec![request.attempt.attempt_ref.clone()],
                return_refs: vec![request.returned.return_ref.clone()],
                evidence_refs,
                verification_refs: request.verification_refs,
                invocation_succeeded: true,
                verification_passed: request.verification_passed,
            },
        )?;

        // Native promotion is not enough. Refresh the application's ordinary
        // catalogue and prove that the exact promoted Skill identity now resolves
        // as METHOD:-classified together with the Action/capability it retained.
        self.refresh()?;
        let (resources, _) = current_scoped_invocation_context(self)?;
        let resolution = resolve_method(&method, &resources)?;
        if !resolution.is_complete() {
            return Err(AikitError::new(
                "praxis.promoted_method_not_reusable",
                format!(
                    "promoted Skill did not re-resolve as a complete Method: {}",
                    resolution.warnings.join("; ")
                ),
            ));
        }

        Ok(RecognisedPraxisReceipt {
            naming_expression,
            promoted,
            method,
            proof,
            candidate_ref: ResourceRef::parse(format!("inbox/{}", captured.candidate.id))?,
            source_revision,
        })
    }
}

fn validate_execution_basis(request: &RecognisedPraxisRequest) -> Result<()> {
    if request.attempt.invocation_ref != request.invocation.invocation_ref
        || request.returned.invocation_ref != request.invocation.invocation_ref
        || request.returned.attempt_ref != request.attempt.attempt_ref
    {
        return Err(AikitError::new(
            "praxis.recognition_execution_mismatch",
            "Recognition must name one exact invocation/attempt/Return chain",
        ));
    }
    match &request.attempt.outcome {
        ScopedActionAttemptOutcome::Completed { status: 0, .. } => Ok(()),
        _ => Err(AikitError::new(
            "praxis.recognition_unsuccessful_activity",
            "failed or cancelled Activity cannot be proven as a recognised Method",
        )),
    }
}

fn promoted_revision(manifest: &str, body: &str) -> Result<SourceRevision> {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"aikit.recognised-skill-source/v1");
    hasher.update(&[0]);
    hasher.update(manifest.as_bytes());
    hasher.update(&[0]);
    hasher.update(body.as_bytes());
    SourceRevision::parse(format!("blake3:{}", hasher.finalize().to_hex()))
}

fn proof_ref(
    method: &ResourceRef,
    revision: &SourceRevision,
    returned: &ResourceRef,
    recognition: &[ResourceRef],
    verification: &[ResourceRef],
) -> Result<ResourceRef> {
    let encoded = serde_json::to_vec(&(method, revision, returned, recognition, verification))
        .map_err(|error| AikitError::new("praxis.proof_encoding", error.to_string()))?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"aikit.recognised-method-proof/v1");
    hasher.update(&[0]);
    hasher.update(&encoded);
    ResourceRef::parse(format!("proof/method/{}", hasher.finalize().to_hex()))
}

fn slug(name: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "praxis".into()
    } else {
        out
    }
}

fn dedup_refs(refs: &mut Vec<ResourceRef>) {
    refs.sort();
    refs.dedup();
}
