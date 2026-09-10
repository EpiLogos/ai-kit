mod common;

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use aikit_core::project::{ProjectBinding, ProjectBindingLocator, ProjectConstituentRef};
use aikit_core::resource::{FactoryInteropView, MemoryResourceIndex, ResourceRef};
use aikit_core::scope::ScopeKind;
use aikit_core::{
    compose_context_resolution, project_context_with_ql, QlAttachment, QlClientSubject,
    QlInputLimits, QlInputRefRevision, QlMode, QlOperation, QlProjectionRequest, QlProvenance,
    QlProviderCapabilities, QlProviderClass, QlProviderClient, QlProviderFailure, QlProviderHealth,
    QlProviderRef, QlProviderState, QlReading, QlRefractionRequest, QlResultClass, QlTargetView,
    QL_MEF_REGISTRY_VERSION, QL_OUTPUT_SCHEMA_VERSION, QL_PROVENANCE_SCHEMA_VERSION,
};
use serde_json::json;

use common::{layer_using, profile, script, Fixture};

fn context() -> aikit_core::ContextResolution {
    let layers = vec![layer_using(ScopeKind::Project, &["profile/code/base"])];
    let fixture = Fixture::new(vec![script("script/test/check")])
        .with_profiles(vec![profile(
            "profile/code/base",
            &["script/test/check"],
            &[],
        )])
        .with_layers(layers.clone());
    let deterministic = fixture.resolve().expect("deterministic resolution");
    let factory = FactoryInteropView::from_fixture_json(include_str!(
        "fixtures/factory-interop-v1.json"
    ))
    .expect("Factory CR-001 fixture");
    let binding = ProjectBinding::new(
        factory.project_ref().expect("Factory ProjectRef"),
        ProjectConstituentRef::parse("constituent:source").unwrap(),
        ProjectBindingLocator::LocalDirectory {
            path: PathBuf::from("/work/factory"),
        },
    );
    compose_context_resolution(
        &deterministic,
        binding,
        &layers,
        &MemoryResourceIndex::default(),
        aikit_core::RequestedActors::default(),
    )
}

fn subject() -> QlClientSubject {
    QlClientSubject::new(
        ResourceRef::parse("factory:claim:c-1").unwrap(),
        Some("sha256:claim-c-1-r1".into()),
    )
}

fn projection(mode: QlMode) -> QlProjectionRequest {
    QlProjectionRequest {
        mode,
        refractions: vec![QlRefractionRequest::new(subject(), "mef:lens:L3@1")],
    }
}

struct FixtureQl {
    state: QlProviderState,
    fail: bool,
    drift_target: bool,
    drift_revision: bool,
    calls: AtomicUsize,
}

impl FixtureQl {
    fn new(state: QlProviderState) -> Self {
        Self {
            state,
            fail: false,
            drift_target: false,
            drift_revision: false,
            calls: AtomicUsize::new(0),
        }
    }

    fn failing() -> Self {
        Self {
            fail: true,
            ..Self::new(QlProviderState::Available)
        }
    }

    fn drift_target() -> Self {
        Self {
            drift_target: true,
            ..Self::new(QlProviderState::Available)
        }
    }

    fn drift_revision() -> Self {
        Self {
            drift_revision: true,
            ..Self::new(QlProviderState::Available)
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn provider_ref() -> QlProviderRef {
        QlProviderRef {
            provider: "fixture-ql".into(),
            version: "0.1.0".into(),
        }
    }
}

impl QlProviderClient for FixtureQl {
    fn capabilities(&self) -> QlProviderCapabilities {
        QlProviderCapabilities {
            provider: Self::provider_ref(),
            health: QlProviderHealth {
                state: self.state,
                detail: (self.state == QlProviderState::Degraded)
                    .then(|| "one semantic source unavailable".into()),
            },
            classes: vec![QlProviderClass::SemanticRefraction],
            supported_forms: vec![
                "qlform:sixfold@1".into(),
                "qlform:four-plus-two@1".into(),
                "qlform:direct-conjugate@1".into(),
            ],
            supported_lenses: vec!["mef:lens:L3@1".into()],
            operations: vec![QlOperation::Capabilities, QlOperation::Refract],
            extension_namespaces: Vec::new(),
            deterministic_operations: vec![QlOperation::Capabilities],
            input_limits: QlInputLimits {
                max_relation_subjects: 4,
                max_synthesis_readings: 8,
            },
            output_schema_versions: vec![QL_OUTPUT_SCHEMA_VERSION.into()],
        }
    }

    fn refract(
        &self,
        request: &QlRefractionRequest,
    ) -> std::result::Result<QlReading, QlProviderFailure> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail {
            return Err(QlProviderFailure::new(
                "fixture.semantic_failure",
                "fixture semantic failure",
            ));
        }

        let target = if self.drift_target {
            ResourceRef::parse("factory:claim:different").unwrap()
        } else {
            request.subject.reference.clone()
        };
        let revision = if self.drift_revision {
            Some("sha256:different-revision".into())
        } else {
            request.subject.revision.clone()
        };

        Ok(QlReading {
            id: ResourceRef::parse("fixture:reading:q4").unwrap(),
            target: QlTargetView {
                subject: target,
                subject_type: request.subject.subject_type.clone(),
                frame_ref: request.subject.frame_ref.clone(),
                context_refs: request.subject.context_refs.clone(),
            },
            operation: QlOperation::Refract.as_str().into(),
            ql_form: request.frame.clone(),
            address: None,
            lens: Some(request.lens.clone()),
            reading: json!({
                "text": "fixture semantic disclosure",
                "status": "partial",
                "confidence_per_mille": 730
            }),
            evidence_refs: vec![ResourceRef::parse("fixture:source:q4").unwrap()],
            warnings: Vec::new(),
            provenance: QlProvenance {
                schema_version: QL_PROVENANCE_SCHEMA_VERSION.into(),
                mef_registry_version: QL_MEF_REGISTRY_VERSION.into(),
                provider: Self::provider_ref(),
                operation: QlOperation::Refract.as_str().into(),
                input_refs: vec![QlInputRefRevision {
                    reference: request.subject.reference.clone(),
                    revision,
                }],
                model: Some("fixture-semantic-model".into()),
                config_ref: Some(ResourceRef::parse("fixture:config:q4").unwrap()),
                result_class: QlResultClass::SemanticStochastic,
                warnings: Vec::new(),
            },
        })
    }
}

#[test]
fn disabled_ql_is_exact_noql_parity_and_never_calls_provider() {
    let context = context();
    let provider = FixtureQl::new(QlProviderState::Available);
    let projected = project_context_with_ql(&context, Some(&provider), projection(QlMode::Disabled))
        .expect("disabled QL");

    assert_eq!(projected.context, context);
    assert_eq!(provider.calls(), 0);
    assert_eq!(projected.discovery.mode, QlMode::Disabled);
    assert!(matches!(
        projected.refractions[0].attachment,
        QlAttachment::Disabled
    ));
}

#[test]
fn optional_no_provider_preserves_context_and_reports_absence() {
    let context = context();
    let projected = project_context_with_ql(&context, None, projection(QlMode::Optional))
        .expect("optional QL absence is non-fatal");

    assert_eq!(projected.context, context);
    assert_eq!(projected.discovery.health.state, QlProviderState::Absent);
    assert!(projected.discovery.capabilities.is_none());
    assert!(matches!(
        projected.refractions[0].attachment,
        QlAttachment::Unavailable {
            ref health,
            ..
        } if health.state == QlProviderState::Absent
    ));
}

#[test]
fn degraded_provider_can_enrich_and_exposes_provider_version_and_exact_identity_provenance() {
    let context = context();
    let provider = FixtureQl::new(QlProviderState::Degraded);
    let projected = project_context_with_ql(&context, Some(&provider), projection(QlMode::Optional))
        .expect("degraded provider remains usable");

    assert_eq!(projected.context, context);
    assert_eq!(provider.calls(), 1);
    let capabilities = projected.discovery.capabilities.as_ref().unwrap();
    assert_eq!(capabilities.provider.provider, "fixture-ql");
    assert_eq!(capabilities.provider.version, "0.1.0");
    assert_eq!(capabilities.health.state, QlProviderState::Degraded);
    assert!(capabilities.operations.contains(&QlOperation::Refract));
    assert!(capabilities
        .output_schema_versions
        .contains(&QL_OUTPUT_SCHEMA_VERSION.to_string()));

    match &projected.refractions[0].attachment {
        QlAttachment::Reading { health, reading } => {
            assert_eq!(health.state, QlProviderState::Degraded);
            assert_eq!(reading.target.subject.as_str(), "factory:claim:c-1");
            assert_eq!(reading.provenance.provider.provider, "fixture-ql");
            assert_eq!(reading.provenance.provider.version, "0.1.0");
            assert_eq!(
                reading.provenance.input_refs[0].reference.as_str(),
                "factory:claim:c-1"
            );
            assert_eq!(
                reading.provenance.input_refs[0].revision.as_deref(),
                Some("sha256:claim-c-1-r1")
            );
            assert_eq!(
                reading.provenance.result_class,
                QlResultClass::SemanticStochastic
            );
            assert_eq!(reading.provenance.model.as_deref(), Some("fixture-semantic-model"));
            assert_eq!(
                reading.provenance.config_ref.as_ref().map(|value| value.as_str()),
                Some("fixture:config:q4")
            );
        }
        other => panic!("expected QL reading, got {other:?}"),
    }
}

#[test]
fn incompatible_provider_is_non_fatal_in_optional_mode_and_is_not_called() {
    let context = context();
    let provider = FixtureQl::new(QlProviderState::Incompatible);
    let projected = project_context_with_ql(&context, Some(&provider), projection(QlMode::Optional))
        .expect("optional incompatible provider");

    assert_eq!(projected.context, context);
    assert_eq!(provider.calls(), 0);
    assert_eq!(projected.discovery.health.state, QlProviderState::Incompatible);
    assert!(matches!(
        projected.refractions[0].attachment,
        QlAttachment::Unavailable { .. }
    ));
}

#[test]
fn required_mode_fails_when_provider_is_absent_or_execution_fails() {
    let context = context();
    let absent = project_context_with_ql(&context, None, projection(QlMode::Required))
        .expect_err("required QL without provider must fail");
    assert_eq!(absent.code(), "ql.required_unavailable");

    let provider = FixtureQl::failing();
    let failure = project_context_with_ql(&context, Some(&provider), projection(QlMode::Required))
        .expect_err("required provider failure must be visible");
    assert_eq!(failure.code(), "ql.required_provider_failure");
    assert_eq!(provider.calls(), 1);
}

#[test]
fn provider_target_or_revision_drift_is_rejected_instead_of_translated() {
    let context = context();
    for provider in [FixtureQl::drift_target(), FixtureQl::drift_revision()] {
        let projected = project_context_with_ql(
            &context,
            Some(&provider),
            projection(QlMode::Optional),
        )
        .expect("optional provider contract failure is an attachment");
        assert_eq!(projected.context, context);
        assert!(matches!(
            projected.refractions[0].attachment,
            QlAttachment::Failed { .. }
        ));
    }
}

#[test]
fn legacy_ql_strings_are_rejected_before_provider_execution() {
    let context = context();
    let provider = FixtureQl::new(QlProviderState::Available);
    let legacy = QlProjectionRequest {
        mode: QlMode::Optional,
        refractions: vec![QlRefractionRequest::new(subject(), "lens:L3")],
    };
    let error = project_context_with_ql(&context, Some(&provider), legacy)
        .expect_err("legacy QL string must not be translated");

    assert_eq!(error.code(), "ql.invalid_lens_ref");
    assert_eq!(provider.calls(), 0);
}
