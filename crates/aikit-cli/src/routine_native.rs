//! Native Method bodies: Routine runs that execute owner Actions, not a model.
//!
//! Most Methods are procedures an agent performs, so a dispatched Routine run
//! opens a resident encounter (a model run). Some Methods are environmental
//! rhythm that must never involve a model — the DAY rollover opens the civil
//! Day and carries each Project's NOW field, and nothing about it needs or
//! permits judgement. Such a Method declares, in its capsule metadata, that its
//! body is native and exactly which owner Actions that body runs:
//!
//! ```toml
//! [metadata.native-method]
//! schema = "aikit.native-method/v1"
//! body = "central-day-rollover"
//! actions = ["central:action/central.time.policy", "central:action/central.day.ensure", …]
//!
//! [[metadata.native-method.credentials]]
//! env = "CENTRAL_NATIVE_TOKEN"
//! actions = ["central:action/central.day.ensure"]
//! ```
//!
//! The declared Actions become the Method's Actions, so the Routine's
//! authority can only grant a subset of them (`routine.action_not_in_method`),
//! and a binary that predates native bodies resolves the same capsule with the
//! generic capability-run Action and refuses the Routine rather than hand it
//! to a model. The runner is selected by the Method, never by a flag.
//!
//! Law of the body: it calls only Actions the Routine's admitted authority
//! names, passes an owner credential only into the one child that needs it,
//! and writes a receipt that names what ran — never the credential. The DAY
//! body completes no task, recognises no Return and reflects nothing.

use std::path::PathBuf;

use aikit_adapters::runner::{CommandRunner, SystemRunner};
use aikit_core::resource::ResourceRef;
use aikit_core::{AikitError, Result};
use serde::Serialize;
use serde_json::{json, Value};

use crate::routine_dispatch::{RoutineRunOutcome, RoutineRunRequest, RoutineRunner, RunStatus};
use crate::secret_location::SecretLocation;

pub const NATIVE_METHOD_SCHEMA: &str = "aikit.native-method/v1";
pub const NATIVE_METHOD_METADATA_KEY: &str = "native-method";
pub const NATIVE_RUN_RECEIPT_SCHEMA: &str = "aikit.native-routine-run/v1";

const TIME_POLICY: &str = "central.time.policy";
const DAY_ENSURE: &str = "central.day.ensure";
const WORLD: &str = "central.world";
const NOW_ROLLOVER: &str = "projectcentral.now.rollover";
const FACTORY_COLLECT: &str = "factory:action/telemetry.collect";
const FACTORY_FIELD: &str = "factory:action/telemetry.field";

/// A native body AIKit knows how to execute. Adding one is a code change and
/// a review, never a capsule's self-declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum NativeBody {
    CentralDayRollover,
    FactoryCollect,
    FactoryFieldRefresh,
}

impl NativeBody {
    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "central-day-rollover" => Some(Self::CentralDayRollover),
            "factory-collect" => Some(Self::FactoryCollect),
            "factory-field-refresh" => Some(Self::FactoryFieldRefresh),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::CentralDayRollover => "central-day-rollover",
            Self::FactoryCollect => "factory-collect",
            Self::FactoryFieldRefresh => "factory-field-refresh",
        }
    }

    /// The owner Actions this body calls; the Method must declare all of them.
    fn required_actions(self) -> &'static [&'static str] {
        match self {
            Self::CentralDayRollover => &[TIME_POLICY, DAY_ENSURE, WORLD, NOW_ROLLOVER],
            Self::FactoryCollect | Self::FactoryFieldRefresh => &[],
        }
    }
    fn required_factory_actions(self) -> &'static [&'static str] {
        match self {
            Self::CentralDayRollover => &[],
            Self::FactoryCollect => &[FACTORY_COLLECT, FACTORY_FIELD],
            Self::FactoryFieldRefresh => &[FACTORY_FIELD],
        }
    }
}

/// An exact, revision-bound project binding carried by a native Method's
/// capsule. A Routine cannot substitute another state/policy at dispatch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FactoryMethodBinding {
    pub state: PathBuf,
    pub policy: PathBuf,
    pub project_world_ref: String,
}

/// One owner credential a native body needs, for the named Actions only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CredentialNeed {
    pub env: String,
    pub actions: Vec<ResourceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NativeMethod {
    pub body: NativeBody,
    pub actions: Vec<ResourceRef>,
    pub credentials: Vec<CredentialNeed>,
    pub factory: Option<FactoryMethodBinding>,
}

/// `central:action/<name>` — the ref form a native Method declares.
pub fn central_action_ref(name: &str) -> String {
    format!("central:action/{name}")
}

fn metadata_error(message: impl Into<String>) -> AikitError {
    AikitError::new("routine.native_method_invalid", message.into())
}

impl NativeMethod {
    /// Read the declaration from a capsule, if it makes one.
    pub fn from_capsule(capsule: &aikit_core::Capsule) -> Result<Option<Self>> {
        let Some(table) = capsule.metadata.get(NATIVE_METHOD_METADATA_KEY) else {
            return Ok(None);
        };
        let table = table.as_table().ok_or_else(|| {
            metadata_error(format!(
                "{}: [metadata.native-method] must be a table",
                capsule.id
            ))
        })?;
        if table.get("schema").and_then(|v| v.as_str()) != Some(NATIVE_METHOD_SCHEMA) {
            return Err(metadata_error(format!(
                "{}: [metadata.native-method] must declare schema = \"{NATIVE_METHOD_SCHEMA}\"",
                capsule.id
            )));
        }
        let body = table
            .get("body")
            .and_then(|v| v.as_str())
            .and_then(NativeBody::parse)
            .ok_or_else(|| {
                metadata_error(format!(
                    "{}: native body is missing or not one AIKit executes (central-day-rollover, factory-collect, factory-field-refresh)",
                    capsule.id
                ))
            })?;
        let refs = |value: Option<&toml::Value>, label: &str| -> Result<Vec<ResourceRef>> {
            let items = value.and_then(|v| v.as_array()).ok_or_else(|| {
                metadata_error(format!(
                    "{}: {label} must be an array of Action refs",
                    capsule.id
                ))
            })?;
            items
                .iter()
                .map(|item| {
                    item.as_str()
                        .filter(|raw| raw.starts_with("central:action/") || raw.starts_with("factory:action/"))
                        .ok_or_else(|| {
                            metadata_error(format!(
                                "{}: {label} entries are central:action/<name> or factory:action/<name> refs",
                                capsule.id
                            ))
                        })
                        .and_then(ResourceRef::parse)
                })
                .collect()
        };
        let actions = refs(table.get("actions"), "actions")?;
        for required in body.required_actions() {
            let reference = central_action_ref(required);
            if !actions.iter().any(|action| action.as_str() == reference) {
                return Err(metadata_error(format!(
                    "{}: the {} body calls {reference}, which the Method does not declare",
                    capsule.id,
                    body.as_str()
                )));
            }
        }
        for required in body.required_factory_actions() {
            if !actions.iter().any(|action| action.as_str() == *required) {
                return Err(metadata_error(format!(
                    "{}: the {} body calls {required}, which the Method does not declare",
                    capsule.id,
                    body.as_str()
                )));
            }
        }
        let factory = if matches!(
            body,
            NativeBody::FactoryCollect | NativeBody::FactoryFieldRefresh
        ) {
            let binding = table
                .get("factory")
                .and_then(toml::Value::as_table)
                .ok_or_else(|| {
                    metadata_error(format!(
                        "{}: Factory native body requires [metadata.native-method.factory]",
                        capsule.id
                    ))
                })?;
            let path = |name: &str| -> Result<PathBuf> {
                let raw = binding
                    .get(name)
                    .and_then(toml::Value::as_str)
                    .ok_or_else(|| {
                        metadata_error(format!("{}: Factory binding requires {name}", capsule.id))
                    })?;
                let path = PathBuf::from(raw);
                if !path.is_absolute() || raw.len() > 4096 || raw.contains('\0') {
                    return Err(metadata_error(format!(
                        "{}: Factory binding {name} must be a bounded absolute path",
                        capsule.id
                    )));
                }
                Ok(path)
            };
            let project_world_ref = binding
                .get("project_world_ref")
                .and_then(toml::Value::as_str)
                .filter(|v| (v.starts_with("project:") || *v == "control:root") && v.len() <= 1024)
                .ok_or_else(|| {
                    metadata_error(format!(
                        "{}: Factory binding needs a project_world_ref",
                        capsule.id
                    ))
                })?
                .to_owned();
            Some(FactoryMethodBinding {
                state: path("state")?,
                policy: path("policy")?,
                project_world_ref,
            })
        } else {
            if table.get("factory").is_some() {
                return Err(metadata_error(format!(
                    "{}: DAY body cannot carry a Factory binding",
                    capsule.id
                )));
            }
            None
        };
        let mut credentials = Vec::new();
        for need in table
            .get("credentials")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
        {
            let env = need
                .get("env")
                .and_then(|v| v.as_str())
                .filter(|env| !env.is_empty())
                .ok_or_else(|| {
                    metadata_error(format!("{}: a credential need names its env", capsule.id))
                })?
                .to_owned();
            let for_actions = refs(need.get("actions"), "credential actions")?;
            if let Some(stray) = for_actions.iter().find(|a| !actions.contains(a)) {
                return Err(metadata_error(format!(
                    "{}: credential {env} is declared for {stray}, which the Method does not declare",
                    capsule.id
                )));
            }
            credentials.push(CredentialNeed {
                env,
                actions: for_actions,
            });
        }
        Ok(Some(Self {
            body,
            actions,
            credentials,
            factory,
        }))
    }
}

/// Picks the runner the Method selects: a native body runs natively; every
/// other Method opens a resident encounter. There is no fallback between them.
pub struct MethodSelectedRunner<E: RoutineRunner, N: RoutineRunner> {
    pub encounter: E,
    pub native: N,
}

impl<E: RoutineRunner, N: RoutineRunner> RoutineRunner for MethodSelectedRunner<E, N> {
    fn run(&self, request: RoutineRunRequest) -> RoutineRunOutcome {
        if request.native.is_some() {
            self.native.run(request)
        } else {
            self.encounter.run(request)
        }
    }
}

/// Executes native bodies against Central through the real `ctrl` binary.
pub struct NativeActionRunner {
    pub home: aikit_store::AikitHome,
    pub central_root: PathBuf,
    pub ctrl: String,
    pub factory: String,
}

impl NativeActionRunner {
    pub fn from_env(home: aikit_store::AikitHome, central_root: PathBuf) -> Self {
        let ctrl = std::env::var("CENTRAL_CTRL_BIN")
            .or_else(|_| std::env::var("OI_CENTRAL_CTRL_BIN"))
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "ctrl".into());
        Self {
            home,
            central_root,
            ctrl,
            factory: std::env::var("FACTORY_BIN")
                .or_else(|_| std::env::var("OI_FACTORY_BIN"))
                .ok()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "factory".into()),
        }
    }

    pub fn receipts_dir(home: &aikit_store::AikitHome) -> PathBuf {
        home.state().join("routine-native-runs")
    }
}

/// One owner Action call inside a native run, as the receipt records it.
#[derive(Debug, Clone, Serialize)]
struct ActionCall {
    action: String,
    input: Value,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    credential_env: Option<String>,
}

struct Run<'a> {
    runner: &'a NativeActionRunner,
    request: &'a RoutineRunRequest,
    method: &'a NativeMethod,
    calls: Vec<ActionCall>,
}

enum CallError {
    /// Refused before any process ran (authority or credential).
    Refused(String),
    /// The owner answered with a failure envelope.
    Owner { code: String, message: String },
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(message) => f.write_str(message),
            Self::Owner { code, message } => write!(f, "{code}: {message}"),
        }
    }
}

impl Run<'_> {
    fn call(&mut self, action: &str, input: Value) -> std::result::Result<Value, CallError> {
        let reference = central_action_ref(action);
        if !self
            .request
            .authorised_actions
            .iter()
            .any(|granted| granted.as_str() == reference)
        {
            return Err(CallError::Refused(format!(
                "the Routine's admitted authority does not grant {reference}; nothing was called"
            )));
        }
        let need = self
            .method
            .credentials
            .iter()
            .find(|need| need.actions.iter().any(|a| a.as_str() == reference));
        // Every child starts without ambient owner credentials; only the one
        // Action that declared a need receives its bound value. (The runner
        // applies removals after sets, so the bound variable is never also
        // in the removal list.)
        let mut process = SystemRunner::new().with_timeout(std::time::Duration::from_secs(120));
        let withheld = std::iter::once("CENTRAL_NATIVE_TOKEN")
            .chain(self.method.credentials.iter().map(|need| need.env.as_str()));
        for env in withheld {
            if need.is_none_or(|need| need.env != env) {
                process = process.with_env_removed(env);
            }
        }
        if let Some(need) = need {
            let bindings = aikit_store::RoutineCredentialStore::new(self.runner.home.clone())
                .bindings(&self.request.routine_ref)
                .map_err(|error| CallError::Refused(error.to_string()))?;
            let location = bindings.get(&need.env).ok_or_else(|| {
                CallError::Refused(format!(
                    "{reference} needs {} and no location is bound for {}; bind one with \
                     `aikit routine credential {} --env {} --location file:/ABSOLUTE/PATH`",
                    need.env, self.request.routine_ref, self.request.routine_ref, need.env
                ))
            })?;
            let secret = SecretLocation::parse(location)
                .and_then(|location| location.resolve())
                .map_err(|error| {
                    CallError::Refused(format!(
                        "{} could not be read from {location}: {error}",
                        need.env
                    ))
                })?;
            process = process.with_env(need.env.clone(), secret.expose());
        }
        let argv = vec![
            self.runner.ctrl.clone(),
            "--json".to_owned(),
            "--root".to_owned(),
            self.runner.central_root.display().to_string(),
            "action".to_owned(),
            "run".to_owned(),
            action.to_owned(),
            input.to_string(),
        ];
        let result = process.run(&argv).map_err(|error| CallError::Owner {
            code: error.code().to_owned(),
            message: error.to_string(),
        });
        let outcome = result.and_then(|output| {
            let envelope: Value =
                serde_json::from_str(output.stdout.trim()).map_err(|_| CallError::Owner {
                    code: "central.action_invalid_output".into(),
                    message: format!(
                        "exit {} without a JSON envelope: {}",
                        output.status,
                        output.stderr.trim()
                    ),
                })?;
            if envelope.get("ok").and_then(Value::as_bool) == Some(true) {
                Ok(envelope.get("data").cloned().unwrap_or(Value::Null))
            } else {
                Err(CallError::Owner {
                    code: envelope
                        .pointer("/error/code")
                        .and_then(Value::as_str)
                        .unwrap_or("central.action_failed")
                        .to_owned(),
                    message: envelope
                        .pointer("/error/message")
                        .and_then(Value::as_str)
                        .unwrap_or("the action refused without a message")
                        .to_owned(),
                })
            }
        });
        self.calls.push(ActionCall {
            action: action.to_owned(),
            input,
            ok: outcome.is_ok(),
            code: outcome.as_ref().err().and_then(|error| match error {
                CallError::Owner { code, .. } => Some(code.clone()),
                CallError::Refused(_) => None,
            }),
            message: outcome.as_ref().err().map(ToString::to_string),
            credential_env: need.map(|need| need.env.clone()),
        });
        outcome
    }

    fn factory_command(
        &mut self,
        action: &str,
        binding: &FactoryMethodBinding,
    ) -> std::result::Result<Value, CallError> {
        if !self
            .request
            .authorised_actions
            .iter()
            .any(|granted| granted.as_str() == action)
        {
            return Err(CallError::Refused(format!(
                "the Routine's admitted authority does not grant {action}; nothing was called"
            )));
        }
        let verb = if action == FACTORY_COLLECT {
            "collect"
        } else {
            "field"
        };
        let input = json!({"project_world_ref":binding.project_world_ref,"state":binding.state,"policy":binding.policy});
        let mut process = SystemRunner::new().with_timeout(std::time::Duration::from_secs(600));
        process = process.with_env_removed("CENTRAL_NATIVE_TOKEN");
        for need in &self.method.credentials {
            process = process.with_env_removed(&need.env);
        }
        let argv = vec![
            self.runner.factory.clone(),
            "telemetry".into(),
            verb.into(),
            binding.state.display().to_string(),
            "--policy".into(),
            binding.policy.display().to_string(),
            "--json".into(),
        ];
        let outcome = process
            .run(&argv)
            .map_err(|error| CallError::Owner {
                code: error.code().to_owned(),
                message: error.to_string(),
            })
            .and_then(|output| {
                if output.status != 0 {
                    return Err(CallError::Owner {
                        code: "factory.command_failed".into(),
                        message: format!(
                            "factory telemetry {verb} exited {}: {}",
                            output.status,
                            output.stderr.trim()
                        ),
                    });
                }
                serde_json::from_str::<Value>(&output.stdout).map_err(|error| CallError::Owner {
                    code: "factory.invalid_json".into(),
                    message: format!("factory telemetry {verb} did not return JSON: {error}"),
                })
            });
        self.calls.push(ActionCall {
            action: action.into(),
            input,
            ok: outcome.is_ok(),
            code: outcome.as_ref().err().and_then(|e| match e {
                CallError::Owner { code, .. } => Some(code.clone()),
                CallError::Refused(_) => None,
            }),
            message: outcome.as_ref().err().map(ToString::to_string),
            credential_env: None,
        });
        outcome
    }
}

/// The civil day before `date` (`YYYY-MM-DD`), by calendar arithmetic only.
fn previous_civil_day(date: &str) -> Option<String> {
    let day: jiff::civil::Date = date.parse().ok()?;
    day.yesterday().ok().map(|previous| previous.to_string())
}

impl NativeActionRunner {
    fn factory_policy_cadence(
        run: &Run<'_>,
        binding: &FactoryMethodBinding,
        collect: bool,
    ) -> Result<String> {
        use aikit_core::schedule::ScheduleShape;
        let metadata = std::fs::symlink_metadata(&binding.policy).map_err(|error| {
            AikitError::new(
                "routine.factory_policy_unavailable",
                format!("{}: {error}", binding.policy.display()),
            )
        })?;
        if !metadata.is_file() || metadata.len() > 256 * 1024 {
            return Err(AikitError::new(
                "routine.factory_policy_invalid",
                format!(
                    "{} must be a bounded regular policy file",
                    binding.policy.display()
                ),
            ));
        }
        let bytes = std::fs::read(&binding.policy).map_err(|error| {
            AikitError::new(
                "routine.factory_policy_unavailable",
                format!("{}: {error}", binding.policy.display()),
            )
        })?;
        let policy: Value = serde_json::from_slice(&bytes).map_err(|error| {
            AikitError::new(
                "routine.factory_policy_invalid",
                format!("{}: {error}", binding.policy.display()),
            )
        })?;
        let workflow = if collect { "collect" } else { "field-refresh" };
        if policy.get("schema").and_then(Value::as_str) != Some("factory.sensing-policy/v1")
            || policy.get("project_world_ref").and_then(Value::as_str)
                != Some(binding.project_world_ref.as_str())
            || policy
                .pointer(&format!("/workflows/{workflow}/enabled"))
                .and_then(Value::as_bool)
                != Some(true)
        {
            return Err(AikitError::new(
                "routine.factory_policy_invalid",
                format!(
                    "{} does not enable {workflow} for {}",
                    binding.policy.display(),
                    binding.project_world_ref
                ),
            ));
        }
        let declared = policy
            .pointer(&format!("/workflows/{workflow}/schedule"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                AikitError::new(
                    "routine.factory_cadence_missing",
                    format!("{} lacks the {workflow} cadence", binding.policy.display()),
                )
            })?;
        let saved = run.request.time_schedule.as_ref().ok_or_else(|| {
            AikitError::new(
                "routine.factory_schedule_missing",
                "Factory native Routine must have a saved AIKit schedule",
            )
        })?;
        let actual = match &saved.schedule {
            ScheduleShape::Every { interval_ms } => format!("every:{interval_ms}"),
            ScheduleShape::Cron { expression } => format!("cron:{expression}"),
            ScheduleShape::Daily { time } => format!("daily:{time}"),
            ScheduleShape::Once { .. } => {
                return Err(AikitError::new(
                    "routine.factory_schedule_invalid",
                    "Factory sensing cadence cannot be a one-shot Schedule",
                ))
            }
        };
        if declared != actual {
            return Err(AikitError::new("routine.factory_cadence_changed", format!("{workflow} policy cadence {declared} differs from saved Routine schedule {actual}; reprove and update the Routine")));
        }
        Ok(format!("blake3:{}", blake3::hash(&bytes).to_hex()))
    }

    fn factory_sensing(&self, run: &mut Run<'_>, collect: bool) -> (RunStatus, Value) {
        use aikit_store::now_context::{
            FactorySensingProjection, FACTORY_SENSING_PROJECTION_SCHEMA,
        };
        let Some(binding) = run.method.factory.clone() else {
            return (
                RunStatus::Failed,
                json!({"stage":"binding","error":"Factory Method has no project binding"}),
            );
        };
        let policy_revision = match Self::factory_policy_cadence(run, &binding, collect) {
            Ok(revision) => revision,
            Err(error) => {
                return (
                    RunStatus::Failed,
                    json!({"stage":"policy-cadence","error":error.to_string()}),
                )
            }
        };
        let Some(redis_path) = crate::inhabitation::world_redis_config_path(None, Some(&self.home))
        else {
            return (
                RunStatus::Failed,
                json!({"stage":"redis-config","error":"Factory sensing needs AIKIT_WORLD_REDIS_CONFIG or AIKit home's redis-now.json"}),
            );
        };
        let redis = match crate::inhabitation::open_world_store(&redis_path) {
            Ok(redis) => redis,
            Err(error) => {
                return (
                    RunStatus::Failed,
                    json!({"stage":"redis-config","error":error.to_string()}),
                )
            }
        };
        // Capture the hot-field version before any owner read. A later publisher
        // cannot overwrite a field read against an older owner moment.
        let expected = match redis
            .store
            .factory_sensing_version(&binding.project_world_ref, redis.secret.as_ref())
        {
            Ok(version) => version,
            Err(error) => {
                return (
                    RunStatus::Failed,
                    json!({"stage":"redis-version","error":error.to_string()}),
                )
            }
        };
        if collect {
            let collection = match run.factory_command(FACTORY_COLLECT, &binding) {
                Ok(collection) => collection,
                Err(error) => {
                    return (
                        RunStatus::Failed,
                        json!({"stage":"collect","error":error.to_string()}),
                    )
                }
            };
            if collection.get("schema").and_then(Value::as_str)
                != Some("factory.signal-collection/v1")
                || collection.get("project_world_ref").and_then(Value::as_str)
                    != Some(binding.project_world_ref.as_str())
            {
                return (
                    RunStatus::Failed,
                    json!({"stage":"collect-validation","error":"Factory collection did not return the bound ProjectWorld and collection schema"}),
                );
            }
        }
        let field = match run.factory_command(FACTORY_FIELD, &binding) {
            Ok(field) => field,
            Err(error) => {
                return (
                    RunStatus::Failed,
                    json!({"stage":"field","error":error.to_string()}),
                )
            }
        };
        let Some(source_revision) = field
            .get("source_revision")
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            return (
                RunStatus::Failed,
                json!({"stage":"field","error":"Factory field lacks source_revision"}),
            );
        };
        let projection = FactorySensingProjection {
            schema: FACTORY_SENSING_PROJECTION_SCHEMA.into(),
            project_world_ref: binding.project_world_ref.clone(),
            version: expected.saturating_add(1),
            source_revision: source_revision.clone(),
            field,
            published_at_unix_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|v| v.as_millis() as u64)
                .unwrap_or(0),
        };
        if let Err(error) = projection.validate() {
            return (
                RunStatus::Failed,
                json!({"stage":"field-validation","error":error.to_string()}),
            );
        }
        match Self::factory_policy_cadence(run, &binding, collect) {
            Ok(current) if current == policy_revision => {}
            Ok(_) => {
                return (
                    RunStatus::Failed,
                    json!({"stage":"policy-changed","error":"Factory policy changed during this native run; reread the owner and retry"}),
                )
            }
            Err(error) => {
                return (
                    RunStatus::Failed,
                    json!({"stage":"policy-changed","error":error.to_string()}),
                )
            }
        }
        match redis
            .store
            .publish_factory_sensing(&projection, expected, redis.secret.as_ref())
        {
            Ok(version) => (
                RunStatus::Completed,
                json!({"stage":"published","project_world_ref":binding.project_world_ref,"source_revision":source_revision,"policy_revision":policy_revision,"version":version,"cursor":projection.field.get("cursor"),"counts":projection.field.get("counts")}),
            ),
            Err(error) => (
                RunStatus::Failed,
                json!({"stage":"redis-publish","error":error.to_string(),"project_world_ref":binding.project_world_ref,"source_revision":source_revision,"expected_version":expected}),
            ),
        }
    }
    fn day_rollover(&self, run: &mut Run<'_>) -> (RunStatus, Value) {
        // 1. The recognised civil-time policy, read fresh: its revision is the
        //    basis the Day is opened against.
        let policy = match run.call(TIME_POLICY, json!({})) {
            Ok(policy) => policy,
            Err(error) => {
                return (
                    RunStatus::Failed,
                    json!({ "stage": "time-policy", "error": error.to_string() }),
                )
            }
        };
        let Some(revision) = policy
            .get("revision")
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            return (
                RunStatus::Failed,
                json!({ "stage": "time-policy", "error": "central.time.policy answered without a revision" }),
            );
        };
        // 2. Open (or confirm) the civil Day under that exact policy revision.
        let day = match run.call(
            DAY_ENSURE,
            json!({ "expected_time_policy_revision": revision }),
        ) {
            Ok(day) => day,
            Err(error) => {
                return (
                    RunStatus::Failed,
                    json!({ "stage": "day-ensure", "error": error.to_string(), "time_policy_revision": revision }),
                )
            }
        };
        let civil_date = day
            .pointer("/temporal/civil_date")
            .and_then(Value::as_str)
            .or_else(|| day.pointer("/today/civil_date").and_then(Value::as_str))
            .map(str::to_owned);
        let Some(civil_date) = civil_date else {
            return (
                RunStatus::Failed,
                json!({ "stage": "day-ensure", "error": "central.day.ensure answered without a civil date" }),
            );
        };
        let Some(previous) = previous_civil_day(&civil_date) else {
            return (
                RunStatus::Failed,
                json!({ "stage": "day-ensure", "error": format!("{civil_date} is not a civil date") }),
            );
        };
        let day_summary = json!({
            "day_ref": day.get("day_ref"),
            "civil_date": civil_date,
            "created": day.get("created"),
            "today_advanced": day.get("today_advanced"),
            "tasks_carried_or_ticked": day.get("tasks_carried_or_ticked"),
            "now_cleared_or_archived": day.get("now_cleared_or_archived"),
            "now_horizon": day.get("now_horizon"),
            "time_policy_revision": revision,
        });
        // 3. Every Project whose ProjectCentral carries a NOW field.
        let world = match run.call(WORLD, json!({})) {
            Ok(world) => world,
            Err(error) => {
                return (
                    RunStatus::Failed,
                    json!({ "stage": "world", "day": day_summary, "error": error.to_string() }),
                )
            }
        };
        let projects: Vec<String> = world
            .pointer("/work/projects")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|project| {
                project
                    .pointer("/projectcentral/now/present")
                    .and_then(Value::as_bool)
                    == Some(true)
            })
            .filter_map(|project| {
                project
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .collect();
        // 4. Close yesterday into today in each Project NOW field: live
        //    handoffs carry, resolved ones release — Central's rule, not ours.
        let mut closes = Vec::new();
        let mut failed = false;
        for project in &projects {
            let input = json!({ "project": project, "day": previous, "next_day": civil_date });
            match run.call(NOW_ROLLOVER, input) {
                Ok(report) => closes.push(json!({
                    "project": project,
                    "outcome": "closed",
                    "day_record": report.get("day_record"),
                    "carried": report.get("carried"),
                    "removed": report.get("removed"),
                    "protected": report.get("protected"),
                    "now_horizon": report.get("now_horizon"),
                })),
                Err(CallError::Owner { code, message }) if message.contains("already closed") => {
                    closes.push(json!({ "project": project, "outcome": "already-closed", "code": code, "detail": message }))
                }
                Err(error) => {
                    failed = true;
                    closes.push(json!({ "project": project, "outcome": "failed", "error": error.to_string() }))
                }
            }
        }
        (
            if failed {
                RunStatus::Failed
            } else {
                RunStatus::Completed
            },
            json!({
                "stage": "complete",
                "day": day_summary,
                "closed_day": previous,
                "projects": closes,
                "not_done": [
                    "no task completed or ticked",
                    "no Return recognised",
                    "no clearing closed, completed or archived",
                    "no root Day document closed (central.day.lifecycle is human-only)",
                    "no model or encounter spawned",
                ],
            }),
        )
    }
}

impl RoutineRunner for NativeActionRunner {
    fn run(&self, request: RoutineRunRequest) -> RoutineRunOutcome {
        let Some(method) = request.native.clone() else {
            return RoutineRunOutcome {
                status: RunStatus::Failed,
                detail: "the native runner was handed a Method without a native body; nothing ran"
                    .into(),
            };
        };
        let mut run = Run {
            runner: self,
            request: &request,
            method: &method,
            calls: Vec::new(),
        };
        let (status, result) = match method.body {
            NativeBody::CentralDayRollover => self.day_rollover(&mut run),
            NativeBody::FactoryCollect => self.factory_sensing(&mut run, true),
            NativeBody::FactoryFieldRefresh => self.factory_sensing(&mut run, false),
        };
        let receipt = json!({
            "schema": NATIVE_RUN_RECEIPT_SCHEMA,
            "runner": "native",
            "method_body": method.body.as_str(),
            "routine_ref": request.routine_ref.to_string(),
            "invocation_ref": request.invocation_ref.to_string(),
            "method_ref": request.method_ref.to_string(),
            "method_revision": request.method_revision.to_string(),
            "central_root": self.central_root.display().to_string(),
            "status": match status {
                RunStatus::Completed => "completed",
                RunStatus::Failed => "failed",
                RunStatus::Unreturned => "unreturned",
            },
            "result": result,
            "calls": run.calls,
        });
        let path = write_receipt(&self.home, &request.invocation_ref, &receipt);
        RoutineRunOutcome {
            status,
            detail: json!({
                "runner": "native",
                "method_body": method.body.as_str(),
                "receipt": path.as_ref().map(|path| path.display().to_string()).ok(),
                "receipt_write_error": path.err().map(|error| error.to_string()),
                "result": receipt["result"],
            })
            .to_string(),
        }
    }
}

fn write_receipt(
    home: &aikit_store::AikitHome,
    invocation_ref: &ResourceRef,
    receipt: &Value,
) -> Result<PathBuf> {
    let dir = NativeActionRunner::receipts_dir(home);
    std::fs::create_dir_all(&dir).map_err(|error| {
        AikitError::new(
            "routine.native_receipt_write_failed",
            format!("{}: {error}", dir.display()),
        )
    })?;
    let path = receipt_path(home, invocation_ref);
    let bytes = serde_json::to_vec_pretty(receipt).map_err(|error| {
        AikitError::new("routine.native_receipt_write_failed", error.to_string())
    })?;
    std::fs::write(&path, bytes).map_err(|error| {
        AikitError::new(
            "routine.native_receipt_write_failed",
            format!("{}: {error}", path.display()),
        )
    })?;
    Ok(path)
}

/// `aikit routine credential <ROUTINE> --env ENV (--location LOC | --clear)`.
/// Refuses a variable the Routine's Method does not declare a need for.
pub fn bind_credential(
    home: &aikit_store::AikitHome,
    method: Option<&NativeMethod>,
    routine_ref: &ResourceRef,
    env: &str,
    location: Option<&str>,
) -> Result<Value> {
    let Some(method) = method else {
        return Err(AikitError::new(
            "routine.credential_not_native",
            format!(
                "{routine_ref} runs a Method without a native body; only native bodies receive owner credentials"
            ),
        ));
    };
    if !method.credentials.iter().any(|need| need.env == env) {
        return Err(AikitError::new(
            "routine.credential_not_declared",
            format!(
                "the {} body declares no need for {env}; declared: {}",
                method.body.as_str(),
                method
                    .credentials
                    .iter()
                    .map(|need| need.env.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ));
    }
    if let Some(location) = location {
        // Store only what parses as a location; never read it here.
        SecretLocation::parse(location)?;
    }
    let bindings =
        aikit_store::RoutineCredentialStore::new(home.clone()).set(routine_ref, env, location)?;
    Ok(json!({
        "routine": routine_ref.to_string(),
        "bindings": bindings,
        "note": "locations only; the value is read when the owner child is spawned and never stored",
    }))
}

/// Where a native run's receipt for `invocation_ref` lives.
pub fn receipt_path(home: &aikit_store::AikitHome, invocation_ref: &ResourceRef) -> PathBuf {
    let name = blake3::hash(invocation_ref.as_str().as_bytes()).to_hex()[..24].to_string();
    NativeActionRunner::receipts_dir(home).join(format!("{name}.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capsule(metadata: &str) -> aikit_core::Capsule {
        aikit_core::Capsule::from_toml_str(&format!(
            "schema = 1\nid = \"skill/test/day\"\nkind = \"skill\"\nname = \"Day\"\n\
             description = \"METHOD: test\"\n[skill]\nroot = \"payload\"\n{metadata}"
        ))
        .unwrap()
    }

    const FULL: &str = r#"
[metadata.native-method]
schema = "aikit.native-method/v1"
body = "central-day-rollover"
actions = ["central:action/central.time.policy", "central:action/central.day.ensure", "central:action/central.world", "central:action/projectcentral.now.rollover"]
[[metadata.native-method.credentials]]
env = "CENTRAL_NATIVE_TOKEN"
actions = ["central:action/central.day.ensure"]
"#;

    #[test]
    fn a_capsule_without_the_declaration_is_not_native() {
        assert!(NativeMethod::from_capsule(&capsule("")).unwrap().is_none());
    }

    #[test]
    fn the_day_declaration_names_its_actions_and_its_one_credential_need() {
        let method = NativeMethod::from_capsule(&capsule(FULL)).unwrap().unwrap();
        assert_eq!(method.body, NativeBody::CentralDayRollover);
        assert_eq!(method.actions.len(), 4);
        assert_eq!(method.credentials[0].env, "CENTRAL_NATIVE_TOKEN");
        assert_eq!(
            method.credentials[0].actions[0].as_str(),
            "central:action/central.day.ensure"
        );
    }

    #[test]
    fn a_body_that_would_call_an_undeclared_action_is_refused() {
        let missing = FULL.replace("\"central:action/central.world\", ", "");
        assert_eq!(
            NativeMethod::from_capsule(&capsule(&missing))
                .unwrap_err()
                .code(),
            "routine.native_method_invalid"
        );
        let unknown = FULL.replace("central-day-rollover", "reflect-on-the-day");
        assert_eq!(
            NativeMethod::from_capsule(&capsule(&unknown))
                .unwrap_err()
                .code(),
            "routine.native_method_invalid"
        );
    }

    #[test]
    fn factory_native_method_binds_exact_project_and_owner_paths() {
        let metadata = r#"
[metadata.native-method]
schema = "aikit.native-method/v1"
body = "factory-collect"
actions = ["factory:action/telemetry.collect", "factory:action/telemetry.field"]
[metadata.native-method.factory]
state = "/tmp/factory-state.json"
policy = "/tmp/ProjectCentral/user/factory-policy.json"
project_world_ref = "project:Alpha"
"#;
        let method = NativeMethod::from_capsule(&capsule(metadata))
            .unwrap()
            .unwrap();
        assert_eq!(method.body, NativeBody::FactoryCollect);
        assert_eq!(method.factory.unwrap().project_world_ref, "project:Alpha");
        let missing = metadata.replace("\"factory:action/telemetry.collect\", ", "");
        assert_eq!(
            NativeMethod::from_capsule(&capsule(&missing))
                .unwrap_err()
                .code(),
            "routine.native_method_invalid"
        );
        let relative = metadata.replace("/tmp/factory-state.json", "factory-state.json");
        assert_eq!(
            NativeMethod::from_capsule(&capsule(&relative))
                .unwrap_err()
                .code(),
            "routine.native_method_invalid"
        );
        let root = metadata
            .replace("factory-collect", "factory-field-refresh")
            .replace("\"factory:action/telemetry.collect\", ", "")
            .replace("project:Alpha", "control:root");
        let root = NativeMethod::from_capsule(&capsule(&root))
            .unwrap()
            .unwrap();
        assert_eq!(root.body, NativeBody::FactoryFieldRefresh);
        assert_eq!(root.factory.unwrap().project_world_ref, "control:root");
    }

    #[test]
    fn the_closed_day_is_the_civil_day_before_by_calendar_arithmetic() {
        assert_eq!(
            previous_civil_day("2026-09-24").as_deref(),
            Some("2026-09-23")
        );
        assert_eq!(
            previous_civil_day("2026-03-01").as_deref(),
            Some("2026-02-28")
        );
        assert_eq!(
            previous_civil_day("2027-01-01").as_deref(),
            Some("2026-12-31")
        );
        assert!(previous_civil_day("not-a-day").is_none());
    }

    #[test]
    fn a_credential_the_body_does_not_declare_cannot_be_bound() {
        let dir = tempfile::tempdir().unwrap();
        let home = aikit_store::AikitHome::at(dir.path().join("home"));
        let method = NativeMethod::from_capsule(&capsule(FULL)).unwrap().unwrap();
        let routine = ResourceRef::parse("routine/day").unwrap();
        assert_eq!(
            bind_credential(
                &home,
                Some(&method),
                &routine,
                "OPENAI_API_KEY",
                Some("file:/x")
            )
            .unwrap_err()
            .code(),
            "routine.credential_not_declared"
        );
        assert_eq!(
            bind_credential(
                &home,
                None,
                &routine,
                "CENTRAL_NATIVE_TOKEN",
                Some("file:/x")
            )
            .unwrap_err()
            .code(),
            "routine.credential_not_native"
        );
        let bound = bind_credential(
            &home,
            Some(&method),
            &routine,
            "CENTRAL_NATIVE_TOKEN",
            Some("file:/secure/token"),
        )
        .unwrap();
        assert_eq!(
            bound["bindings"]["CENTRAL_NATIVE_TOKEN"],
            "file:/secure/token"
        );
    }
}
