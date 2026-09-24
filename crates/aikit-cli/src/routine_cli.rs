//! The Routine authoring and reconciliation verbs: `aikit method prove`,
//! `aikit routine create|list|show|enable|disable|run-now|reprove|delete|
//! import-foreign`, and the gateway dispatcher tick.
//!
//! These are the CLI face over stores and the dispatcher core; every refusal
//! carries an existing `routine.*` code in plain words, and every mutation
//! answers through the ordinary `--json` envelope.

use std::path::PathBuf;

use serde_json::{json, Value};

use aikit_core::method::method_payload;
use aikit_core::resource::routine::{
    MethodProofInput, ProvenMethodBasis, Routine, RoutineAuthority, RoutineSchedulerBinding,
    RoutineSchedulerState, RoutineState, RoutineTrigger,
};
use aikit_core::resource::{ProviderRef, ResourceRef, SourceRef};
use aikit_core::schedule::{ScheduleRecord, TIME_SCHEDULE_VERSION};
use aikit_core::{AikitError, Result};
use aikit_store::{ForeignAdoption, RoutineStore, StoredRoutine};

use crate::routine_dispatch::{
    CatalogMethodResolver, CtrlOccurrenceSource, MethodResolver, OccurrenceSource,
    ResidentEncounterRunner, RoutineDispatcher, RoutineRunner, AIKIT_GATEWAY_PROVIDER,
    TICK_INTERVAL_MS,
};
use crate::routine_native::{
    FactoryMethodBinding, MethodSelectedRunner, NativeActionRunner, NativeBody,
};

/// The trigger a create call declares: either a full `aikit.time-schedule/v1`
/// record or a plain RoutineTrigger.
#[derive(Debug, Clone, PartialEq)]
pub enum TriggerSpec {
    Manual,
    Event { event_ref: String },
    External { trigger_ref: String },
    Schedule(ScheduleRecord),
}

pub fn parse_trigger(value: Value) -> Result<TriggerSpec> {
    if value.get("schema").and_then(Value::as_str) == Some(TIME_SCHEDULE_VERSION) {
        let record: ScheduleRecord = serde_json::from_value(value).map_err(|error| {
            AikitError::new(
                "routine.trigger_invalid",
                format!("invalid {TIME_SCHEDULE_VERSION} record: {error}"),
            )
        })?;
        // Serde restores the record without its constructor's checks; the
        // shape must still be a valid time-shape before it is stored.
        record.validate()?;
        return Ok(TriggerSpec::Schedule(record));
    }
    let kind = value.get("kind").and_then(Value::as_str).ok_or_else(|| {
        AikitError::new(
            "routine.trigger_invalid",
            "the trigger JSON must be an aikit.time-schedule/v1 record or carry a kind: \
                 manual | event | external",
        )
    })?;
    match kind {
        "manual" => Ok(TriggerSpec::Manual),
        "event" => {
            let event_ref = value
                .get("event_ref")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    AikitError::new(
                        "routine.trigger_invalid",
                        "an event trigger carries event_ref (aikit.routine-event/v1:client:kind)",
                    )
                })?
                .to_owned();
            Ok(TriggerSpec::Event { event_ref })
        }
        "external" => {
            let trigger_ref = value
                .get("trigger_ref")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    AikitError::new(
                        "routine.trigger_invalid",
                        "an external trigger carries trigger_ref",
                    )
                })?
                .to_owned();
            Ok(TriggerSpec::External { trigger_ref })
        }
        other => Err(AikitError::new(
            "routine.trigger_invalid",
            format!("unknown trigger kind `{other}`; expected manual, event, external or an {TIME_SCHEDULE_VERSION} record"),
        )),
    }
}

fn parse_json<T: serde::de::DeserializeOwned>(raw: &str, label: &str) -> Result<T> {
    let text = if let Some(path) = raw.strip_prefix('@') {
        std::fs::read_to_string(path).map_err(|error| {
            AikitError::new(
                "cli.structured_json_unreadable",
                format!("could not read {label} from {path}: {error}"),
            )
        })?
    } else {
        raw.to_owned()
    };
    serde_json::from_str(&text).map_err(|error| {
        AikitError::new(
            "cli.structured_json_invalid",
            format!("invalid {label} JSON: {error}"),
        )
    })
}

fn parse_json_value(raw: &str, label: &str) -> Result<Value> {
    let text = if let Some(path) = raw.strip_prefix('@') {
        std::fs::read_to_string(path).map_err(|error| {
            AikitError::new(
                "cli.structured_json_unreadable",
                format!("could not read {label} from {path}: {error}"),
            )
        })?
    } else {
        raw.to_owned()
    };
    serde_json::from_str(&text).map_err(|error| {
        AikitError::new(
            "cli.structured_json_invalid",
            format!("invalid {label} JSON: {error}"),
        )
    })
}

fn now_unix_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_millis() as i64)
        .unwrap_or(0)
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
        "routine".into()
    } else {
        out
    }
}

/// The catalogue's Method-classified capsules with their payload text, for
/// Method matching on import.
pub fn catalog_methods(
    home: &aikit_store::AikitHome,
) -> Result<Vec<(ResourceRef, String, String)>> {
    use aikit_core::catalog::Catalog;
    let load = crate::app::load_catalog(home, None)?;
    let mut methods = Vec::new();
    for capsule in load.catalog.capsules() {
        if let Some(payload) = method_payload(&capsule.description) {
            methods.push((
                ResourceRef::parse(capsule.id.to_string())?,
                capsule.name.clone(),
                format!("{} {payload}", capsule.name),
            ));
        }
    }
    Ok(methods)
}

/// `aikit method prove --method <ref> --proof-json ...`
pub fn method_prove(
    home: &aikit_store::AikitHome,
    method: &str,
    proof_json: &str,
) -> Result<Value> {
    let method_ref = ResourceRef::parse(method)?;
    let resolver = CatalogMethodResolver { home: home.clone() };
    let resolved = resolver.resolve(&method_ref)?;
    let input: MethodProofInput = parse_json(proof_json, "Method proof input")?;
    let basis = aikit_core::resource::routine::prove_method(&resolved, input)?;
    serde_json::to_value(&basis).map_err(|error| {
        AikitError::new(
            "cli.routine_json_failed",
            format!("could not encode ProvenMethodBasis: {error}"),
        )
    })
}

/// `aikit routine create` — the Routine sits in Draft until enabled.
#[allow(clippy::too_many_arguments)]
pub fn create(
    home: &aikit_store::AikitHome,
    name: &str,
    description: &str,
    method: &str,
    proof_json: &str,
    trigger_json: &str,
    authority_json: &str,
    agent_profile: Option<&str>,
    context_scope: &[String],
) -> Result<Value> {
    let method_ref = ResourceRef::parse(method)?;
    let resolver = CatalogMethodResolver { home: home.clone() };
    let resolved = resolver.resolve(&method_ref)?;
    let proof: ProvenMethodBasis = parse_json(proof_json, "ProvenMethodBasis")?;
    let trigger_value = parse_json_value(trigger_json, "trigger")?;
    let trigger_spec = parse_trigger(trigger_value)?;
    let authority: RoutineAuthority = parse_json(authority_json, "RoutineAuthority")?;
    let agent_profile_ref = match agent_profile {
        Some(raw) => Some(ResourceRef::parse(raw)?),
        None => None,
    };
    let mut context_scope_refs = Vec::new();
    for scope in context_scope {
        context_scope_refs.push(ResourceRef::parse(scope)?);
    }

    let routine_ref = ResourceRef::parse(format!("routine/{}", slug(name)))?;
    let store = RoutineStore::new(home.clone());
    if store.get(&routine_ref).is_ok() {
        return Err(AikitError::new(
            "routine.already_exists",
            format!(
                "a Routine named {routine_ref} is already stored; rename this one or delete the \
                 existing Routine first"
            ),
        ));
    }

    let trigger = match &trigger_spec {
        TriggerSpec::Manual => RoutineTrigger::Manual,
        TriggerSpec::Event { event_ref } => RoutineTrigger::Event {
            event_ref: event_ref.clone(),
        },
        TriggerSpec::External { trigger_ref } => RoutineTrigger::External {
            trigger_ref: trigger_ref.clone(),
        },
        TriggerSpec::Schedule(record) => RoutineTrigger::Schedule {
            schedule_ref: record.schedule_ref.to_string(),
        },
    };

    let routine = Routine::new(
        routine_ref.clone(),
        SourceRef::parse(format!("source:aikit:routines/{routine_ref}"))?,
        None,
        name,
        description,
        &resolved,
        proof,
        trigger,
        authority,
        agent_profile_ref,
        context_scope_refs,
    )?;

    // New automations default to the AIKit gateway dispatcher: the binding
    // names the material timer that will fire them, so a Manual or Event
    // Routine the gateway dispatches carries the same honest provenance.
    let mut stored = match trigger_spec {
        TriggerSpec::Schedule(schedule) => {
            let mut record = StoredRoutine::new(routine, Some(schedule), None)?;
            record
                .routine
                .set_scheduler_binding(RoutineSchedulerBinding {
                    provider: ProviderRef::parse(AIKIT_GATEWAY_PROVIDER)?,
                    provider_job_id: None,
                    observed_state: RoutineSchedulerState::Planned,
                })?;
            record
        }
        _ => {
            let mut record = StoredRoutine::new(routine, None, None)?;
            record
                .routine
                .set_scheduler_binding(RoutineSchedulerBinding {
                    provider: ProviderRef::parse(AIKIT_GATEWAY_PROVIDER)?,
                    provider_job_id: None,
                    observed_state: RoutineSchedulerState::Planned,
                })?;
            record
        }
    };
    stored.restamp_revision()?;
    store.put(stored)?;

    Ok(json!({
        "routine": routine_ref.to_string(),
        "state": "draft",
        "note": "created in Draft; enable it with `aikit routine enable` and a fresh authority receipt",
    }))
}

fn parse_state_filter(raw: &str) -> Result<RoutineState> {
    match raw {
        "draft" => Ok(RoutineState::Draft),
        "enabled" => Ok(RoutineState::Enabled),
        "disabled" => Ok(RoutineState::Disabled),
        "stale-proof" => Ok(RoutineState::StaleProof),
        other => Err(AikitError::new(
            "routine.state_unknown",
            format!("unknown state `{other}`; use draft, enabled, disabled or stale-proof"),
        )),
    }
}

/// The read-only reconciliation face over both foreign harness stores: which
/// timers exist, which Routine claims them, and why an unclaimed timer is
/// unreconciled. Never writes the harness stores.
pub fn foreign_reconciliation(home: &aikit_store::AikitHome) -> Result<Value> {
    let home_dir = foreign_home_dir();
    let store = RoutineStore::new(home.clone());
    let routines = store.list()?;
    let mut providers = Vec::new();
    for provider in [
        crate::foreign_cron::ForeignProvider::OpenClawCron,
        crate::foreign_cron::ForeignProvider::HermesCron,
    ] {
        let mut jobs = Vec::new();
        for job in crate::foreign_cron::read_store(provider, &home_dir)? {
            let claimed = routines.iter().any(|record| {
                record.foreign_adoption.as_ref().is_some_and(|adoption| {
                    adoption.provider == provider.as_str() && adoption.provider_job_id == job.job_id
                }) || record.routine.scheduler.as_ref().is_some_and(|binding| {
                    binding.provider.as_str() == provider.provider_ref()
                        && binding.provider_job_id.as_deref() == Some(&job.job_id)
                })
            });
            let reason = if claimed {
                None
            } else {
                Some(match job.refusal_reason() {
                    Err(error) => error.to_string(),
                    Ok(()) => format!(
                        "no Routine claims this timer; import it with `aikit routine \
                         import-foreign --provider {} --job-id {}` or retire it in the harness \
                         ({})",
                        provider.as_str(),
                        job.job_id,
                        provider.retirement_hint(&job.job_id)
                    ),
                })
            };
            jobs.push(json!({
                "job_id": job.job_id,
                "name": job.name,
                "active": job.active,
                "schedule": job.schedule.as_ref().map(|shape| serde_json::to_value(shape).unwrap_or(Value::Null)),
                "payload_text": job.payload_text,
                "reconciled": claimed,
                "state": if claimed { "reconciled" } else { "unreconciled" },
                "reason": reason,
            }));
        }
        providers.push(json!({
            "provider": provider.as_str(),
            "store": provider.store_path(&home_dir).display().to_string(),
            "jobs": jobs,
        }));
    }
    Ok(json!({ "providers": providers }))
}

/// `aikit routine list [--state ...]`
pub fn list(home: &aikit_store::AikitHome, state: Option<&str>) -> Result<Value> {
    let store = RoutineStore::new(home.clone());
    let state_filter = state.map(parse_state_filter).transpose()?;
    let mut routines = Vec::new();
    for record in store.list()? {
        if let Some(filter) = state_filter {
            if record.routine.state != filter {
                continue;
            }
        }
        routines.push(routine_summary(&record));
    }
    let foreign = foreign_reconciliation(home)?;
    Ok(json!({
        "routines": routines,
        "foreign_reconciliation": foreign,
    }))
}

fn routine_summary(record: &StoredRoutine) -> Value {
    json!({
        "routine": record.routine.id.to_string(),
        "name": record.routine.name,
        "method": record.routine.method.to_string(),
        "method_revision": record.routine.method_revision.to_string(),
        "state": record.routine.state,
        "trigger": record.routine.trigger,
        "scheduler": record.routine.scheduler,
        "foreign_adoption": record.foreign_adoption,
        "revision": record.routine.revision,
    })
}

/// `aikit routine show <ref> — full record plus, when Central is reachable,
/// the next occurrences with the exact policy revision they were read at.
pub fn show(home: &aikit_store::AikitHome, routine_ref: &str) -> Result<Value> {
    let reference = ResourceRef::parse(routine_ref)?;
    let store = RoutineStore::new(home.clone());
    let record = store.get(&reference)?;
    let mut data = routine_summary(&record);
    if let Some(object) = data.as_object_mut() {
        object.insert(
            "proof".into(),
            serde_json::to_value(&record.routine.proof).unwrap_or(Value::Null),
        );
        object.insert(
            "authority".into(),
            serde_json::to_value(&record.routine.authority).unwrap_or(Value::Null),
        );
        object.insert(
            "explanation".into(),
            serde_json::to_value(record.routine.explain()).unwrap_or(Value::Null),
        );
        if let Some(schedule) = &record.time_schedule {
            object.insert(
                "time_schedule".into(),
                serde_json::to_value(schedule).unwrap_or(Value::Null),
            );
        }
        // Which body the Method selects, and where a native body's declared
        // credentials are bound (locations only).
        let native = CatalogMethodResolver { home: home.clone() }
            .native_method(&record.routine.method)
            .ok()
            .flatten();
        object.insert(
            "method_body".into(),
            match &native {
                Some(native) => json!(format!("native:{}", native.body.as_str())),
                None => json!("encounter"),
            },
        );
        if native.is_some() {
            object.insert(
                "credential_bindings".into(),
                serde_json::to_value(
                    aikit_store::RoutineCredentialStore::new(home.clone())
                        .bindings(&record.routine.id)?,
                )
                .unwrap_or(Value::Null),
            );
        }
    }
    // Next occurrences are best-effort: a Central that cannot be reached is an
    // honest error field, never an invented schedule.
    match next_occurrences(&record) {
        Ok(occurrences) => {
            if let Some(object) = data.as_object_mut() {
                object.insert("next_occurrences".into(), occurrences);
            }
        }
        Err(error) => {
            if let Some(object) = data.as_object_mut() {
                object.insert("occurrence_error".into(), Value::String(error.to_string()));
            }
        }
    }
    Ok(data)
}

fn next_occurrences(record: &StoredRoutine) -> Result<Value> {
    let Some(schedule) = &record.time_schedule else {
        return Ok(json!([]));
    };
    let schedule_value = serde_json::to_value(&schedule.schedule)
        .map_err(|error| AikitError::new("routine.schedule_encoding", error.to_string()))?;
    let source = CtrlOccurrenceSource {
        central_root: CtrlOccurrenceSource::discover()?,
    };
    let now = now_unix_ms();
    let reading = source.occurrences(&schedule_value, now, now + 24 * 60 * 60_000)?;
    Ok(json!({
        "time_policy_ref": reading.time_policy_ref.to_string(),
        "time_policy_revision": reading.time_policy_revision.to_string(),
        "occurrences": reading.occurrences.iter().map(|occurrence| json!({
            "occurrence_ref": occurrence.occurrence_ref.to_string(),
            "due_unix_ms": occurrence.due_unix_ms,
        })).collect::<Vec<_>>(),
    }))
}

/// `aikit routine enable <ref> --authority-json ...`
pub fn enable(
    home: &aikit_store::AikitHome,
    routine_ref: &str,
    authority_json: &str,
) -> Result<Value> {
    let reference = ResourceRef::parse(routine_ref)?;
    let authority: RoutineAuthority = parse_json(authority_json, "RoutineAuthority")?;
    let store = RoutineStore::new(home.clone());
    let mut record = store.get(&reference)?;
    let resolver = CatalogMethodResolver { home: home.clone() };
    let method = resolver.resolve(&record.routine.method)?;
    // Plain refusal before any write when the authority names Actions outside
    // the proven Method.
    for action in &authority.action_refs {
        if !method.actions.contains(action) {
            return Err(AikitError::new(
                "routine.action_not_in_method",
                format!(
                    "Routine authority cannot introduce an Action outside the proven Method \
                     ({action} is not one of the Method's Actions)"
                ),
            )
            .with("action", action.to_string()));
        }
    }
    record.routine.authority = authority;
    record.routine.enable(&method)?;
    record.restamp_revision()?;
    let enabled = store.put(record)?;
    Ok(json!({
        "routine": reference.to_string(),
        "state": enabled.routine.state,
        "note": if enabled.routine.scheduler.as_ref().is_some_and(|binding| binding.provider.as_str() == "provider:aikit-gateway") {
            "enabled and bound to the gateway dispatcher; it will fire when the gateway runs"
        } else {
            "enabled; the gateway dispatcher does not own this Routine's binding"
        },
    }))
}

/// `aikit routine disable <ref>`
pub fn disable(home: &aikit_store::AikitHome, routine_ref: &str) -> Result<Value> {
    let reference = ResourceRef::parse(routine_ref)?;
    let store = RoutineStore::new(home.clone());
    let mut record = store.get(&reference)?;
    record.routine.disable();
    record.restamp_revision()?;
    store.put(record)?;
    Ok(json!({ "routine": reference.to_string(), "state": "disabled" }))
}

/// `aikit routine reprove <ref> --proof-json ...` — returns to Disabled.
pub fn reprove(
    home: &aikit_store::AikitHome,
    routine_ref: &str,
    proof_json: &str,
) -> Result<Value> {
    let reference = ResourceRef::parse(routine_ref)?;
    let proof: ProvenMethodBasis = parse_json(proof_json, "ProvenMethodBasis")?;
    let store = RoutineStore::new(home.clone());
    let mut record = store.get(&reference)?;
    let resolver = CatalogMethodResolver { home: home.clone() };
    let method = resolver.resolve(&record.routine.method)?;
    record.routine.reprove(&method, proof)?;
    record.restamp_revision()?;
    let reproved = store.put(record)?;
    Ok(json!({
        "routine": reference.to_string(),
        "state": reproved.routine.state,
        "method_revision": reproved.routine.method_revision.to_string(),
        "note": "reproven; explicitly enable again — reproof never silently resumes automation",
    }))
}

/// `aikit routine delete <ref>` — refuses while Enabled.
pub fn delete(home: &aikit_store::AikitHome, routine_ref: &str) -> Result<Value> {
    let reference = ResourceRef::parse(routine_ref)?;
    let store = RoutineStore::new(home.clone());
    let record = store.get(&reference)?;
    if record.routine.state == RoutineState::Enabled {
        return Err(AikitError::new(
            "routine.delete_enabled",
            format!(
                "{reference} is Enabled; disable it before deleting — a live automation is not \
                 removed silently"
            ),
        ));
    }
    store.delete(&reference)?;
    Ok(json!({ "routine": reference.to_string(), "deleted": true }))
}

fn foreign_home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// `aikit routine run-now <ref>` — a manual observation through the same gate,
/// dispatched by the same runner the scheduled path uses.
pub fn run_now(home: &aikit_store::AikitHome, routine_ref: &str) -> Result<Value> {
    let reference = ResourceRef::parse(routine_ref)?;
    let dispatcher = production_dispatcher(home.clone())?;
    let dispatch = dispatcher.run_now(&reference, now_unix_ms())?;
    serde_json::to_value(dispatch)
        .map_err(|error| AikitError::new("cli.routine_json_failed", error.to_string()))
}

/// `aikit routine import-foreign ...` (addendum A-6).
#[allow(clippy::too_many_arguments)]
pub fn import_foreign(
    home: &aikit_store::AikitHome,
    provider: &str,
    job_id: &str,
    method: Option<&str>,
    proof_json: Option<&str>,
    adopt: bool,
    report: bool,
) -> Result<Value> {
    let provider = crate::foreign_cron::ForeignProvider::parse(provider)?;
    let home_dir = foreign_home_dir();
    let job = crate::foreign_cron::read_job(provider, job_id, &home_dir)?;
    if report {
        let reconciliation = foreign_reconciliation(home)?;
        return Ok(json!({
            "report": true,
            "job": {
                "provider": provider.as_str(),
                "job_id": job.job_id,
                "name": job.name,
                "active": job.active,
                "schedule": job.schedule.as_ref().map(|shape| serde_json::to_value(shape).unwrap_or(Value::Null)),
                "payload_text": job.payload_text,
                "reconciled": false,
                "reason": job.refusal_reason().err().map(|error| error.to_string()),
            },
            "foreign_reconciliation": reconciliation,
        }));
    }

    // The import gate: representable schedule, a Method match, and a proven
    // basis. Anything less is refused and the job stays harness-native,
    // visible as unreconciled.
    job.refusal_reason()?;
    let schedule = job.schedule.clone().ok_or_else(|| {
        AikitError::new(
            "routine.import_unrepresentable_schedule",
            format!(
                "job {} carries a schedule shape this import cannot read",
                job.job_id
            ),
        )
    })?;
    let method_ref = match method {
        Some(raw) => ResourceRef::parse(raw)?,
        None => infer_method(home, &job)?,
    };
    let proof_raw = proof_json.ok_or_else(|| {
        AikitError::new(
            "routine.import_proof_required",
            format!(
                "importing {} requires a proven basis: run the Method once, prove it with \
                 `aikit method prove`, then pass --proof-json; without proof the job stays \
                 harness-native and appears as unreconciled",
                job.job_id
            ),
        )
        .with("job_id", job.job_id.clone())
    })?;
    let proof: ProvenMethodBasis = parse_json(proof_raw, "ProvenMethodBasis")?;

    let store = RoutineStore::new(home.clone());
    let routine_ref = ResourceRef::parse(format!("routine/foreign-{}", slug(&job.name)))?;
    if store.get(&routine_ref).is_ok() {
        return Err(AikitError::new(
            "routine.already_exists",
            format!(
                "a Routine for this job is already stored as {routine_ref}; delete it first if \
                 you mean to re-import"
            ),
        ));
    }

    let resolver = CatalogMethodResolver { home: home.clone() };
    let resolved = resolver.resolve(&method_ref)?;
    // The Routine sits Disabled with its authority pending: only `routine
    // enable` supplies the owner's receipt.
    let routine = Routine::new(
        routine_ref.clone(),
        SourceRef::parse(format!("source:aikit:routines/{routine_ref}"))?,
        None,
        if job.name.trim().is_empty() {
            format!("Imported {} job {}", provider.as_str(), job.job_id)
        } else {
            job.name.clone()
        },
        format!(
            "Imported from {} job {} (read-only reconciliation; the harness timer is retired by \
             the owner in the harness)",
            provider.as_str(),
            job.job_id
        ),
        &resolved,
        proof,
        RoutineTrigger::Schedule {
            schedule_ref: format!("schedule/foreign-{}", slug(&job.job_id)),
        },
        RoutineAuthority {
            authority_ref: ResourceRef::parse(format!(
                "authority:routine:pending-{}",
                slug(&job.job_id)
            ))?,
            revision: None,
            action_refs: vec![ResourceRef::parse(
                crate::scoped_invocation::NATIVE_CAPABILITY_RUN_ACTION,
            )?],
            granted: false,
            unattended: false,
        },
        None,
        vec![],
    )?;
    // Imports are Disabled until the owner explicitly enables them with a
    // fresh authority receipt — never Draft-into-service by accident.
    let mut routine = routine;
    routine.disable();
    let schedule_record = ScheduleRecord::new(
        ResourceRef::parse(format!("schedule/foreign-{}", slug(&job.job_id)))?,
        schedule,
        None,
    )?;
    let mut stored = StoredRoutine::new(routine, Some(schedule_record), None)?;
    stored
        .routine
        .set_scheduler_binding(RoutineSchedulerBinding {
            provider: ProviderRef::parse(provider.provider_ref())?,
            provider_job_id: Some(job.job_id.clone()),
            observed_state: if job.active {
                RoutineSchedulerState::Active
            } else {
                RoutineSchedulerState::Degraded
            },
        })?;
    if adopt {
        stored.foreign_adoption = Some(ForeignAdoption {
            provider: provider.as_str().to_owned(),
            provider_job_id: job.job_id.clone(),
            adopted_at: jiff::Timestamp::now().to_string(),
        });
    }
    stored.restamp_revision()?;
    store.put(stored)?;

    let mut receipt = json!({
        "routine": routine_ref.to_string(),
        "provider": provider.as_str(),
        "job_id": job.job_id,
        "state": "disabled",
        "binding": provider.provider_ref(),
        "note": "imported Disabled; enable it with a fresh authority receipt, and the harness timer stays as it is until you retire it",
    });
    if adopt {
        receipt["adopted"] = Value::Bool(true);
        receipt["retirement"] = Value::String(format!(
            "after the Routine's first admitted scheduled run, retire the harness timer: {}",
            provider.retirement_hint(job_id)
        ));
    }
    Ok(receipt)
}

/// Infer the Method a foreign job's payload runs: the one Method-classified
/// capsule whose id or name appears in the payload text. Ambiguity is refused,
/// never guessed.
fn infer_method(
    home: &aikit_store::AikitHome,
    job: &crate::foreign_cron::ForeignJob,
) -> Result<ResourceRef> {
    let hay = format!("{}\n{}", job.name, job.payload_text).to_lowercase();
    let mut matches = Vec::new();
    for (id, name, text) in catalog_methods(home)? {
        let id_hit = hay.contains(&id.as_str().to_lowercase());
        let name_hit = !name.is_empty() && hay.contains(&name.to_lowercase());
        if id_hit || name_hit {
            matches.push((id, text));
        }
    }
    match matches.len() {
        0 => Err(AikitError::new(
            "routine.import_no_method_match",
            format!(
                "no Method-classified capsule matches job {}'s payload; name one with --method",
                job.job_id
            ),
        )),
        1 => Ok(matches.remove(0).0),
        _ => Err(AikitError::new(
            "routine.import_method_ambiguous",
            format!(
                "several Methods match job {}: {}; name one with --method",
                job.job_id,
                matches
                    .iter()
                    .map(|(id, _)| id.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )),
    }
}

/// The production dispatcher: Central resolves time, the catalogue resolves
/// Methods, and the Method selects its body — a native body runs owner
/// Actions through `ctrl`, every other Method opens a resident encounter.
pub type ProductionDispatcher = RoutineDispatcher<
    CtrlOccurrenceSource,
    CatalogMethodResolver,
    MethodSelectedRunner<ResidentEncounterRunner, NativeActionRunner>,
>;

pub fn production_dispatcher(home: aikit_store::AikitHome) -> Result<ProductionDispatcher> {
    let central_root = CtrlOccurrenceSource::discover()?;
    let resolver = CatalogMethodResolver { home: home.clone() };
    let runner = MethodSelectedRunner {
        encounter: ResidentEncounterRunner { home: home.clone() },
        native: NativeActionRunner::from_env(home.clone(), central_root.clone()),
    };
    Ok(RoutineDispatcher::new(
        home,
        CtrlOccurrenceSource { central_root },
        resolver,
        runner,
    ))
}

/// `aikit routine credential <ROUTINE> --env ENV (--location LOC | --clear)`.
pub fn credential(
    home: &aikit_store::AikitHome,
    routine_ref: &str,
    env: &str,
    location: Option<&str>,
) -> Result<Value> {
    let reference = ResourceRef::parse(routine_ref)?;
    let record = RoutineStore::new(home.clone()).get(&reference)?;
    let resolver = CatalogMethodResolver { home: home.clone() };
    let native = resolver.native_method(&record.routine.method)?;
    crate::routine_native::bind_credential(home, native.as_ref(), &reference, env, location)
}

/// The gateway serve loop's tick hook: one deterministic dispatcher pass every
/// interval. Failures are returned to the loop (remembered, never fatal).
pub struct GatewayDispatcherTick {
    pub dispatcher: ProductionDispatcher,
}

impl aikit_adapters::GatewayTick for GatewayDispatcherTick {
    fn tick(&self) -> aikit_core::Result<Value> {
        gateway_pass(&self.dispatcher, now_unix_ms())
    }
}

/// One dispatcher pass for `aikit gateway tick` (A-4).
pub fn gateway_tick(home: &aikit_store::AikitHome) -> Result<Value> {
    let dispatcher = production_dispatcher(home.clone())?;
    gateway_pass(&dispatcher, now_unix_ms())
}

fn gateway_pass(dispatcher: &ProductionDispatcher, now: i64) -> Result<Value> {
    let report = dispatcher.tick(now)?;
    let mut value = serde_json::to_value(report)
        .map_err(|error| AikitError::new("cli.routine_json_failed", error.to_string()))?;
    let resolver = CatalogMethodResolver {
        home: dispatcher.home().clone(),
    };
    let runner = NativeActionRunner::from_env(dispatcher.home().clone(), PathBuf::new());
    let changes = factory_change_pass(dispatcher, &resolver, now, |binding| {
        observe_factory_change(dispatcher.home(), &runner, binding)
    });
    value["factory_changes"] = changes;
    Ok(value)
}

/// A read of each enabled, project-bound Factory event Routine's native owner
/// cursor. The scheduled Routine remains the recovery and cadence fallback;
/// this pass admits an event only when the owner cursor differs from Redis.
/// No probe result is treated as an authority grant: event_pass revalidates
/// Method, proof, Routine state and action authority before native execution.
pub fn factory_change_pass<O, M, R, F>(
    dispatcher: &RoutineDispatcher<O, M, R>,
    resolver: &M,
    now: i64,
    mut observe: F,
) -> Value
where
    O: OccurrenceSource,
    M: MethodResolver,
    R: RoutineRunner,
    F: FnMut(&FactoryMethodBinding) -> Result<Option<Value>>,
{
    let mut considered = Vec::new();
    let mut changed = Vec::new();
    let mut dispatched = Vec::new();
    let mut failures = Vec::new();
    let records = match RoutineStore::new(dispatcher.home().clone()).list() {
        Ok(records) => records,
        Err(error) => {
            return json!({"considered":considered,"changed":changed,"dispatched":dispatched,"failures":[error.to_string()]})
        }
    };
    let mut project_counts = std::collections::BTreeMap::<String, usize>::new();
    for record in &records {
        if record.routine.state == RoutineState::Enabled {
            if let RoutineTrigger::Event { event_ref } = &record.routine.trigger {
                if let Some(project) =
                    event_ref.strip_prefix("aikit.routine-event/v1:factory:field-changed:")
                {
                    *project_counts.entry(project.to_owned()).or_default() += 1;
                }
            }
        }
    }
    for record in records {
        if record.routine.state != RoutineState::Enabled {
            continue;
        }
        let RoutineTrigger::Event { event_ref } = &record.routine.trigger else {
            continue;
        };
        if !event_ref.starts_with("aikit.routine-event/v1:factory:field-changed:") {
            continue;
        }
        let native = match resolver.native_method(&record.routine.method) {
            Ok(Some(native)) if native.body == NativeBody::FactoryFieldRefresh => native,
            Ok(_) => continue,
            Err(error) => {
                failures.push(format!("{}: {error}", record.routine.id));
                continue;
            }
        };
        let Some(binding) = native.factory else {
            failures.push(format!(
                "{}: Factory Method has no binding",
                record.routine.id
            ));
            continue;
        };
        let expected_event = format!(
            "aikit.routine-event/v1:factory:field-changed:{}",
            binding.project_world_ref
        );
        if *event_ref != expected_event {
            failures.push(format!(
                "{}: Factory change trigger does not match bound ProjectWorld",
                record.routine.id
            ));
            continue;
        }
        if project_counts
            .get(&binding.project_world_ref)
            .copied()
            .unwrap_or(0)
            != 1
        {
            failures.push(format!(
                "{}: more than one Factory change Routine is enabled for {}",
                record.routine.id, binding.project_world_ref
            ));
            continue;
        }
        considered.push(binding.project_world_ref.clone());
        let field = match observe(&binding) {
            Ok(Some(field)) => field,
            Ok(None) => continue,
            Err(error) => {
                failures.push(format!("{}: {error}", record.routine.id));
                continue;
            }
        };
        changed.push(binding.project_world_ref.clone());
        // A stable bucket suppresses duplicate gateway ticks in one interval.
        // A failed admission/publication or Redis loss can retry on the next
        // bucket even when the owner cursor itself has not changed again.
        let packet = json!({
            "project_world_ref": binding.project_world_ref,
            "owner_cursor": field.get("cursor"),
            "source_revision": field.get("source_revision"),
            "observed_bucket_unix_ms": now.div_euclid(TICK_INTERVAL_MS) * TICK_INTERVAL_MS,
        });
        match dispatcher.event_pass("factory", "field-changed", &packet, now) {
            Ok(records) => dispatched.extend(records),
            Err(error) => failures.push(format!("{}: {error}", record.routine.id)),
        }
    }
    json!({"considered":considered,"changed":changed,"dispatched":dispatched,"failures":failures})
}

/// Returns a changed native field, or no event when Redis already carries its
/// cursor. An absent Redis projection is a change even if an earlier event
/// admitted the same owner cursor before the loss.
pub fn observe_factory_change(
    home: &aikit_store::AikitHome,
    runner: &NativeActionRunner,
    binding: &FactoryMethodBinding,
) -> Result<Option<Value>> {
    let field = runner.observe_factory_field(binding)?;
    let redis_path =
        crate::inhabitation::world_redis_config_path(None, Some(home)).ok_or_else(|| {
            AikitError::new(
                "routine.factory_redis_unavailable",
                "Redis World config unavailable",
            )
        })?;
    let redis = crate::inhabitation::open_world_store(&redis_path)?;
    let hot = redis
        .store
        .read_factory_sensing(&binding.project_world_ref, redis.secret.as_ref())?;
    if hot.as_ref().and_then(|p| p.field.get("cursor")) == field.get("cursor") {
        Ok(None)
    } else {
        Ok(Some(field))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigger_parsing_accepts_schedule_records_and_plain_kinds() {
        let schedule = parse_trigger(json!({
            "schema": TIME_SCHEDULE_VERSION,
            "schedule_ref": "schedule/daily-0600",
            "schedule": { "kind": "daily", "time": "06:00" }
        }))
        .unwrap();
        assert!(matches!(schedule, TriggerSpec::Schedule(_)));
        assert!(matches!(
            parse_trigger(json!({ "kind": "manual" })).unwrap(),
            TriggerSpec::Manual
        ));
        let event = parse_trigger(json!({
            "kind": "event",
            "event_ref": "aikit.routine-event/v1:claude:Stop"
        }))
        .unwrap();
        assert!(matches!(event, TriggerSpec::Event { .. }));
        assert!(parse_trigger(json!({ "kind": "hourly" })).is_err());
        assert!(parse_trigger(json!({ "schema": "aikit.time-schedule/v1", "schedule_ref": "s", "schedule": { "kind": "daily", "time": "25:00" } })).is_err());
    }

    #[test]
    fn slugs_are_stable_and_bounded() {
        assert_eq!(slug("Daily Nara Flow"), "daily-nara-flow");
        assert_eq!(slug("!!!"), "routine");
    }
}
