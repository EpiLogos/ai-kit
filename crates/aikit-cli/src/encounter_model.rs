//! Model selection for the existing resident owner. Catalogue identity, scoped
//! source policy, native Agency authority and credential delivery are separate
//! inputs. A configured body is not reported as an inference result.
use super::{error, native_admission, read_binding};
use crate::encounter_service::{
    EncounterContextAdmission, EncounterProtocol, EncounterProvider, EncounterRequiredSource,
    EncounterService,
};
use aikit_adapters::credential_provider::{EnvironmentImportProvider, NativeSecureStoreProvider};
use aikit_core::credential::{
    resolve_credential, CredentialRef, CredentialResolutionRequest, SecretMaterialisationClass,
    SecretProvider, SecretRequirement, SecretRequirementRef, SecretValue,
};
use aikit_core::resource::{canonical_model_ref, CredentialCondition, ProviderRef};
use aikit_core::{ResourceRef, Result};
use aikit_store::{AikitHome, CredentialBindingStore};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ModelCredential {
    pub requirement_ref: SecretRequirementRef,
    pub credential_ref: CredentialRef,
    pub target_env: String,
    /// Explicit import, never inferred from an ambient variable or a .env file.
    pub from_env: Option<String>,
}

/// An explicit AIKit-owned dispatch policy supplied through the existing native
/// provider configuration as a pinned SourceRef. It narrows an existing grant;
/// neither this source nor a profile creates the grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ModelPolicy {
    pub schema: String,
    pub agent_ref: ResourceRef,
    pub world_ref: ResourceRef,
    pub authority_ref: ResourceRef,
    pub bounds_refs: Vec<ResourceRef>,
    pub model_ref: ResourceRef,
    pub provider_ref: ProviderRef,
    pub native_provider: String,
    pub provider_native_id: String,
    pub expires_at_unix_ms: u64,
    pub credential: Option<ModelCredential>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PreparedModel {
    pub policy_source: EncounterRequiredSource,
    pub policy: ModelPolicy,
    pub catalogue_entry: Value,
    pub catalogue_digest: String,
    pub agency_source: Value,
    pub agency_ref: ResourceRef,
    pub world_binding_ref: ResourceRef,
    pub credential_reading: Option<Value>,
}

fn read_policy(source: &EncounterRequiredSource) -> Result<ModelPolicy> {
    for path in [
        source.path.clone(),
        source.path.canonicalize().map_err(error)?,
    ] {
        if path
            .ancestors()
            .any(|p| p.join(".no-agent-retrieval").exists())
        {
            return Err(error(
                "Selected model policy is withheld from Agent retrieval",
            ));
        }
    }
    EncounterContextAdmission {
        sources: vec![source.clone()],
        source_activations: vec![],
        projection: None,
        activation: None,
    }
    .verify()?;
    let bytes = fs::read(&source.path).map_err(error)?;
    if format!("blake3:{}", blake3::hash(&bytes).to_hex()) != source.content_digest {
        return Err(error("Model policy changed while it was being read"));
    }
    let policy: ModelPolicy = serde_json::from_slice(&bytes).map_err(error)?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(error)?
        .as_millis();
    if policy.schema != "aikit.model-dispatch-policy/v1"
        || u128::from(policy.expires_at_unix_ms) <= now
        || policy.bounds_refs.is_empty()
        || policy.bounds_refs.len() > 64
        || policy.native_provider.is_empty()
        || policy.native_provider.len() > 128
        || !policy
            .native_provider
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
        || policy.provider_native_id.is_empty()
        || policy.provider_native_id.len() > 1024
        || policy.provider_native_id.starts_with('-')
        || policy.provider_native_id.chars().any(char::is_control)
    {
        return Err(error(
            "Model dispatch needs a current, bounded, explicit native policy",
        ));
    }
    canonical_model_ref(policy.model_ref.as_str())?;
    Ok(policy)
}

fn valid_credential_variable(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        && (name.ends_with("_API_KEY") || name.ends_with("_TOKEN") || name.ends_with("_KEY"))
        && !["CENTRAL_", "WORKCELL_", "AIKIT_", "LD_", "DYLD_"]
            .iter()
            .any(|p| name.starts_with(p))
}

fn credential(
    home: &AikitHome,
    session: &ResourceRef,
    use_: &ModelCredential,
    materialise: bool,
) -> Result<(Value, Option<SecretValue>)> {
    if !valid_credential_variable(&use_.target_env)
        || use_
            .from_env
            .as_ref()
            .is_some_and(|v| !valid_credential_variable(v))
    {
        return Err(error("Model credentials need explicit non-control credential variable names; environment control injection is refused"));
    }
    let stored = CredentialBindingStore::new(home).load(&use_.credential_ref)?;
    if let Some(binding) = &stored {
        if binding.revoked
            || binding
                .expires_at
                .as_deref()
                .map(|s| s.parse::<jiff::Timestamp>().map_err(error))
                .transpose()?
                .is_some_and(|t| t <= jiff::Timestamp::now())
        {
            return Err(error(
                "Selected credential binding is revoked or expired; no environment bypass",
            ));
        }
    }
    let native = NativeSecureStoreProvider::new();
    let environment = use_
        .from_env
        .as_ref()
        .map(|name| {
            EnvironmentImportProvider::from_process(use_.credential_ref.clone(), name, None)
        })
        .transpose()?;
    let mut descriptors = vec![native.descriptor(&use_.credential_ref)];
    if let Some(env) = &environment {
        descriptors.push(env.descriptor(&use_.credential_ref));
    }
    let resolution = resolve_credential(CredentialResolutionRequest {
        requirement: SecretRequirement {
            requirement_ref: use_.requirement_ref.clone(),
            credential_ref: use_.credential_ref.clone(),
            consumer_ref: session.to_string(),
            purpose: "Scoped model dispatch into the selected native resident".into(),
            permitted_materialisation: [SecretMaterialisationClass::ProcessEnv].into(),
        },
        providers: descriptors,
        headless: true,
        allow_from_env: use_.from_env.is_some(),
    })?;
    let provider = resolution.selected_provider_ref.as_ref().ok_or_else(|| {
        error("No eligible current credential provider; an inventory reference is not key material")
    })?;
    let secret = if materialise {
        let source: &dyn SecretProvider = if native.descriptor(&use_.credential_ref).provider_ref
            == *provider
        {
            &native
        } else {
            environment
                .as_ref()
                .filter(|e| e.descriptor(&use_.credential_ref).provider_ref == *provider)
                .ok_or_else(|| {
                    error("Selected credential provider is not materialisable by this native path")
                })?
        };
        Some(source.materialise(&use_.credential_ref, SecretMaterialisationClass::ProcessEnv)?
            .ok_or_else(|| error("Selected credential provider did not return material; refusing provider execution"))?)
    } else {
        None
    };
    Ok((
        json!({"resolution":resolution,"binding":stored,"delivery":"process-env", "secret_persisted":false}),
        secret,
    ))
}

pub(crate) fn prepare(
    home: &AikitHome,
    session: &ResourceRef,
    provider: &EncounterProvider,
) -> Result<Option<PreparedModel>> {
    let Some(source) = &provider.model_policy else {
        return Ok(None);
    };
    if provider.protocol != EncounterProtocol::PiRpc {
        return Err(error("This model-dispatch adapter supports Pi RPC only; ACP configuration is not assumed to have Pi selection/readback semantics"));
    }
    let binding = read_binding(home, session)?
        .ok_or_else(|| error("Selected model needs a real Agency/WorldBinding, not a profile"))?;
    let admitted = native_admission(&binding)?;
    let policy = read_policy(source)?;
    let determination = &admitted.receipt["determination"];
    if policy.agent_ref != admitted.agent_ref
        || policy.world_ref != admitted.world_ref
        || !admitted.authorises(&ResourceRef::parse("action/aikit/model-realise")?)
        || !determination["authority_refs"]
            .as_array()
            .is_some_and(|a| a.contains(&json!(policy.authority_ref)))
        || !policy.bounds_refs.iter().all(|r| {
            determination["bounds_refs"]
                .as_array()
                .is_some_and(|a| a.contains(&json!(r)))
        })
    {
        return Err(error("Current Agency does not authorise this selected model policy, World, authority or bounds"));
    }
    let (catalogue, _) = aikit_store::model_catalogue::resolved_catalogue(home);
    let entry = catalogue.get(&policy.model_ref).ok_or_else(|| {
        error("Selected Model is absent from the canonical catalogue; detection does not mint it")
    })?;
    let routes: Vec<_> = entry
        .routes
        .iter()
        .filter(|r| r.provider == policy.provider_ref && r.claims(&policy.provider_native_id))
        .collect();
    if routes.is_empty() {
        return Err(error(
            "Native model/provider does not name a declared route for the canonical Model",
        ));
    }
    if routes
        .iter()
        .any(|r| r.credential != CredentialCondition::NotRequired)
        && policy.credential.is_none()
    {
        return Err(error(
            "The declared model route requires an explicitly resolved credential",
        ));
    }
    let credential_reading = policy
        .credential
        .as_ref()
        .map(|c| credential(home, session, c, false).map(|r| r.0))
        .transpose()?;
    let catalogue_entry = serde_json::to_value(entry).map_err(error)?;
    let catalogue_digest = format!(
        "blake3:{}",
        blake3::hash(catalogue_entry.to_string().as_bytes()).to_hex()
    );
    Ok(Some(PreparedModel {
        policy_source: source.clone(),
        policy,
        catalogue_entry,
        catalogue_digest,
        agency_source: serde_json::to_value(&binding.agency_source).map_err(error)?,
        agency_ref: admitted.agency_ref,
        world_binding_ref: admitted.world_binding_ref,
        credential_reading,
    }))
}

impl PreparedModel {
    pub fn fingerprint(&self) -> Result<String> {
        Ok(format!(
            "blake3:{}",
            blake3::hash(&serde_json::to_vec(self).map_err(error)?).to_hex()
        ))
    }
    pub fn require_same(&self, current: &Option<Self>) -> Result<()> {
        if current.as_ref() != Some(self) {
            return Err(error("Selected model source/catalogue/Agency/credential basis changed; explicitly re-resolve the resident"));
        }
        Ok(())
    }
}

/// Pi's native flags select its model. Its real get_state and assistant result
/// must also confirm the same provider/id; these arguments alone are not proof.
fn selected_argv(provider: &EncounterProvider, model: &PreparedModel) -> Result<Vec<String>> {
    if provider.argv.is_empty()
        || provider.argv.iter().any(|a| {
            a == "--"
                || a == "--model"
                || a == "--provider"
                || a.starts_with("--model=")
                || a.starts_with("--provider=")
        })
    {
        return Err(error("Model-selected provider needs one unambiguous native provider/model binding; conflicting flags are not rewritten"));
    }
    let mut argv = provider.argv.clone();
    argv.extend([
        "--provider".into(),
        model.policy.native_provider.clone(),
        "--model".into(),
        model.policy.provider_native_id.clone(),
    ]);
    Ok(argv)
}

/// Scoped final-child environment. Raw material is not serializable and is
/// never stored in model, provider, task, delivery or material receipts.
pub(crate) struct ModelEnvironment {
    credential: Option<(String, SecretValue)>,
}
impl ModelEnvironment {
    pub fn apply(self, command: &mut Command) {
        command.env_clear();
        for name in [
            "HOME",
            "PATH",
            "TERM",
            "LANG",
            "LC_ALL",
            "TZ",
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_STATE_HOME",
            "AIKIT_HOME",
            "AIKIT_CONTEXT_ID",
            "AIKIT_ISOLATION",
        ] {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        if let Some((name, value)) = self.credential {
            command.env(name, value.expose());
        }
    }
}

pub(crate) fn execution(
    home: &AikitHome,
    session: &ResourceRef,
    provider: &EncounterProvider,
) -> Result<(Vec<String>, Option<ModelEnvironment>)> {
    let Some(model) = prepare(home, session, provider)? else {
        return Ok((provider.argv.clone(), None));
    };
    let delivery = model
        .policy
        .credential
        .as_ref()
        .map(|c| {
            let (reading, secret) = credential(home, session, c, true)?;
            if model.credential_reading.as_ref() != Some(&reading) {
                return Err(error(
                    "Credential provider changed between resolution and launch",
                ));
            }
            Ok((
                c.target_env.clone(),
                secret.ok_or_else(|| error("Missing model secret material"))?,
            ))
        })
        .transpose()?;
    model.require_same(&prepare(home, session, provider)?)?;
    Ok((
        selected_argv(provider, &model)?,
        Some(ModelEnvironment {
            credential: delivery,
        }),
    ))
}

pub(crate) fn direct_launcher(
    session: &ResourceRef,
    provider: &EncounterProvider,
    model: &PreparedModel,
) -> Result<Vec<String>> {
    Ok(vec![
        std::env::current_exe()
            .map_err(error)?
            .display()
            .to_string(),
        "encounter-model-exec".into(),
        "--agent-session".into(),
        session.to_string(),
        "--provider".into(),
        provider.id.clone(),
        "--expected-model-basis".into(),
        model.fingerprint()?,
    ])
}

impl EncounterService {
    /// Internal native exec, not an IPC operation and not a second runtime.
    pub fn exec_model(
        home: &AikitHome,
        session: &ResourceRef,
        provider_id: &str,
        expected: &str,
    ) -> Result<()> {
        let service = Self::new(home.clone())?;
        service.require_attached(session)?;
        let provider = service
            .providers()?
            .into_iter()
            .find(|p| p.id == provider_id)
            .ok_or_else(|| error("Selected native model provider configuration was removed"))?;
        let model = prepare(home, session, &provider)?
            .ok_or_else(|| error("Model binding was removed; no unselected fallback"))?;
        if model.fingerprint()? != expected {
            return Err(error("Model launch basis changed since native admission"));
        }
        let (argv, environment) = execution(home, session, &provider)?;
        let (program, args) = argv
            .split_first()
            .ok_or_else(|| error("Missing native model executable"))?;
        let mut command = Command::new(program);
        command.args(args);
        environment
            .ok_or_else(|| error("Missing scoped model environment"))?
            .apply(&mut command);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            Err(error(command.exec()))
        }
        #[cfg(not(unix))]
        {
            Err(error(
                "Scoped native model exec is unsupported on this platform",
            ))
        }
    }
}

/// Exact target of the existing native open-model operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EncounterModelOpen {
    pub space: aikit_core::session_space::SessionSpaceRef,
    pub agent_session: ResourceRef,
    pub cwd: std::path::PathBuf,
    pub model_ref: ResourceRef,
    pub provider_ref: Option<ProviderRef>,
    pub body: Option<String>,
    pub expected_agency: aikit_adapters::agency_admission::AdmittedAgency,
}
pub(crate) fn validate_target(
    home: &AikitHome,
    session: &ResourceRef,
    configured: &EncounterProvider,
    request: &EncounterModelOpen,
) -> Result<()> {
    let binding = read_binding(home, session)?
        .ok_or_else(|| error("Selected model target lacks native Agency"))?;
    let admitted = native_admission(&binding)?;
    if admitted != request.expected_agency {
        return Err(error("Selected Agency/source/WorldBinding changed between composition and resident admission"));
    }
    let model = prepare(home, session, configured)?
        .ok_or_else(|| error("The configured body has no explicit model policy"))?;
    if model.policy.model_ref != request.model_ref
        || request
            .provider_ref
            .as_ref()
            .is_some_and(|p| p != &model.policy.provider_ref)
        || request.body.as_ref().is_some_and(|b| b != &configured.id)
    {
        return Err(error(
            "Resolved body/model does not match the explicit catalogue target",
        ));
    }
    Ok(())
}
impl EncounterService {
    pub(crate) fn open_model(&self, request: EncounterModelOpen) -> Result<Value> {
        self.require_attached(&request.agent_session)?;
        let mut candidates = Vec::new();
        for configured in self.providers()? {
            if configured.model_policy.is_none()
                || request.body.as_ref().is_some_and(|b| b != &configured.id)
            {
                continue;
            }
            if validate_target(&self.home, &request.agent_session, &configured, &request).is_ok()
                && self
                    .check_task_launch(&request.agent_session, &configured, &request.cwd)
                    .is_ok()
            {
                candidates.push(configured);
            }
        }
        if candidates.len() != 1 {
            return Err(error(if candidates.is_empty() {
                "No configured body has current model/source/credential/authority/protocol eligibility; no fallback selected"
            } else {
                "Several native bodies are eligible; explicitly select a body"
            }));
        }
        let provider = candidates.remove(0);
        let mut result = self.open_native(
            request.space.clone(),
            request.agent_session.clone(),
            provider.id,
            request.cwd.clone(),
            false,
            Some(&request),
        )?;
        result["selected"] = json!(true);
        result["executed"] = json!(false);
        result["standing"] = json!("native selected-model resident, not an inference result");
        Ok(result)
    }
}
