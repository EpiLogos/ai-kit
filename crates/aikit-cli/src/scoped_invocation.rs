//! Source-qualified Resolve -> one native AIKit Action -> actual invocation.
//!
//! Factory owns workflow/multi-agent execution. This aperture is deliberately
//! narrower: it admits one canonical contextual Action, revalidates the original
//! provider scope at the effect point, then delegates process policy/trust and
//! execution to the existing `AikitApplication::run` path.

use std::time::{SystemTime, UNIX_EPOCH};

use aikit_core::context_resolution::{ContextResolution, RequestedActors};
use aikit_core::resource::operative_scope::invocation::{
    ScopedActionAttemptEvidence, ScopedActionAttemptOutcome, ScopedActionAuthority,
    ScopedActionInput, ScopedActionInvocation, ScopedActionReturnEvidence, ScopedReturnArrival,
};
use aikit_core::resource::operative_scope::{
    compose_scoped_context, ScopeAwareOperativeProvider, ScopedContextResolution,
    ScopedResolveExpression,
};
use aikit_core::resource::{
    OwnerRef, ResourceDescriptor, ResourceKind, ResourceRecord, ResourceRef, ResourceSearchIndex,
    ResourceSource, SourceAuthority, SourceRef, SourceState,
};
use aikit_core::{AikitError, Result};
use aikit_tui::project_world_service::{context_resolution_from_resources, resource_index};

use crate::app::{AikitApplication, RunHandle, RunRequest, Service};

pub const NATIVE_CAPABILITY_RUN_ACTION: &str = "action/capability/run";

#[derive(Debug, Clone)]
pub struct ScopedRunRequest {
    pub expression: ScopedResolveExpression,
    pub subject: ResourceRef,
    pub authority: ScopedActionAuthority,
    pub input: ScopedActionInput,
    pub args: Vec<String>,
    pub confirmed: bool,
    pub attempt_ordinal: u32,
}

#[derive(Debug, Clone)]
pub enum ScopedRunOutcome {
    Completed {
        invocation: Box<ScopedActionInvocation>,
        attempt: ScopedActionAttemptEvidence,
        returned: ScopedActionReturnEvidence,
        run: RunHandle,
    },
    Failed {
        invocation: ScopedActionInvocation,
        attempt: ScopedActionAttemptEvidence,
        error: AikitError,
    },
}

pub trait ScopedActionApplication {
    fn invoke_scoped_action<P: ScopeAwareOperativeProvider>(
        &mut self,
        request: ScopedRunRequest,
        provider: &P,
    ) -> Result<ScopedRunOutcome>;
}

impl ScopedActionApplication for Service {
    fn invoke_scoped_action<P: ScopeAwareOperativeProvider>(
        &mut self,
        request: ScopedRunRequest,
        provider: &P,
    ) -> Result<ScopedRunOutcome> {
        let (resources, context) = invocation_context(self)?;
        let scoped =
            compose_scoped_context(&request.expression, &resources, &context, 128, provider)?;
        let action_ref = ResourceRef::parse(NATIVE_CAPABILITY_RUN_ACTION)?;
        let candidate = scoped.action(&action_ref, &resources)?;

        // Rebuild the native resource/context join immediately before the effect.
        // A source, Project or composition change between Resolve and launch is a
        // refusal, not something a fresh QL reading may silently retarget.
        let (_, current) = invocation_context(self)?;
        scoped.revalidate(&current, provider)?;

        let invocation = ScopedActionInvocation::admit(
            &scoped,
            &candidate,
            request.subject.clone(),
            request.authority,
            request.input,
        )?;
        let started = now_unix_ms()?;
        let run = self.run(RunRequest {
            name: request.subject.to_string(),
            args: request.args,
            export: None,
            confirmed: request.confirmed,
        });
        let finished = now_unix_ms()?;

        match run {
            Ok(run) => {
                let result_digest = digest_run(&run);
                let attempt = ScopedActionAttemptEvidence::finish(
                    &invocation,
                    request.attempt_ordinal,
                    started,
                    finished,
                    ScopedActionAttemptOutcome::Completed {
                        status: run.report.status,
                        detached: run.report.detached,
                        result_digest: result_digest.clone(),
                    },
                )?;
                // Completion standing is deliberately independent of the act.
                // If the provider moved while the process ran, the Return keeps
                // both the admitted scopes and the later observation.
                let completion = invocation_context(self)
                    .map(|(_, context)| scoped.completion_observations(&context, provider))
                    .unwrap_or_else(|error| {
                        scoped
                            .observations()
                            .iter()
                            .map(|previous| aikit_core::resource::operative_scope::ObservedExpressionScope {
                                scope: previous.scope.clone(),
                                observation: aikit_core::resource::operative_scope::ScopeObservation::Unavailable {
                                    reason: error.to_string(),
                                },
                            })
                            .collect()
                    });
                let returned = ScopedActionReturnEvidence::observe(
                    &invocation,
                    &attempt,
                    completion,
                    ScopedReturnArrival {
                        observed_at_unix_ms: now_unix_ms()?,
                        producer_sequence: None,
                        delivery_sequence: 1,
                    },
                    Some(result_digest),
                    vec![ResourceRef::parse(format!(
                        "evidence/run/{}/{}",
                        run.capsule, attempt.ordinal
                    ))?],
                )?;
                Ok(ScopedRunOutcome::Completed {
                    invocation: Box::new(invocation),
                    attempt,
                    returned,
                    run,
                })
            }
            Err(error) => {
                let attempt = ScopedActionAttemptEvidence::finish(
                    &invocation,
                    request.attempt_ordinal,
                    started,
                    finished,
                    ScopedActionAttemptOutcome::Failed {
                        code: error.code().into(),
                        message: error.message().into(),
                    },
                )?;
                Ok(ScopedRunOutcome::Failed {
                    invocation,
                    attempt,
                    error,
                })
            }
        }
    }
}

fn invocation_context(service: &Service) -> Result<(ResourceSearchIndex, ContextResolution)> {
    let mut resources = resource_index(service)?;
    resources.insert_resource(native_run_action(), Vec::new());
    let context =
        context_resolution_from_resources(service, RequestedActors::default(), &resources)?;
    Ok((resources, context))
}

fn native_run_action() -> ResourceRecord {
    let mut descriptor = ResourceDescriptor::new(
        ResourceRef::parse(NATIVE_CAPABILITY_RUN_ACTION)
            .expect("static native Action reference must be valid"),
        ResourceKind::Action,
        "Run capability",
        "invoke one package-backed capability through the existing AIKit policy/trust runner",
    );
    descriptor.owner = Some(
        OwnerRef::parse("aikit/application-service")
            .expect("static native Action owner reference must be valid"),
    );
    descriptor.sources.push(ResourceSource {
        source: SourceRef::parse("source/aikit/application-service")
            .expect("static native Action source reference must be valid"),
        authority: Some(SourceAuthority::Authored),
        revision: None,
        locator: None,
        state: SourceState::Available,
    });
    descriptor.annotations.insert(
        "action.expected-return-forms".into(),
        "run-result,run-failure".into(),
    );
    ResourceRecord::new(descriptor)
}

fn digest_run(run: &RunHandle) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"aikit.scoped-run-result/v1");
    hasher.update(&[0]);
    hasher.update(run.capsule.to_string().as_bytes());
    hasher.update(&[0]);
    hasher.update(run.report.status.to_string().as_bytes());
    hasher.update(&[0]);
    hasher.update(if run.report.detached {
        b"detached"
    } else {
        b"joined"
    });
    for line in &run.report.output {
        hasher.update(&[0]);
        hasher.update(line.as_bytes());
    }
    hasher.finalize().to_hex().to_string()
}

fn now_unix_ms() -> Result<u64> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| AikitError::new("run.clock_before_epoch", error.to_string()))?
        .as_millis();
    u64::try_from(millis)
        .map_err(|_| AikitError::new("run.clock_overflow", "system time exceeds u64 milliseconds"))
}

/// Kept as a separate helper so tests and non-QL callers can exercise the exact
/// same native run result digest without manufacturing provider semantics.
pub fn run_result_digest(run: &RunHandle) -> String {
    digest_run(run)
}

/// Exposes the context assembly to acceptance tests without publishing another
/// Context store. Every call reconstructs it from the current native owners.
pub fn current_scoped_invocation_context(
    service: &Service,
) -> Result<(ResourceSearchIndex, ContextResolution)> {
    invocation_context(service)
}

#[allow(dead_code)]
fn _type_check_scoped_context(_: &ScopedContextResolution) {}
