"""One-use source publication for this scoped branch; removed in its source commit.
No runtime/test patches: publish actual production files before normal CI tests.
"""
from pathlib import Path
import subprocess

BASE = 'bf4b8cea15ad03e69b5dc934703d6fa5e775c6e4'
CHANGED = []
def edit(path, replacements):
    p = Path(path)
    old = subprocess.check_output(['git', 'show', f'{BASE}:{path}']).decode()
    if p.read_text() != old:
        raise RuntimeError(f'Concurrent source change: {path}')
    text = old
    for before, after in replacements:
        if before not in text:
            raise RuntimeError(f'Missing exact edit anchor: {path}: {before[:70]}')
        text = text.replace(before, after)
    p.write_text(text)
    CHANGED.append(path)

edit('crates/aikit-adapters/src/credential_provider.rs', [
 ('supported_materialisation: [SecretMaterialisationClass::ProviderNativeLease]', 'supported_materialisation: [SecretMaterialisationClass::ProviderNativeLease, SecretMaterialisationClass::ProcessEnv]'),
 ('if class != SecretMaterialisationClass::ProviderNativeLease {', 'if !matches!(class, SecretMaterialisationClass::ProviderNativeLease | SecretMaterialisationClass::ProcessEnv) {'),
])
edit('crates/aikit-adapters/src/pi_rpc_connection.rs', [
 ('    abort_acknowledged: bool,', '    expected_model: Option<(String, String)>,\n    model_observation: Option<crate::agent_connection::NativeModelObservation>,\n    abort_acknowledged: bool,'),
 ('            abort_acknowledged: false,', '            expected_model: None,\n            model_observation: None,\n            abort_acknowledged: false,'),
 ('    fn request(\n', '''    pub fn with_selected_model(mut self, provider: &str, model_id: &str) -> Result<Self> {
        if provider.trim().is_empty() || model_id.trim().is_empty() {
            return Err(error("connection.pi_rpc.model_selection", "Native provider and model id are required"));
        }
        self.expected_model = Some((provider.into(), model_id.into()));
        Ok(self)
    }

    fn request(
'''),
 ('        self.observed_session = Some(id.into());', '''        if let Some((provider, model)) = &self.expected_model {
            if data["model"]["provider"].as_str() != Some(provider.as_str())
                || data["model"]["id"].as_str() != Some(model.as_str()) {
                return Err(error("connection.pi_rpc.model_mismatch", "Pi native state does not confirm the selected provider/model; no default or fallback is admitted"));
            }
            self.model_observation = Some(crate::agent_connection::NativeModelObservation {
                current_model_id:Some(model.clone()),
                available_models:vec![crate::agent_connection::NativeModelEntry {
                    id:model.clone(), name:data["model"]["name"].as_str().unwrap_or(model).into(), description:None,
                }], standing:Some(format!("Pi native get_state; provider={provider}; configuration, not an inference receipt")),
            });
        }
        self.observed_session = Some(id.into());'''),
 ('                    binding.provenance = self.provenance.clone();', '                    binding.provenance = self.provenance.clone();\n                    binding.model_observation = self.model_observation.clone();'),
 ('                if result["role"] == "assistant" {', '''                if result["role"] == "assistant" {
                    if let Some((provider, model)) = &self.expected_model {
                        if result["provider"].as_str() != Some(provider.as_str())
                            || result["model"].as_str() != Some(model.as_str()) {
                            return Err(error("connection.pi_rpc.response_model_mismatch", "Assistant result does not name the selected native provider/model; response remains failed, not attributed to the requested Model"));
                        }
                    }'''),
])
edit('crates/aikit-cli/src/encounter_agency.rs', [
 ('#[path = "encounter_task.rs"]', '#[path = "encounter_model.rs"]\npub(crate) mod model;\n\n#[path = "encounter_task.rs"]'),
])
edit('crates/aikit-cli/src/bin/aikit-session-space.rs', [
 ('enum Command {', '''enum Command {
    /// Internal scoped Model launch; raw credential material never enters JSON.
    EncounterModelExec {
        #[arg(long)] agent_session: String,
        #[arg(long)] provider: String,
        #[arg(long)] expected_model_basis: String,
    },'''),
 ('    match cli.command {', '''    match cli.command {
        Command::EncounterModelExec { agent_session, provider, expected_model_basis } =>
            aikit_cli::encounter_service::EncounterService::exec_model(service.home(),
                &aikit_core::ResourceRef::parse(agent_session)?, &provider, &expected_model_basis),'''),
])
edit('crates/aikit-cli/src/encounter_task.rs', [
 ('            || cwd != record.request.cwd {', '            || provider.model_policy != record.launcher.model_policy\n            || cwd != record.request.cwd {'),
 ('    pub fn read_task(', '''    pub(crate) fn is_task_bound(&self, session: &ResourceRef) -> Result<bool> {
        Ok(read(&self.home, session)?.is_some())
    }
    pub fn read_task('''),
 ('        let mut command = Command::new(&record.request.workcell_boundary_bin);', '        let (model_argv, model_environment) = super::model::execution(home, session, &record.request.provider)?;\n        let mut command = Command::new(&record.request.workcell_boundary_bin);'),
 ('            .args(&record.request.provider.argv)', '            .args(&model_argv)'),
 ('            .env_remove("WORKCELL_CONTROL_TOKEN");', '            .env_remove("WORKCELL_CONTROL_TOKEN");\n        if let Some(environment) = model_environment { environment.apply(&mut command); }'),
])
# The large resident owner keeps its existing runtime, source/permission,
# continuation and delivery operations; only native model admission is added.
edit('crates/aikit-cli/src/encounter_service.rs', [
 ('#[path = "encounter_agency.rs"]', 'pub use agency::model::EncounterModelOpen;\n\n#[path = "encounter_agency.rs"]'),
 ('pub struct EncounterProvider {', '''pub struct EncounterProvider {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_policy: Option<EncounterRequiredSource>,'''),
 ('pub enum EncounterRequest {', 'pub enum EncounterRequest {\n    OpenModel { request: Box<EncounterModelOpen> },'),
 ('struct Resident {', 'struct Resident {\n    model: Option<agency::model::PreparedModel>,'),
 ('        resident.context.verify()?;\n        Ok(())', '''        resident.context.verify()?;
        let current_model = agency::model::prepare(&self.home, session, configured)?;
        match &resident.model {
            Some(model) => model.require_same(&current_model)?,
            None if current_model.is_some() => return Err(error("Existing resident did not execute this model selection; start an explicitly selected body")),
            None => {}
        }
        if resident.model.is_some() {
            if resident.host.identity(session)?.state != aikit_adapters::SessionLaneState::Resident {
                return Err(error("Model readmission requires an idle native resident; current work is not interrupted by another send"));
            }
            resident.host.initialize()?;
            self.encounters.append_event(session, "model-admission-checked", &json!({"selection":resident.model,"standing":"current native model/source/credential basis checked; no inference implied"}))?;
        }
        Ok(())'''),
 ('        reconnect: bool,\n    ) -> Result<Value> {', '        reconnect: bool,\n        model_target: Option<&EncounterModelOpen>,\n    ) -> Result<Value> {'),
 ('        self.check_task_launch(&agent_session, &configured, &cwd)?;', '        if let Some(target) = model_target { agency::model::validate_target(&self.home, &agent_session, &configured, target)?; }\n        self.check_task_launch(&agent_session, &configured, &cwd)?;'),
 ('        let context = self.open_context(&agent_session, &configured)?;', '''        let context = self.open_context(&agent_session, &configured)?;
        let model = agency::model::prepare(&self.home, &agent_session, &configured)?;
        let launch_argv = if self.is_task_bound(&agent_session)? { configured.argv.clone() }
            else if let Some(model) = &model { agency::model::direct_launcher(&agent_session, &configured, model)? }
            else { configured.argv.clone() };'''),
 ('&configured.argv,\n                    Some(&cwd),', '&launch_argv,\n                    Some(&cwd),'),
 ('                let adapter = aikit_adapters::pi_rpc_connection::PiRpcConnectionAdapter::new(\n                    provider.clone(),\n                    agent_session.clone(),\n                    BTreeMap::new(),\n                );', '''                let mut adapter = aikit_adapters::pi_rpc_connection::PiRpcConnectionAdapter::new(
                    provider.clone(),
                    agent_session.clone(),
                    BTreeMap::new(),
                );
                if let Some(model) = &model { adapter = adapter.with_selected_model(&model.policy.native_provider, &model.policy.provider_native_id)?; }'''),
 ('"context_admission":context}', '"context_admission":context,"model_selection":model,"effective_launch_argv":launch_argv}'),
 ('        let next_cursor = self.read(&agent_session, 0)?.next_cursor;', '        let next_cursor = self.read(&agent_session, 0)?.next_cursor;\n        let model_reading = serde_json::to_value(&model).map_err(error)?;'),
 ('                context,\n                observation_path,', '                context,\n                model,\n                observation_path,'),
 ('"model_observation":model_observation,"resident":true', '"model_observation":model_observation,"model_selection":model_reading,"resident":true,"inference_observed":false'),
 ('"model_observation":held.lane.binding().model_observation,"resident":true', '"model_observation":held.lane.binding().model_observation,"model_selection":held.model,"resident":true'),
 ('        match request {', '        match request {\n            EncounterRequest::OpenModel { request } => self.open_model(*request),'),
 ('self.open_native(space, agent_session, provider, cwd, false)', 'self.open_native(space, agent_session, provider, cwd, false, None)'),
 ('self.open_native(space, agent_session, provider, cwd, true)', 'self.open_native(space, agent_session, provider, cwd, true, None)'),
])
for path in ['crates/aikit-cli/tests/encounter_context_admission.rs', 'crates/aikit-cli/tests/encounter_shutdown.rs']:
    edit(path, [('EncounterProvider {', 'EncounterProvider {\n            model_policy: None,')])
edit('crates/aikit-cli/tests/caw_native_delivery.rs', [
 ('//! Source-built native-owner acceptance.', '//! Source-built native-owner acceptance.'),
]) if False else None
p = Path('crates/aikit-cli/tests/caw_native_delivery.rs')
if p.read_text() != subprocess.check_output(['git','show',f'{BASE}:{p}']).decode():
    raise RuntimeError('Concurrent native delivery test change')
p.write_text(p.read_text() + '\n#[path = "support/caw_model_resident.rs"]\nmod model_proof;\n')
CHANGED.append(str(p))
edit('.github/workflows/caw-native-delivery.yml', [
 ('      - name: Native Central allocation to Workcell protection preparation', '          grep -q MODEL_SELECTED_NATIVE_RESPONSE_EXECUTED "$RUNNER_TEMP/caw-evidence/native.log"\n      - name: Native Central allocation to Workcell protection preparation'),
])
subprocess.run(['git','add','--',*CHANGED],check=True)
print('PUBLISHED_SOURCE_PATHS=' + ','.join(CHANGED))
