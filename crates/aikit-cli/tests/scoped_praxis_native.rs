#![cfg(unix)]

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use aikit_cli::app::Service;
use aikit_cli::recognised_praxis::{RecognisedPraxisApplication, RecognisedPraxisRequest};
use aikit_cli::scoped_invocation::{
    current_scoped_invocation_context, ScopedActionApplication, ScopedRunOutcome, ScopedRunRequest,
    NATIVE_CAPABILITY_RUN_ACTION,
};
use aikit_core::context_resolution::ContextResolution;
use aikit_core::resource::operative_scope::invocation::{
    ScopedActionAuthority, ScopedActionInput, ScopedInputDisposition,
};
use aikit_core::resource::operative_scope::{
    OperativeScope, ScopeAwareOperativeProvider, ScopeObservation, ScopeSource,
    ScopedResolveExpression,
};
use aikit_core::resource::{
    parse_resolve_expression, ActionRef, ActionSemanticProfile, AddressHorizon,
    OperativeSemanticProvider, OperativeSemanticProviderCapabilities,
    OperativeSemanticProviderDescriptor, OperativeSemanticProviderStatus, OwnerRef, ProviderRef,
    RelationOp, ResolveExpression, ResolvePath, ResourceRef, SourceRef, SourceRevision,
    OPERATIVE_SEMANTIC_PROVIDER_VERSION,
};
use aikit_store::AikitHome;
use tempfile::TempDir;

const CONTEXT_ID: &str = "ctx_01HZAW94NATIVE00000000000";
const SUBJECT: &str = "script/demo/scoped-praxis";

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn fixture() -> (TempDir, TempDir, Service) {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    let capsule = home
        .path()
        .join("registries/personal/capsules/script/demo/scoped-praxis");
    write(
        &capsule.join("manifest.toml"),
        r#"schema = 1
id = "script/demo/scoped-praxis"
kind = "script"
name = "scoped-praxis"
description = "A real subprocess used by AW94 native acceptance."

[script]
entry = "payload/run.sh"
interpreter = ["/bin/sh"]
mode = "capture"
cwd = "project"
"#,
    );
    let entry = capsule.join("payload/run.sh");
    write(&entry, "#!/bin/sh\nprintf 'aw94:%s\\n' \"$1\"\n");
    let mut permissions = fs::metadata(&entry).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&entry, permissions).unwrap();
    write(
        &project.path().join(".aikit/profile.toml"),
        &format!("schema = 1\nenable = [\"{SUBJECT}\"]\n"),
    );

    let mut env = BTreeMap::new();
    env.insert("AIKIT_CONTEXT_ID".to_string(), CONTEXT_ID.to_string());
    let service = Service::open(AikitHome::at(home.path()), project.path(), move |key| {
        env.get(key).cloned()
    })
    .unwrap();
    (home, project, service)
}

fn scope() -> OperativeScope {
    OperativeScope {
        provider: ProviderRef::parse("provider/ql-mef").unwrap(),
        binding: ResourceRef::parse("ql/binding/aw94").unwrap(),
        owner: OwnerRef::parse("QL-MEF").unwrap(),
        owner_revision: SourceRevision::parse("producer-aw94").unwrap(),
        interpretation: ResourceRef::parse("ql/interpretation/c-prime").unwrap(),
        interpretation_revision: SourceRevision::parse("c-prime-r1").unwrap(),
        world: ResourceRef::parse("world/aw94").unwrap(),
        generation: SourceRevision::parse("generation-1").unwrap(),
        whole: ResourceRef::parse("whole/aw94").unwrap(),
        subject: ResourceRef::parse(SUBJECT).unwrap(),
        sources: vec![ScopeSource {
            source: SourceRef::parse("source/ql-mef/aw94").unwrap(),
            revision: SourceRevision::parse("source-r1").unwrap(),
        }],
        method_skill: None,
    }
}

struct Provider;

impl OperativeSemanticProvider for Provider {
    type SemanticRef = String;
    type ResourceReading = String;
    type ActionProfile = String;
    type Path = String;

    fn descriptor(&self) -> OperativeSemanticProviderDescriptor {
        OperativeSemanticProviderDescriptor {
            version: OPERATIVE_SEMANTIC_PROVIDER_VERSION.into(),
            provider: ProviderRef::parse("provider/ql-mef").unwrap(),
            status: OperativeSemanticProviderStatus::Available,
            capabilities: OperativeSemanticProviderCapabilities::default(),
            provenance: vec![ResourceRef::parse("evidence/ql-provider/aw94").unwrap()],
        }
    }

    fn bind_horizon(&self, _: AddressHorizon) -> aikit_core::Result<Option<Self::SemanticRef>> {
        Ok(None)
    }

    fn bind_relation(&self, _: RelationOp) -> aikit_core::Result<Option<Self::SemanticRef>> {
        Ok(None)
    }

    fn resource_readings(
        &self,
        _: &ResourceRef,
        _: Option<&ResolveExpression>,
    ) -> aikit_core::Result<Vec<Self::ResourceReading>> {
        Ok(Vec::new())
    }

    fn enrich_action(
        &self,
        _: &ActionSemanticProfile,
    ) -> aikit_core::Result<Option<Self::ActionProfile>> {
        Ok(None)
    }

    fn enrich_path(&self, _: &ResolvePath) -> aikit_core::Result<Option<Self::Path>> {
        Ok(None)
    }
}

impl ScopeAwareOperativeProvider for Provider {
    fn observe_scope(
        &self,
        requested: &OperativeScope,
        _: &ContextResolution,
    ) -> aikit_core::Result<ScopeObservation> {
        Ok(ScopeObservation::Current {
            binding: requested.clone(),
            evidence: vec![ResourceRef::parse("evidence/ql-scope/aw94").unwrap()],
        })
    }
}

#[test]
fn qualified_resolve_runs_real_subprocess_then_names_proves_and_reresolves_method_skill() {
    let (_home, _project, mut service) = fixture();
    let provider = Provider;
    let (resources, _) = current_scoped_invocation_context(&service).unwrap();
    let action_resource = ResourceRef::parse(NATIVE_CAPABILITY_RUN_ACTION).unwrap();
    let action = ActionRef::parse(action_resource, &resources).unwrap();
    let subject = ResourceRef::parse(SUBJECT).unwrap();
    let expression = ScopedResolveExpression {
        expression: parse_resolve_expression(&format!(
            "( {SUBJECT} / + {NATIVE_CAPABILITY_RUN_ACTION} )"
        ))
        .unwrap(),
        scopes: vec![aikit_core::resource::operative_scope::ExpressionScope {
            node: Vec::new(),
            binding: scope(),
        }],
    };
    let authority = ScopedActionAuthority {
        authority_ref: ResourceRef::parse("authority/aw94/manual").unwrap(),
        authority_revision: Some(SourceRevision::parse("authority-r1").unwrap()),
        action,
        subject: subject.clone(),
        granted: true,
        unattended: false,
        evidence: vec![ResourceRef::parse("evidence/authority/aw94").unwrap()],
    };
    let outcome = service
        .invoke_scoped_action(
            ScopedRunRequest {
                expression,
                subject,
                authority,
                input: ScopedActionInput {
                    input_ref: ResourceRef::parse("input/aw94/one").unwrap(),
                    revision: Some(SourceRevision::parse("input-r1").unwrap()),
                    digest: "a".repeat(64),
                    disposition: ScopedInputDisposition::NewInput,
                },
                args: vec!["native".into()],
                confirmed: true,
                attempt_ordinal: 1,
            },
            &provider,
        )
        .unwrap();

    let (invocation, attempt, returned, run) = match outcome {
        ScopedRunOutcome::Completed {
            invocation,
            attempt,
            returned,
            run,
        } => (invocation, attempt, returned, run),
        ScopedRunOutcome::Failed { error, .. } => panic!("native invocation failed: {error}"),
    };
    assert_eq!(run.report.status, 0);
    assert!(run.report.output.join("\n").contains("aw94:native"));
    assert_eq!(returned.original_scopes, invocation.original_scopes);

    let receipt = service
        .recognise_praxis(RecognisedPraxisRequest {
            name: "AW94 native echo".into(),
            skill_body: "# AW94 native echo\n\nInvoke the proven scoped capability and verify its returned evidence.\n"
                .into(),
            skill_id: None,
            invocation,
            attempt,
            returned,
            recognition_refs: vec![ResourceRef::parse("recognition/aw94/one").unwrap()],
            verification_refs: vec![ResourceRef::parse("verification/aw94/one").unwrap()],
            verification_passed: true,
        })
        .unwrap();

    assert_eq!(receipt.method.id.as_str(), "skill/recognised/aw94-native-echo");
    assert_eq!(receipt.proof.method, receipt.method.id);
    assert!(matches!(
        receipt.naming_expression,
        ResolveExpression::Binary {
            op: RelationOp::Express,
            ..
        }
    ));
    assert!(receipt.promoted.manifest_path.is_file());
    assert!(receipt.promoted.payload_path.is_file());
}
