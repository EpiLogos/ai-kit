//! Lossless intake of Factory's public developmental CLI contracts.
//!
//! AIKit does not reproduce Factory's developmental model. The native Factory
//! executable opens and validates its owner state; this adapter traverses the
//! versioned public readings, checks the identity/provenance relations it joins,
//! and carries each complete owner JSON document as observed Resource evidence.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

use aikit_core::{
    resource::{
        Eligibility, OwnerRef, ProviderOffer, ProviderRef, ProviderState, ResourceDescriptor,
        ResourceKind, ResourceLocator, ResourceRecord, ResourceSource, SourceAuthority, SourceRef,
        SourceRevision, SourceState,
    },
    AikitError, Result,
};
use serde_json::{Map, Value};

use crate::runner::CommandRunner;

pub const FACTORY_DEVELOPMENTAL_PROVIDER: &str = "factory.developmental-local-provider/v1";
pub const FACTORY_DEVELOPMENTAL_SCHEMA_SHA256: &str =
    "6d29a65744f70af16b5a348ebbd0a803ddd0b295c7bda7587316c9e8a0d0c0ec";
pub const FACTORY_DEVELOPMENTAL_CONTRACT_OWNER_REVISION: &str =
    "12a721dbbb51e3c70d52ef00220efa859ef930fd";
pub const FACTORY_COMMISSION_REQUEST_SCHEMA_SHA256: &str =
    "78dd34ae441ab585c82fcc1f30614ca4d116b347af896ba4ab2404566a530c69";
pub const FACTORY_COMMISSION_SCHEMA_SHA256: &str =
    "51c45a601685dbf24ebb766d9cc059f9b81a7258f27819f61b69c450cb7aa214";

const PROJECT: &str = "factory.project-reading/v1";
const JOURNEY: &str = "factory.journey-reading/v1";
const RUN: &str = "factory.run-reading/v1";
const UNIT_LIST: &str = "factory.workflow-unit-list-reading/v1";
const UNIT: &str = "factory.workflow-unit-reading/v1";
const TELEMETRY: &str = "factory.execution-telemetry-reading/v1";
const ROUTINE_CONTINUATION: &str = "factory.routine-continuation-reading/v1";
const COMMISSION_READING: &str = "factory.commission-reading/v1";
const COMMISSION_REQUEST: &str = "factory.commission-request/v1";
const COMMISSION_RECEIPT: &str = "factory.commission-receipt/v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactoryDevelopmentalBinding {
    pub executable: PathBuf,
    pub state: PathBuf,
    pub project_ref: String,
}

impl FactoryDevelopmentalBinding {
    pub fn new(
        executable: impl Into<PathBuf>,
        state: impl Into<PathBuf>,
        project_ref: impl Into<String>,
    ) -> Result<Self> {
        let binding = Self {
            executable: executable.into(),
            state: state.into(),
            project_ref: project_ref.into(),
        };
        require_ref(&binding.project_ref, Some("project"), "projectRef")?;
        if binding.executable.as_os_str().is_empty() || binding.state.as_os_str().is_empty() {
            return Err(error(
                "factory.developmental_invalid_binding",
                "Factory executable and state path must be non-empty",
            ));
        }
        Ok(binding)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FactoryOwnedReading {
    pub contract: String,
    pub subject_ref: String,
    pub owner_revision: u64,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FactoryDevelopmentalObservation {
    pub readings: Vec<FactoryOwnedReading>,
    pub resources: Vec<ResourceRecord>,
}

/// Result of crossing AIKit's Factory-work boundary. The mutation and all
/// minted identities remain Factory-owned; AIKit returns the exact receipt and
/// then reads the resulting owner state through the same public CLI.
#[derive(Debug, Clone, PartialEq)]
pub struct FactoryWorkStart {
    pub receipt: Value,
    pub observation: FactoryDevelopmentalObservation,
}

pub fn start_factory_work<R: CommandRunner>(
    runner: &R,
    executable: impl Into<PathBuf>,
    state: impl Into<PathBuf>,
    request_file: impl Into<PathBuf>,
) -> Result<FactoryWorkStart> {
    let executable = executable.into();
    let state = state.into();
    let request_file = request_file.into();
    if executable.as_os_str().is_empty()
        || state.as_os_str().is_empty()
        || request_file.as_os_str().is_empty()
    {
        return Err(error(
            "factory.commission_invalid_binding",
            "Factory executable, state path and Commission request file must be non-empty",
        ));
    }
    let request: Value =
        serde_json::from_slice(&std::fs::read(&request_file).map_err(|error| {
            self::error(
                "factory.commission_request_unreadable",
                format!(
                    "could not read Factory Commission request {}: {error}",
                    request_file.display()
                ),
            )
        })?)
        .map_err(|error| {
            self::error(
                "factory.commission_request_invalid_json",
                format!("Factory Commission request is not valid JSON: {error}"),
            )
        })?;
    let request_body = object(&request, "Factory Commission request")?;
    require_equal(
        text(request_body, "contract")?,
        COMMISSION_REQUEST,
        "request.contract",
    )?;
    let request_ref = text(request_body, "requestRef")?.to_string();
    require_nonempty(&request_ref, "requestRef")?;
    require_equal(text(request_body, "writeOwner")?, "factory", "writeOwner")?;

    let argv = vec![
        executable.display().to_string(),
        "development".into(),
        "commission".into(),
        state.display().to_string(),
        request_file.display().to_string(),
        "--json".into(),
    ];
    let output = runner
        .run(&argv)?
        .require(&argv, "factory.commission_owner_operation_failed")?;
    let receipt: Value = serde_json::from_str(&output.stdout).map_err(|error| {
        self::error(
            "factory.commission_invalid_receipt",
            format!("Factory returned invalid Commission receipt JSON: {error}"),
        )
    })?;
    let project_ref = validate_commission_receipt(&receipt, &request, &request_ref)?;
    let binding = FactoryDevelopmentalBinding::new(executable, state, project_ref)?;
    let mut observation = read_factory_developmental(runner, &binding)?;
    let commission_readback = invoke(
        runner,
        &binding,
        "commission-read",
        Some(&request_ref),
        None,
    )?;
    let receipt_commission = value_object(
        object(&receipt, "Commission receipt")?
            .get("commission")
            .ok_or_else(|| missing("commission"))?,
        "Commission",
    )?;
    let journey_ref = text(receipt_commission, "journeyRef")?;
    let run_ref = text(receipt_commission, "runRef")?;
    let mut observed_runs = BTreeSet::new();
    observed_runs.insert(run_ref.to_string());
    let commission = validate_commission_reading(
        &commission_readback,
        &request_ref,
        &binding.project_ref,
        journey_ref,
        &observed_runs,
    )?;
    observation.readings.push(reading(
        COMMISSION_READING,
        &request_ref,
        positive_u64(commission, "revision")?,
        commission_readback,
    ));
    observation.resources = project_resources(&binding, &observation.readings)?;
    let preserved = observation
        .readings
        .iter()
        .find(|reading| {
            reading.contract == COMMISSION_READING && reading.subject_ref == request_ref
        })
        .ok_or_else(|| {
            error(
                "factory.commission_missing_readback",
                "Factory did not publish the newly admitted Commission through commission-read",
            )
        })?;
    let preserved_commission = object(&preserved.value, "Commission readback")?
        .get("commission")
        .ok_or_else(|| missing("commission"))?;
    let receipt_commission = object(&receipt, "Commission receipt")?
        .get("commission")
        .ok_or_else(|| missing("commission"))?;
    if preserved_commission != receipt_commission {
        return Err(error(
            "factory.commission_readback_mismatch",
            "Factory Commission receipt and canonical readback disagree",
        ));
    }
    Ok(FactoryWorkStart {
        receipt,
        observation,
    })
}

pub fn read_factory_developmental<R: CommandRunner>(
    runner: &R,
    binding: &FactoryDevelopmentalBinding,
) -> Result<FactoryDevelopmentalObservation> {
    let mut readings = Vec::new();
    let project = invoke(runner, binding, "project", Some(&binding.project_ref), None)?;
    validate_common(&project, PROJECT, &binding.project_ref)?;
    let project_revision = positive_u64(object(&project, "project")?, "projectRevision")?;
    readings.push(reading(
        PROJECT,
        &binding.project_ref,
        project_revision,
        project.clone(),
    ));

    let mut journey_summaries = BTreeMap::new();
    let mut run_refs = BTreeSet::new();
    let mut journey_invocations = BTreeMap::new();
    let mut run_invocations = BTreeMap::new();
    for summary in array(object(&project, "project")?, "journeys")? {
        let summary = value_object(summary, "journey summary")?;
        let journey_ref = text(summary, "journeyRef")?;
        require_ref(journey_ref, Some("journey"), "journeyRef")?;
        if journey_summaries
            .insert(journey_ref.to_string(), summary.clone())
            .is_some()
        {
            return Err(duplicate("journey", journey_ref));
        }
        for run_ref in canonical_ref_set(summary, "runRefs", "run")? {
            run_refs.insert(run_ref);
        }
    }

    for (journey_ref, summary) in &journey_summaries {
        let value = invoke(runner, binding, "journey", Some(journey_ref), None)?;
        validate_common(&value, JOURNEY, &binding.project_ref)?;
        let body = object(&value, "journey")?;
        require_equal(text(body, "journeyRef")?, journey_ref, "journeyRef")?;
        let revision = positive_u64(body, "revision")?;
        if revision != positive_u64(summary, "revision")?
            || text(body, "status")? != text(summary, "status")?
            || text(body, "frontier")? != text(summary, "frontier")?
        {
            return Err(error(
                "factory.developmental_relation_mismatch",
                format!("Journey {journey_ref} detail disagrees with its Project summary"),
            ));
        }
        let detailed_runs = canonical_ref_set(body, "runRefs", "run")?;
        if detailed_runs != canonical_ref_set(summary, "runRefs", "run")? {
            return Err(error(
                "factory.developmental_relation_mismatch",
                format!("Journey {journey_ref} Run refs disagree with its Project summary"),
            ));
        }
        run_refs.extend(detailed_runs);
        for invocation_ref in string_array(body, "routineInvocationRefs")? {
            require_nonempty(&invocation_ref, "routineInvocationRef")?;
            bind_declared_identity(
                &mut journey_invocations,
                invocation_ref,
                journey_ref,
                "Journey",
            )?;
        }
        readings.push(reading(JOURNEY, journey_ref, revision, value));
    }

    for run_ref in &run_refs {
        let value = invoke(runner, binding, "run", Some(run_ref), None)?;
        validate_common(&value, RUN, &binding.project_ref)?;
        let body = object(&value, "run")?;
        require_equal(text(body, "runRef")?, run_ref, "runRef")?;
        let revision = positive_u64(body, "revision")?;
        for invocation_ref in string_array(body, "routineInvocationRefs")? {
            require_nonempty(&invocation_ref, "routineInvocationRef")?;
            bind_declared_identity(&mut run_invocations, invocation_ref, run_ref, "Run")?;
        }
        readings.push(reading(RUN, run_ref, revision, value));
    }

    if journey_invocations.keys().ne(run_invocations.keys()) {
        return Err(error(
            "factory.developmental_relation_mismatch",
            "Journey and Run Routine-invocation declarations disagree",
        ));
    }
    for (invocation_ref, declared_journey_ref) in &journey_invocations {
        let value = invoke(
            runner,
            binding,
            "routine-continuation",
            Some(invocation_ref),
            None,
        )?;
        let body = object(&value, "routine continuation")?;
        require_equal(text(body, "contract")?, ROUTINE_CONTINUATION, "contract")?;
        let continuation = value_object(
            body.get("continuation")
                .ok_or_else(|| missing("continuation"))?,
            "continuation",
        )?;
        require_equal(text(continuation, "writeOwner")?, "factory", "writeOwner")?;
        let journey_ref = text(continuation, "journeyRef")?;
        let run_ref = text(continuation, "runRef")?;
        if journey_ref != declared_journey_ref
            || run_invocations.get(invocation_ref).map(String::as_str) != Some(run_ref)
        {
            return Err(error(
                "factory.developmental_relation_mismatch",
                format!(
                    "Routine continuation {invocation_ref} disagrees with its Journey or Run declaration"
                ),
            ));
        }
        let evidence = value_object(
            continuation
                .get("invocationEvidence")
                .ok_or_else(|| missing("invocationEvidence"))?,
            "invocationEvidence",
        )?;
        require_equal(
            text(evidence, "invocation_ref")?,
            invocation_ref,
            "invocationEvidence.invocation_ref",
        )?;
        readings.push(reading(
            ROUTINE_CONTINUATION,
            invocation_ref,
            positive_u64(continuation, "revision")?,
            value,
        ));
    }

    let units = invoke(runner, binding, "workflow-units", None, None)?;
    validate_common(&units, UNIT_LIST, &binding.project_ref)?;
    let unit_list_revision = provenance_revision(&units)?;
    readings.push(reading(
        UNIT_LIST,
        &binding.project_ref,
        unit_list_revision,
        units.clone(),
    ));
    let mut unit_refs = BTreeSet::new();
    for summary in array(object(&units, "workflow-unit list")?, "units")? {
        let summary = value_object(summary, "workflow-unit summary")?;
        let unit_ref = text(summary, "workflowUnitRef")?;
        require_ref(unit_ref, Some("workflow-unit"), "workflowUnitRef")?;
        if !unit_refs.insert(unit_ref.to_string()) {
            return Err(duplicate("workflow-unit", unit_ref));
        }
    }

    let mut telemetry = BTreeMap::<String, String>::new();
    for unit_ref in &unit_refs {
        let value = invoke(runner, binding, "workflow-unit", Some(unit_ref), None)?;
        validate_common(&value, UNIT, &binding.project_ref)?;
        let body = object(&value, "workflow unit")?;
        require_equal(text(body, "workflowUnitRef")?, unit_ref, "workflowUnitRef")?;
        let revision = provenance_revision(&value)?;
        if let Some(correlation) = body.get("currentCorrelation").and_then(Value::as_object) {
            if let Some(refs) = correlation.get("telemetryRefs") {
                if !refs.is_null() {
                    for telemetry_ref in value_string_array(refs, "telemetryRefs")? {
                        require_ref(&telemetry_ref, Some("telemetry"), "telemetryRef")?;
                        if let Some(existing) =
                            telemetry.insert(telemetry_ref.clone(), unit_ref.clone())
                        {
                            if existing != *unit_ref {
                                return Err(error("factory.developmental_relation_mismatch", format!("telemetry {telemetry_ref} is related to both {existing} and {unit_ref}")));
                            }
                        }
                    }
                }
            }
        }
        readings.push(reading(UNIT, unit_ref, revision, value));
    }

    for (telemetry_ref, unit_ref) in &telemetry {
        let value = invoke(
            runner,
            binding,
            "execution-telemetry",
            Some(telemetry_ref),
            None,
        )?;
        validate_common(&value, TELEMETRY, &binding.project_ref)?;
        let body = object(&value, "execution telemetry")?;
        require_equal(text(body, "telemetryRef")?, telemetry_ref, "telemetryRef")?;
        require_equal(text(body, "workflowUnitRef")?, unit_ref, "workflowUnitRef")?;
        let run_ref = text(body, "runRef")?;
        if !run_refs.contains(run_ref) {
            return Err(error(
                "factory.developmental_relation_mismatch",
                format!("telemetry {telemetry_ref} refers to unobserved Run {run_ref}"),
            ));
        }
        readings.push(reading(
            TELEMETRY,
            telemetry_ref,
            provenance_revision(&value)?,
            value,
        ));
    }

    let resources = project_resources(binding, &readings)?;
    Ok(FactoryDevelopmentalObservation {
        readings,
        resources,
    })
}

fn invoke<R: CommandRunner>(
    runner: &R,
    binding: &FactoryDevelopmentalBinding,
    operation: &str,
    subject: Option<&str>,
    run: Option<&str>,
) -> Result<Value> {
    let mut argv = vec![
        binding.executable.display().to_string(),
        "development".into(),
        operation.into(),
        binding.state.display().to_string(),
    ];
    if let Some(subject) = subject {
        argv.push(subject.into());
    }
    if let Some(run) = run {
        argv.push(run.into());
    }
    argv.push("--json".into());
    let output = runner
        .run(&argv)?
        .require(&argv, "factory.developmental_owner_read_failed")?;
    serde_json::from_str(&output.stdout).map_err(|e| {
        error(
            "factory.developmental_invalid_json",
            format!("Factory {operation} returned invalid JSON: {e}"),
        )
    })
}

fn validate_common(value: &Value, contract: &str, project_ref: &str) -> Result<()> {
    let body = object(value, contract)?;
    require_equal(text(body, "contract")?, contract, "contract")?;
    require_equal(text(body, "projectRef")?, project_ref, "projectRef")?;
    let provenance = value_object(
        body.get("provenance")
            .ok_or_else(|| missing("provenance"))?,
        "provenance",
    )?;
    require_equal(text(provenance, "owner")?, "factory", "provenance.owner")?;
    let _ = provenance_revision(value)?;
    Ok(())
}

fn validate_commission_receipt(
    receipt: &Value,
    request: &Value,
    request_ref: &str,
) -> Result<String> {
    let body = object(receipt, "Factory Commission receipt")?;
    require_equal(text(body, "contract")?, COMMISSION_RECEIPT, "contract")?;
    if !matches!(text(body, "status")?, "applied" | "already-applied") {
        return Err(error(
            "factory.commission_invalid_receipt",
            "Factory Commission status must be applied or already-applied",
        ));
    }
    let commission = value_object(
        body.get("commission")
            .ok_or_else(|| missing("commission"))?,
        "Commission",
    )?;
    validate_commission(commission, request_ref)?;
    if commission
        .get("request")
        .ok_or_else(|| missing("commission.request"))?
        != request
    {
        return Err(error(
            "factory.commission_receipt_mismatch",
            "Factory receipt did not preserve the exact Commission request",
        ));
    }
    let request_body = object(request, "Commission request")?;
    let composition = value_object(
        request_body
            .get("centralComposition")
            .ok_or_else(|| missing("centralComposition"))?,
        "centralComposition",
    )?;
    require_equal(
        text(composition, "authorityStanding")?,
        "membership-non-authoritative",
        "centralComposition.authorityStanding",
    )?;
    let root_act = value_object(
        request_body
            .get("rootAct")
            .ok_or_else(|| missing("rootAct"))?,
        "rootAct",
    )?;
    require_equal(
        text(root_act, "standing")?,
        "commissioned-not-executed",
        "rootAct.standing",
    )?;
    Ok(text(commission, "projectRef")?.to_string())
}

fn validate_commission_reading<'a>(
    value: &'a Value,
    request_ref: &str,
    project_ref: &str,
    journey_ref: &str,
    observed_runs: &BTreeSet<String>,
) -> Result<&'a Map<String, Value>> {
    let body = object(value, "Factory Commission reading")?;
    require_equal(text(body, "contract")?, COMMISSION_READING, "contract")?;
    let commission = value_object(
        body.get("commission")
            .ok_or_else(|| missing("commission"))?,
        "Commission",
    )?;
    validate_commission(commission, request_ref)?;
    require_equal(text(commission, "projectRef")?, project_ref, "projectRef")?;
    require_equal(text(commission, "journeyRef")?, journey_ref, "journeyRef")?;
    let run_ref = text(commission, "runRef")?;
    require_ref(run_ref, Some("run"), "runRef")?;
    if !observed_runs.contains(run_ref) {
        return Err(error(
            "factory.developmental_relation_mismatch",
            format!("Commission {request_ref} refers to unobserved Run {run_ref}"),
        ));
    }
    let traversal = array(body, "traversal")?;
    if traversal.is_empty() {
        return Err(error(
            "factory.commission_invalid_reading",
            "Factory Commission traversal must not be empty",
        ));
    }
    Ok(commission)
}

fn validate_commission<'a>(
    commission: &'a Map<String, Value>,
    request_ref: &str,
) -> Result<&'a Map<String, Value>> {
    require_equal(
        text(commission, "contract")?,
        "factory.commission/v1",
        "commission.contract",
    )?;
    if positive_u64(commission, "revision")? != 1 {
        return Err(error(
            "factory.commission_invalid_reading",
            "Factory Commission revision must be 1",
        ));
    }
    for (field, kind) in [
        ("projectRef", "project"),
        ("journeyRef", "journey"),
        ("runRef", "run"),
    ] {
        require_ref(text(commission, field)?, Some(kind), field)?;
    }
    let request = value_object(
        commission
            .get("request")
            .ok_or_else(|| missing("commission.request"))?,
        "Commission request",
    )?;
    require_equal(
        text(request, "contract")?,
        COMMISSION_REQUEST,
        "commission.request.contract",
    )?;
    require_equal(
        text(request, "requestRef")?,
        request_ref,
        "commission.request.requestRef",
    )?;
    Ok(commission)
}

fn provenance_revision(value: &Value) -> Result<u64> {
    let provenance = value_object(
        object(value, "reading")?
            .get("provenance")
            .ok_or_else(|| missing("provenance"))?,
        "provenance",
    )?;
    provenance
        .get("factoryStateRevision")
        .or_else(|| provenance.get("buildStateRevision"))
        .and_then(Value::as_u64)
        .filter(|v| *v > 0)
        .ok_or_else(|| {
            error(
                "factory.developmental_invalid_reading",
                "provenance requires a positive factoryStateRevision or buildStateRevision",
            )
        })
}

fn project_resources(
    binding: &FactoryDevelopmentalBinding,
    readings: &[FactoryOwnedReading],
) -> Result<Vec<ResourceRecord>> {
    let mut records = BTreeMap::<String, ResourceRecord>::new();
    for reading in readings {
        let body = object(&reading.value, "reading")?;
        let (kind, name, description) = match reading.contract.as_str() {
            PROJECT => (
                ResourceKind::Project,
                reading.subject_ref.clone(),
                format!(
                    "Factory Project at revision {}",
                    body.get("projectRevision")
                        .and_then(Value::as_u64)
                        .unwrap_or(reading.owner_revision)
                ),
            ),
            JOURNEY => (
                ResourceKind::Journey,
                reading.subject_ref.clone(),
                format!("{} · {}", text(body, "status")?, text(body, "frontier")?),
            ),
            RUN => (
                ResourceKind::Run,
                reading.subject_ref.clone(),
                format!(
                    "{} · {}",
                    text(body, "lifecycle")?,
                    text(body, "destination")?
                ),
            ),
            UNIT => (
                ResourceKind::WorkflowUnit,
                body.get("locator")
                    .and_then(Value::as_str)
                    .unwrap_or(&reading.subject_ref)
                    .to_string(),
                text(body, "developmentalConcern")?.to_string(),
            ),
            UNIT_LIST | TELEMETRY | ROUTINE_CONTINUATION | COMMISSION_READING => continue,
            _ => {
                return Err(error(
                    "factory.developmental_unsupported_contract",
                    &reading.contract,
                ))
            }
        };
        let mut descriptor = ResourceDescriptor::new(
            aikit_core::resource::ResourceRef::parse(&reading.subject_ref)?,
            kind,
            name,
            description,
        );
        descriptor.owner = Some(OwnerRef::parse("factory")?);
        descriptor
            .sources
            .push(resource_source(binding, reading.owner_revision)?);
        annotate(&mut descriptor, reading)?;
        let mut record = ResourceRecord::new(descriptor);
        record.eligibility = Eligibility::Eligible;
        record.providers.push(provider(binding)?);
        records.insert(reading.subject_ref.clone(), record);
    }
    for reading in readings.iter().filter(|r| r.contract == RUN) {
        for action in array(object(&reading.value, "run")?, "actions")? {
            let action = value_object(action, "Factory action")?;
            let action_ref = text(action, "actionRef")?;
            let mut descriptor = ResourceDescriptor::new(
                aikit_core::resource::ResourceRef::parse(action_ref)?,
                ResourceKind::Action,
                text(action, "label")?,
                "Factory-owned developmental Action",
            );
            descriptor.owner = Some(OwnerRef::parse("factory")?);
            descriptor
                .sources
                .push(resource_source(binding, reading.owner_revision)?);
            descriptor.annotations.insert(
                "factory.action".into(),
                serde_json::to_string(action).map_err(json_error)?,
            );
            descriptor.annotations.insert(
                "factory.run-refs".into(),
                serde_json::to_string(&[&reading.subject_ref]).map_err(json_error)?,
            );
            descriptor.annotations.insert(
                "factory.owner-revision".into(),
                reading.owner_revision.to_string(),
            );
            descriptor.annotations.insert(
                "factory.contract-owner-revision".into(),
                FACTORY_DEVELOPMENTAL_CONTRACT_OWNER_REVISION.into(),
            );
            descriptor.annotations.insert(
                "factory.contract-schema-sha256".into(),
                FACTORY_DEVELOPMENTAL_SCHEMA_SHA256.into(),
            );
            let mut record = ResourceRecord::new(descriptor);
            record.eligibility = Eligibility::Eligible;
            record.providers.push(provider(binding)?);
            match records.entry(action_ref.into()) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(record);
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    if entry.get().descriptor.annotations.get("factory.action")
                        != record.descriptor.annotations.get("factory.action")
                    {
                        return Err(error("factory.developmental_conflicting_action", format!("Factory reused Action identity {action_ref} with conflicting evidence")));
                    }
                    let mut runs: BTreeSet<String> = serde_json::from_str(
                        entry
                            .get()
                            .descriptor
                            .annotations
                            .get("factory.run-refs")
                            .expect("Factory Action run refs are installed above"),
                    )
                    .map_err(json_error)?;
                    runs.insert(reading.subject_ref.clone());
                    entry.get_mut().descriptor.annotations.insert(
                        "factory.run-refs".into(),
                        serde_json::to_string(&runs).map_err(json_error)?,
                    );
                }
            }
        }
    }
    for reading in readings.iter().filter(|r| r.contract == TELEMETRY) {
        let body = object(&reading.value, "telemetry")?;
        let unit_ref = text(body, "workflowUnitRef")?;
        let record = records.get_mut(unit_ref).ok_or_else(|| {
            error(
                "factory.developmental_orphan_telemetry",
                format!(
                    "telemetry {} has no indexed WorkflowUnit",
                    reading.subject_ref
                ),
            )
        })?;
        record.descriptor.annotations.insert(
            format!("factory.telemetry.{}", reading.subject_ref),
            serde_json::to_string(&reading.value).map_err(json_error)?,
        );
    }
    for reading in readings
        .iter()
        .filter(|r| r.contract == ROUTINE_CONTINUATION)
    {
        let continuation = value_object(
            object(&reading.value, "routine continuation")?
                .get("continuation")
                .ok_or_else(|| missing("continuation"))?,
            "continuation",
        )?;
        let evidence = serde_json::to_string(&reading.value).map_err(json_error)?;
        for relation in ["journeyRef", "runRef"] {
            let related_ref = text(continuation, relation)?;
            records
                .get_mut(related_ref)
                .ok_or_else(|| {
                    error(
                        "factory.developmental_orphan_routine_continuation",
                        format!(
                            "Routine continuation {} has no indexed {relation}",
                            reading.subject_ref
                        ),
                    )
                })?
                .descriptor
                .annotations
                .insert(
                    format!("factory.routine-continuation.{}", reading.subject_ref),
                    evidence.clone(),
                );
        }
    }
    for reading in readings.iter().filter(|r| r.contract == COMMISSION_READING) {
        let body = object(&reading.value, "Commission reading")?;
        let commission = value_object(
            body.get("commission")
                .ok_or_else(|| missing("commission"))?,
            "Commission",
        )?;
        let evidence = serde_json::to_string(&reading.value).map_err(json_error)?;
        for relation in ["projectRef", "journeyRef", "runRef"] {
            let related_ref = text(commission, relation)?;
            records
                .get_mut(related_ref)
                .ok_or_else(|| {
                    error(
                        "factory.developmental_orphan_commission",
                        format!(
                            "Commission {} has no indexed {relation}",
                            reading.subject_ref
                        ),
                    )
                })?
                .descriptor
                .annotations
                .insert(
                    format!("factory.commission.{}", reading.subject_ref),
                    evidence.clone(),
                );
            let descriptor = &mut records
                .get_mut(related_ref)
                .expect("Commission relation was resolved above")
                .descriptor;
            descriptor.annotations.insert(
                "factory.commission-request-schema-sha256".into(),
                FACTORY_COMMISSION_REQUEST_SCHEMA_SHA256.into(),
            );
            descriptor.annotations.insert(
                "factory.commission-schema-sha256".into(),
                FACTORY_COMMISSION_SCHEMA_SHA256.into(),
            );
        }
    }
    Ok(records.into_values().collect())
}

fn annotate(descriptor: &mut ResourceDescriptor, reading: &FactoryOwnedReading) -> Result<()> {
    descriptor
        .annotations
        .insert("factory.contract".into(), reading.contract.clone());
    descriptor.annotations.insert(
        "factory.owner-revision".into(),
        reading.owner_revision.to_string(),
    );
    descriptor.annotations.insert(
        "factory.contract-owner-revision".into(),
        FACTORY_DEVELOPMENTAL_CONTRACT_OWNER_REVISION.into(),
    );
    descriptor.annotations.insert(
        "factory.contract-schema-sha256".into(),
        FACTORY_DEVELOPMENTAL_SCHEMA_SHA256.into(),
    );
    descriptor.annotations.insert(
        "factory.owner-reading".into(),
        serde_json::to_string(&reading.value).map_err(json_error)?,
    );
    Ok(())
}

fn resource_source(binding: &FactoryDevelopmentalBinding, revision: u64) -> Result<ResourceSource> {
    Ok(ResourceSource {
        source: SourceRef::parse(FACTORY_DEVELOPMENTAL_PROVIDER)?,
        authority: Some(SourceAuthority::Observed),
        revision: Some(SourceRevision::parse(revision.to_string())?),
        locator: Some(ResourceLocator::Path(binding.state.clone())),
        state: SourceState::Available,
    })
}
fn provider(binding: &FactoryDevelopmentalBinding) -> Result<ProviderOffer> {
    Ok(ProviderOffer {
        provider: ProviderRef::parse(FACTORY_DEVELOPMENTAL_PROVIDER)?,
        locator: Some(ResourceLocator::Path(binding.executable.clone())),
        state: ProviderState::Available,
    })
}
fn reading(
    contract: &str,
    subject_ref: &str,
    owner_revision: u64,
    value: Value,
) -> FactoryOwnedReading {
    FactoryOwnedReading {
        contract: contract.into(),
        subject_ref: subject_ref.into(),
        owner_revision,
        value,
    }
}
fn object<'a>(value: &'a Value, label: &str) -> Result<&'a Map<String, Value>> {
    value.as_object().ok_or_else(|| {
        error(
            "factory.developmental_invalid_reading",
            format!("{label} must be a JSON object"),
        )
    })
}
fn value_object<'a>(value: &'a Value, label: &str) -> Result<&'a Map<String, Value>> {
    value.as_object().ok_or_else(|| {
        error(
            "factory.developmental_invalid_reading",
            format!("{label} must be an object"),
        )
    })
}
fn text<'a>(body: &'a Map<String, Value>, field: &str) -> Result<&'a str> {
    body.get(field)
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| missing(field))
}
fn array<'a>(body: &'a Map<String, Value>, field: &str) -> Result<&'a Vec<Value>> {
    body.get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| missing(field))
}
fn string_array(body: &Map<String, Value>, field: &str) -> Result<Vec<String>> {
    value_string_array(body.get(field).ok_or_else(|| missing(field))?, field)
}
fn canonical_ref_set(
    body: &Map<String, Value>,
    field: &str,
    kind: &str,
) -> Result<BTreeSet<String>> {
    let mut refs = BTreeSet::new();
    for reference in string_array(body, field)? {
        require_ref(&reference, Some(kind), field)?;
        if !refs.insert(reference.clone()) {
            return Err(duplicate(kind, &reference));
        }
    }
    Ok(refs)
}
fn bind_declared_identity(
    declarations: &mut BTreeMap<String, String>,
    identity: String,
    subject_ref: &str,
    subject_kind: &str,
) -> Result<()> {
    if let Some(existing) = declarations.insert(identity.clone(), subject_ref.to_string()) {
        return Err(error(
            "factory.developmental_duplicate_ref",
            format!(
                "Routine invocation {identity} is declared more than once by {subject_kind} {existing} / {subject_ref}"
            ),
        ));
    }
    Ok(())
}
fn value_string_array(value: &Value, field: &str) -> Result<Vec<String>> {
    value
        .as_array()
        .ok_or_else(|| missing(field))?
        .iter()
        .map(|v| {
            v.as_str()
                .filter(|s| !s.trim().is_empty())
                .map(str::to_string)
                .ok_or_else(|| missing(field))
        })
        .collect()
}
fn positive_u64(body: &Map<String, Value>, field: &str) -> Result<u64> {
    body.get(field)
        .and_then(Value::as_u64)
        .filter(|v| *v > 0)
        .ok_or_else(|| missing(field))
}
fn require_equal(actual: &str, expected: &str, field: &str) -> Result<()> {
    if actual == expected {
        Ok(())
    } else {
        Err(error(
            "factory.developmental_relation_mismatch",
            format!("{field} `{actual}` does not match `{expected}`"),
        ))
    }
}
fn require_nonempty(value: &str, field: &str) -> Result<()> {
    if value.trim().is_empty() {
        Err(missing(field))
    } else {
        Ok(())
    }
}
fn require_ref(value: &str, kind: Option<&str>, field: &str) -> Result<()> {
    let (actual_kind, id) = value.split_once(':').ok_or_else(|| missing(field))?;
    let valid_char = |b: u8| {
        b.is_ascii_digit() || matches!(b, b'A'..=b'H' | b'J'..=b'N' | b'P'..=b'T' | b'V'..=b'Z')
    };
    if kind.is_some_and(|expected| expected != actual_kind)
        || id.len() != 26
        || !id.bytes().all(valid_char)
        || !matches!(id.as_bytes()[0], b'0'..=b'7')
    {
        return Err(error(
            "factory.developmental_invalid_ref",
            format!("{field} `{value}` is not a canonical Factory ref"),
        ));
    }
    Ok(())
}
fn missing(field: &str) -> AikitError {
    error(
        "factory.developmental_invalid_reading",
        format!("missing or invalid {field}"),
    )
}
fn duplicate(kind: &str, value: &str) -> AikitError {
    error(
        "factory.developmental_duplicate_ref",
        format!("duplicate {kind} ref {value}"),
    )
}
fn json_error(error: serde_json::Error) -> AikitError {
    self::error("factory.developmental_json", error.to_string())
}
fn error(code: &'static str, detail: impl Into<String>) -> AikitError {
    AikitError::new(code, detail.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::Output;

    const P: &str = "project:01ARZ3NDEKTSV4RRFFQ69G5FAE";
    const J: &str = "journey:01ARZ3NDEKTSV4RRFFQ69G5FAD";
    const R: &str = "run:01ARZ3NDEKTSV4RRFFQ69G5FAA";
    const U: &str = "workflow-unit:01ARZ3NDEKTSV4RRFFQ69G5FAB";
    const T: &str = "telemetry:01ARZ3NDEKTSV4RRFFQ69G5FA4";
    const I: &str = "routine-invocation:2026-09-09:test:1";

    struct OwnerCli {
        owner: &'static str,
        duplicate_journey: bool,
        summary_frontier: &'static str,
        run_missing_invocation: bool,
        unrelated_continuation: bool,
        unrelated_telemetry: bool,
    }

    impl CommandRunner for OwnerCli {
        fn run(&self, argv: &[String]) -> Result<Output> {
            let operation = argv.get(2).map(String::as_str).unwrap_or_default();
            let provenance = serde_json::json!({"owner":self.owner,"factoryStateRevision":7,"subjectRevision":3,"source":"canonical Factory state"});
            let value = match operation {
                "project" => {
                    serde_json::json!({"contract":PROJECT,"provenance":provenance,"projectRef":P,"projectRevision":3,"journeys": if self.duplicate_journey { vec![serde_json::json!({"journeyRef":J,"revision":2,"status":"active","frontier":self.summary_frontier,"runRefs":[R]}),serde_json::json!({"journeyRef":J,"revision":2,"status":"active","frontier":self.summary_frontier,"runRefs":[R]})] } else { vec![serde_json::json!({"journeyRef":J,"revision":2,"status":"active","frontier":self.summary_frontier,"runRefs":[R]})] }})
                }
                "journey" => {
                    serde_json::json!({"contract":JOURNEY,"provenance":provenance,"journeyRef":J,"revision":2,"projectRef":P,"status":"active","frontier":"ship","runRefs":[R],"routineInvocationRefs":[I]})
                }
                "run" => {
                    serde_json::json!({"contract":RUN,"provenance":provenance,"runRef":R,"revision":4,"projectRef":P,"routineInvocationRefs": if self.run_missing_invocation { Vec::<String>::new() } else { vec![I.to_string()] },"lifecycle":"active","destination":"main","actions":[{"actionRef":"action:request-more-evidence","label":"Request more evidence","currentlyApplicable":true}]})
                }
                "routine-continuation" => {
                    serde_json::json!({"contract":ROUTINE_CONTINUATION,"continuation":{"contract":"factory.routine-continuation/v1","continuationRef":"routine-continuation:01ARZ3NDEKTSV4RRFFQ69G5FA5","revision":1,"journeyRef":J,"runRef":R,"writeOwner":"factory","invocationEvidence":{"owner":"aikit","invocation_ref": if self.unrelated_continuation { "routine-invocation:other" } else { I },"proof_standing":"current-on-supplied-basis"}},"traversal":[]})
                }
                "workflow-units" => {
                    serde_json::json!({"contract":UNIT_LIST,"provenance":{"owner":self.owner,"buildStateRevision":7,"sourceBases":[]},"projectRef":P,"runFilter":null,"units":[{"workflowUnitRef":U,"locator":"ship/verify","key":"verify","workflowKey":"ship","sourceRef":"workflow-source:01ARZ3NDEKTSV4RRFFQ69G5FAC","sourceRevision":"abc","sourceDigest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","subjectRef":P,"basisRevision":"main","currentCorrelation":{}}]})
                }
                "workflow-unit" => {
                    serde_json::json!({"contract":UNIT,"provenance":{"owner":self.owner,"buildStateRevision":7,"sourceRef":"workflow-source:01ARZ3NDEKTSV4RRFFQ69G5FAC","sourceRevision":"abc","sourceDigest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","identityAlgorithm":"factory.workflow-unit.identity.blake3-ulid/v1"},"projectRef":P,"workflowUnitRef":U,"locator":"ship/verify","developmentalConcern":"verify the accepted change","currentCorrelation":{"telemetryRefs":[T]}})
                }
                "execution-telemetry" => {
                    serde_json::json!({"contract":TELEMETRY,"provenance":{"owner":self.owner,"factoryStateRevision":7,"correlationRef":"execution-correlation:01ARZ3NDEKTSV4RRFFQ69G5FA3","source":"Factory correlation"},"telemetryRef":T,"projectRef":P,"runRef":R,"workflowUnitRef": if self.unrelated_telemetry { "workflow-unit:01ARZ3NDEKTSV4RRFFQ69G5FAY" } else { U },"executionRef":"execution:verify","modelUsage":{"owner":"actuation","availability":"unavailable","observations":[],"reason":"not supplied"}})
                }
                other => return Err(error("test.unexpected_operation", other)),
            };
            Ok(Output::success(serde_json::to_string(&value).unwrap()))
        }
    }

    fn binding() -> FactoryDevelopmentalBinding {
        FactoryDevelopmentalBinding::new("factory", "/tmp/factory-state.json", P).unwrap()
    }

    #[test]
    fn traverses_owner_contracts_and_preserves_lossless_readings_resources_actions_and_telemetry() {
        let observed = read_factory_developmental(
            &OwnerCli {
                owner: "factory",
                duplicate_journey: false,
                summary_frontier: "ship",
                run_missing_invocation: false,
                unrelated_continuation: false,
                unrelated_telemetry: false,
            },
            &binding(),
        )
        .unwrap();
        assert_eq!(observed.readings.len(), 7);
        assert!(observed
            .readings
            .iter()
            .any(|reading| reading.contract == TELEMETRY
                && reading.value["modelUsage"]["availability"] == "unavailable"));
        let unit = observed
            .resources
            .iter()
            .find(|record| record.descriptor.id.as_str() == U)
            .unwrap();
        assert_eq!(unit.descriptor.kind, ResourceKind::WorkflowUnit);
        assert_eq!(unit.descriptor.owner.as_ref().unwrap().as_str(), "factory");
        assert!(unit
            .descriptor
            .annotations
            .get(&format!("factory.telemetry.{T}"))
            .unwrap()
            .contains("not supplied"));
        let run = observed
            .resources
            .iter()
            .find(|record| record.descriptor.id.as_str() == R)
            .unwrap();
        assert!(run
            .descriptor
            .annotations
            .get(&format!("factory.routine-continuation.{I}"))
            .unwrap()
            .contains("current-on-supplied-basis"));
        let action = observed
            .resources
            .iter()
            .find(|record| record.descriptor.id.as_str() == "action:request-more-evidence")
            .unwrap();
        assert_eq!(action.descriptor.kind, ResourceKind::Action);
        assert!(action.descriptor.annotations["factory.action"].contains("currentlyApplicable"));
        assert!(observed.resources.iter().all(|record| record
            .descriptor
            .annotations
            .contains_key("factory.owner-reading")
            || record.descriptor.kind == ResourceKind::Action));
    }

    #[test]
    fn rejects_non_factory_owner_before_installing_any_resource() {
        let error = read_factory_developmental(
            &OwnerCli {
                owner: "aikit",
                duplicate_journey: false,
                summary_frontier: "ship",
                run_missing_invocation: false,
                unrelated_continuation: false,
                unrelated_telemetry: false,
            },
            &binding(),
        )
        .unwrap_err();
        assert_eq!(error.code(), "factory.developmental_relation_mismatch");
    }

    #[test]
    fn rejects_duplicate_owner_identity_instead_of_deduplicating_conflicting_state() {
        let error = read_factory_developmental(
            &OwnerCli {
                owner: "factory",
                duplicate_journey: true,
                summary_frontier: "ship",
                run_missing_invocation: false,
                unrelated_continuation: false,
                unrelated_telemetry: false,
            },
            &binding(),
        )
        .unwrap_err();
        assert_eq!(error.code(), "factory.developmental_duplicate_ref");
    }

    #[test]
    fn rejects_a_late_unrelated_telemetry_reading_without_returning_partial_resources() {
        let error = read_factory_developmental(
            &OwnerCli {
                owner: "factory",
                duplicate_journey: false,
                summary_frontier: "ship",
                run_missing_invocation: false,
                unrelated_continuation: false,
                unrelated_telemetry: true,
            },
            &binding(),
        )
        .unwrap_err();
        assert_eq!(error.code(), "factory.developmental_relation_mismatch");
    }

    #[test]
    fn rejects_routine_continuation_evidence_for_another_invocation() {
        let error = read_factory_developmental(
            &OwnerCli {
                owner: "factory",
                duplicate_journey: false,
                summary_frontier: "ship",
                run_missing_invocation: false,
                unrelated_continuation: true,
                unrelated_telemetry: false,
            },
            &binding(),
        )
        .unwrap_err();
        assert_eq!(error.code(), "factory.developmental_relation_mismatch");
    }

    #[test]
    fn rejects_project_summary_and_journey_detail_drift() {
        let error = read_factory_developmental(
            &OwnerCli {
                owner: "factory",
                duplicate_journey: false,
                summary_frontier: "different frontier",
                run_missing_invocation: false,
                unrelated_continuation: false,
                unrelated_telemetry: false,
            },
            &binding(),
        )
        .unwrap_err();
        assert_eq!(error.code(), "factory.developmental_relation_mismatch");
    }

    #[test]
    fn rejects_a_routine_invocation_not_declared_by_both_journey_and_run() {
        let error = read_factory_developmental(
            &OwnerCli {
                owner: "factory",
                duplicate_journey: false,
                summary_frontier: "ship",
                run_missing_invocation: true,
                unrelated_continuation: false,
                unrelated_telemetry: false,
            },
            &binding(),
        )
        .unwrap_err();
        assert_eq!(error.code(), "factory.developmental_relation_mismatch");
    }
}
