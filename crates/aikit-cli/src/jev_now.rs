//! Operator-native Jev and prepared NOW context surface.
//!
//! This module deliberately composes existing owners rather than adding a new
//! source registry or workflow store: Central resolves source identity/content,
//! AIKit Knowledge supplies the current Wiki/query reading, Factory supplies
//! developmental relations, Jev selects only externally-egressible optional
//! candidates, and Redis retains the participant-specific hot projection.

use crate::app::Service;
use crate::cli::{
    JevInvokeArgs, JevValidateArgs, NowAppendChangeArgs, NowInspectArgs, NowPrepareArgs,
    NowPublishArgs, NowRevokeArgs, NowStatusArgs,
};
use aikit_adapters::central_file_map::{self, CentralFileMapProvider};
use aikit_adapters::jev::{
    CurlJevProvider, JevBoundary, JevCancellation, JevEndpoint, JevInvocation,
};
use aikit_adapters::runner::{CommandRunner, SystemRunner};
use aikit_adapters::secret_resolver::SuiteSecretResolver;
use aikit_core::context_source::{AgentVisibility, ExternalEgress};
use aikit_core::jev::{Answer, JevLimits, JevRequest, JevResponse, Question};
use aikit_core::knowledge_source_pool::SourcePoolProvider;
use aikit_core::secret_ref::{SecretRef, SecretResolver};
use aikit_core::{AikitError, ResourceRef, Result, SourceRef};
use aikit_store::now_context::{
    NowContextBasis, NowContextChange, NowContextItem, NowNeighbour, PreparedFactoryContext,
    PreparedFactoryUnit, PreparedNowContext, RedisNowConfig, RedisNowStore, NOW_PREPARED_SCHEMA,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const PREPARE_SCHEMA: &str = "aikit.now-preparation-request/v1";
const PREPARE_RESULT_SCHEMA: &str = "aikit.now-preparation-result/v1";
const MAX_SOURCE_BYTES: usize = 256 * 1024;
const MAX_CANDIDATES: usize = 64;
const MAX_WORKFLOW_UNITS: usize = 64;

pub(crate) fn fail(code: &'static str, message: impl Into<String>) -> AikitError {
    AikitError::new(code, message)
}

pub(crate) fn read_bytes(path: &Path, label: &str, max: usize) -> Result<Vec<u8>> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|e| fail("jev_now.file_unavailable", format!("{label}: {e}")))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() as usize > max {
        return Err(fail(
            "jev_now.file_invalid",
            format!("{label} must be a bounded regular non-symlink file"),
        ));
    }
    std::fs::read(path).map_err(|e| fail("jev_now.file_unavailable", format!("{label}: {e}")))
}

pub(crate) fn read_json<T: for<'de> Deserialize<'de>>(
    path: &Path,
    label: &str,
    max: usize,
) -> Result<T> {
    serde_json::from_slice(&read_bytes(path, label, max)?)
        .map_err(|e| fail("jev_now.invalid_json", format!("{label}: {e}")))
}

fn now_ms() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| fail("jev_now.clock", e.to_string()))?
        .as_millis()
        .min(u64::MAX as u128) as u64)
}

pub(crate) fn resolve_secret(
    config: &RedisNowConfig,
    allow_env_import: bool,
) -> Result<Option<aikit_core::SecretValue>> {
    config
        .credential_ref
        .as_ref()
        .map(|reference| {
            let resolver = if allow_env_import {
                SuiteSecretResolver::with_env_import()
            } else {
                SuiteSecretResolver::default()
            };
            resolver.resolve(reference)
        })
        .transpose()
}

fn minted_invocation_ref(request: &JevRequest) -> Result<ResourceRef> {
    let identity = format!("{}:{}:{}", request.digest()?, now_ms()?, std::process::id());
    ResourceRef::parse(format!(
        "invocation/jev/{}",
        blake3::hash(identity.as_bytes()).to_hex()
    ))
}

pub fn jev_validate(args: JevValidateArgs) -> Result<Value> {
    let request = JevRequest::parse(&read_bytes(&args.request_file, "Jev request", 1024 * 1024)?)?;
    let response = JevResponse::parse_for(
        &read_bytes(&args.response_file, "Jev response", 1024 * 1024)?,
        &request,
    )?;
    Ok(json!({
        "schema":"aikit.jev-validation/v1",
        "requestDigest": request.digest()?,
        "requestedModel": request.model,
        "returnedModel": response.model,
        "usage": response.usage,
        "answers": response.answers,
        "standing":"validated-provider-protocol-answer"
    }))
}

pub fn jev_invoke(args: JevInvokeArgs) -> Result<Value> {
    let request = JevRequest::parse(&read_bytes(&args.request_file, "Jev request", 1024 * 1024)?)?;
    let limits: JevLimits = read_json(&args.limits_file, "Jev limits", 256 * 1024)?;
    limits.validate(&request)?;
    let credential_ref = SecretRef::parse(&args.credential_ref)?;
    let resolver = if args.allow_env_import {
        SuiteSecretResolver::with_env_import()
    } else {
        SuiteSecretResolver::default()
    };
    let secret = resolver.resolve(&credential_ref)?;
    let initial_material_digest = blake3::hash(secret.expose().as_bytes());
    let invocation_ref = args
        .invocation_ref
        .as_deref()
        .map(ResourceRef::parse)
        .transpose()?
        .unwrap_or(minted_invocation_ref(&request)?);
    let endpoint = args
        .controlled_endpoint
        .map(JevEndpoint::Controlled)
        .unwrap_or(JevEndpoint::Official);
    let provider =
        CurlJevProvider::new(args.curl.unwrap_or_else(|| PathBuf::from("curl")), endpoint);
    let cancellation = JevCancellation::default();
    let mut guard = |_: JevBoundary| -> Result<()> {
        // Re-resolve at every provider boundary: a rotated/revoked native
        // credential ends this invocation rather than authorising a retry with
        // stale material. The secret itself is never persisted or logged.
        let current = resolver.resolve(&credential_ref)?;
        if blake3::hash(current.expose().as_bytes()) != initial_material_digest {
            return Err(fail(
                "jev.credential_changed",
                "The native credential changed during the invocation; recompose explicitly",
            ));
        }
        Ok(())
    };
    let receipt = provider.invoke(
        invocation_ref,
        &request,
        &limits,
        &secret,
        &cancellation,
        &mut guard,
    )?;
    serde_json::to_value(receipt)
        .map_err(|e| fail("jev_now.encode", format!("Jev invocation receipt: {e}")))
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CentralPrepare {
    root: PathBuf,
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    ctrl_bin: Option<PathBuf>,
    #[serde(default)]
    source_refs: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FactoryPrepare {
    state: PathBuf,
    run_ref: String,
    #[serde(default)]
    factory_bin: Option<PathBuf>,
    #[serde(default)]
    workflow_unit_refs: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MatrixPrepare {
    pub(crate) manifest: PathBuf,
    pub(crate) csv: PathBuf,
    #[serde(default)]
    pub(crate) view_id: Option<String>,
    #[serde(default)]
    pub(crate) capability_refs: Vec<String>,
    #[serde(default)]
    pub(crate) full_scope: bool,
    pub(crate) agent_visibility: AgentVisibility,
    pub(crate) external_egress: ExternalEgress,
}

/// One capability row, structured — the same fields the excerpt JSON below
/// carries, kept typed so a consumer (e.g. the contemplation field) never
/// re-parses the CSV or the excerpt string to get code/test relations.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MatrixCapabilityRow {
    pub(crate) id: String,
    pub(crate) need: String,
    pub(crate) operation: String,
    pub(crate) outcome: String,
    pub(crate) implementation_status: String,
    pub(crate) standing: String,
    pub(crate) source_refs: Vec<String>,
    pub(crate) code_refs: Vec<String>,
    pub(crate) test_refs: Vec<String>,
    pub(crate) account_ref: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MatrixEvidence {
    pub(crate) matrix_id: String,
    pub(crate) whole_account_ref: String,
    pub(crate) view_id: String,
    pub(crate) view_title: String,
    pub(crate) view_semantics: String,
    pub(crate) row_axis: Value,
    pub(crate) column_axis: Value,
    pub(crate) declared_capability_refs: Vec<String>,
    pub(crate) expanded_account_refs: Vec<String>,
    pub(crate) questions: Vec<String>,
    pub(crate) manifest_digest: String,
    pub(crate) csv_digest: String,
    /// Structured per-capability rows (additive: existing callers that only
    /// read the named fields above are unaffected). Carries code_refs/
    /// test_refs, which the excerpt string above also embeds textually.
    #[serde(default)]
    pub(crate) capability_rows: Vec<MatrixCapabilityRow>,
    /// Every relation record in the selected view, regardless of which
    /// capability it ties to — the grid relations the contemplation field
    /// needs, not just the ones touching the selection.
    #[serde(default)]
    pub(crate) grid_relations: Vec<Value>,
    /// Every relation record across all views (the prepared-context path
    /// stays view-bound through `grid_relations`; contemplation reads the
    /// whole matrix).
    pub(crate) all_view_relations: Vec<Value>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "mode", deny_unknown_fields)]
enum SelectionMode {
    All,
    Jev {
        credential_ref: String,
        limits: Box<JevLimits>,
        state: Value,
        #[serde(default = "default_threshold")]
        relevance_threshold: f64,
        #[serde(default)]
        allow_env_import: bool,
        #[serde(default)]
        curl: Option<PathBuf>,
        /// Deterministic protocol proof only. The transport enforces loopback
        /// and the fixed non-secret controlled marker; omitted means the
        /// official TypeSafe endpoint.
        #[serde(default)]
        controlled_endpoint: Option<SocketAddr>,
    },
}
fn default_threshold() -> f64 {
    0.5
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NowPrepareRequest {
    schema: String,
    redis: RedisNowConfig,
    project_ref: ResourceRef,
    now_ref: ResourceRef,
    participant_ref: ResourceRef,
    agent_session: ResourceRef,
    concern: String,
    disclosure_revision: String,
    #[serde(default)]
    practice_refs: Vec<ResourceRef>,
    central: CentralPrepare,
    #[serde(default)]
    factory: Option<FactoryPrepare>,
    #[serde(default)]
    matrix: Option<MatrixPrepare>,
    #[serde(default)]
    wiki_queries: Vec<String>,
    #[serde(default)]
    candidate_items: Vec<NowContextItem>,
    #[serde(default)]
    continuation: Option<String>,
    expected_version: u64,
    #[serde(default)]
    external_provider: bool,
    #[serde(default)]
    allow_redis_env_import: bool,
    selection: SelectionMode,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SelectionEvidence {
    mode: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    invocation: Option<JevInvocation>,
    #[serde(default)]
    selected_candidate_refs: Vec<ResourceRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    catalogue_sufficient_noul: Option<f64>,
    #[serde(default)]
    withheld_from_jev: Vec<ResourceRef>,
}

#[derive(Clone, Debug)]
struct FactoryEvidence {
    revision: Option<String>,
    run: Value,
    journeys: Vec<Value>,
    workflow_units: Vec<Value>,
    prepared: PreparedFactoryContext,
    neighbours: Vec<NowNeighbour>,
    dependency_revisions: BTreeMap<String, String>,
}

fn factory_text(value: &Value, field: &str) -> Result<String> {
    value[field]
        .as_str()
        .filter(|text| !text.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| {
            fail(
                "now_context.factory_invalid",
                format!("Factory reading omitted required field {field}"),
            )
        })
}

fn factory_texts(value: &Value, field: &str) -> Result<Vec<String>> {
    let values = value[field].as_array().ok_or_else(|| {
        fail(
            "now_context.factory_invalid",
            format!("Factory reading omitted required array {field}"),
        )
    })?;
    values
        .iter()
        .map(|item| {
            item.as_str()
                .filter(|text| !text.trim().is_empty())
                .map(str::to_owned)
                .ok_or_else(|| {
                    fail(
                        "now_context.factory_invalid",
                        format!("Factory {field} contains a non-string or empty value"),
                    )
                })
        })
        .collect()
}

fn factory_optional_texts(value: &Value, field: &str) -> Result<Vec<String>> {
    if value[field].is_null() {
        Ok(Vec::new())
    } else {
        factory_texts(value, field)
    }
}

fn prepared_factory_unit(unit: &Value) -> Result<PreparedFactoryUnit> {
    Ok(PreparedFactoryUnit {
        workflow_unit_ref: factory_text(unit, "workflowUnitRef")?,
        subject_ref: factory_text(unit, "subjectRef")?,
        basis_revision: factory_text(unit, "basisRevision")?,
        developmental_concern: factory_text(unit, "developmentalConcern")?,
        required_difference: factory_text(unit, "requiredDifference")?,
        required_return_contract: factory_text(&unit["requiredReturn"], "contract")?,
        required_return_address: factory_text(&unit["requiredReturn"], "address")?,
        required_verification: factory_texts(unit, "requiredVerification")?,
        agent_refs: factory_texts(&unit["agentRequirements"], "agentRefs")?,
        agent_set_refs: factory_texts(&unit["agentRequirements"], "agentSetRefs")?,
        agency_refs: factory_texts(&unit["agentRequirements"], "agencyRefs")?,
        praxis_refs: factory_texts(unit, "praxisRefs")?,
        capability_refs: factory_texts(unit, "capabilityRefs")?,
        dependencies: factory_texts(unit, "dependencies")?,
        independence_from: factory_texts(unit, "independenceFrom")?,
        permitted_effects: factory_texts(unit, "permittedEffects")?,
        stop_conditions: factory_text(unit, "stopConditions")?,
        escalation_conditions: factory_text(unit, "escalationConditions")?,
        current_agency_refs: factory_optional_texts(&unit["currentCorrelation"], "agencyRefs")?,
    })
}

pub(crate) fn file_digest(path: &Path, label: &str, max: usize) -> Result<(Vec<u8>, String)> {
    let bytes = read_bytes(path, label, max)?;
    let digest = format!("blake3:{}", blake3::hash(&bytes).to_hex());
    Ok((bytes, digest))
}

fn parse_csv_rows(input: &str) -> Result<Vec<Vec<String>>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut chars = input.chars().peekable();
    let mut quoted = false;
    while let Some(ch) = chars.next() {
        if quoted {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    quoted = false;
                }
            } else {
                field.push(ch);
            }
            continue;
        }
        match ch {
            '"' if field.is_empty() => quoted = true,
            ',' => {
                row.push(std::mem::take(&mut field));
            }
            '\n' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            '\r' if chars.peek() == Some(&'\n') => {}
            '\r' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            _ => field.push(ch),
        }
    }
    if quoted {
        return Err(fail(
            "now_context.matrix_invalid",
            "Capability matrix CSV ends inside a quoted field",
        ));
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    Ok(rows)
}

fn csv_header_index(headers: &[String], name: &str) -> Result<usize> {
    headers
        .iter()
        .position(|value| value == name)
        .ok_or_else(|| {
            fail(
                "now_context.matrix_invalid",
                format!("Capability matrix CSV is missing required column {name}"),
            )
        })
}

fn csv_field(row: &[String], index: usize) -> &str {
    row.get(index).map(String::as_str).unwrap_or("")
}

fn matrix_ref_list(raw: &str) -> Vec<String> {
    if raw.trim().is_empty() {
        return Vec::new();
    }
    serde_json::from_str::<Vec<String>>(raw).unwrap_or_else(|_| {
        raw.split(';')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect()
    })
}

fn matrix_axis(view: &Value, key: &str) -> Result<Value> {
    let axis = view.get(key).and_then(Value::as_object).ok_or_else(|| {
        fail(
            "now_context.matrix_invalid",
            format!("Capability matrix view is missing {key}"),
        )
    })?;
    if axis.get("id").and_then(Value::as_str).is_none()
        || axis.get("label").and_then(Value::as_str).is_none()
        || axis.get("members").and_then(Value::as_array).is_none()
    {
        return Err(fail(
            "now_context.matrix_invalid",
            format!("Capability matrix {key} has no id/label/members"),
        ));
    }
    Ok(Value::Object(axis.clone()))
}

pub(crate) fn read_matrix(config: &MatrixPrepare) -> Result<(Vec<NowContextItem>, MatrixEvidence)> {
    if config.full_scope == !config.capability_refs.is_empty() {
        return Err(fail(
            "now_context.matrix_scope",
            "Matrix scope must select exactly one of full_scope or capability_refs",
        ));
    }
    if config.capability_refs.len() > MAX_CANDIDATES {
        return Err(fail(
            "now_context.matrix_scope",
            "Selected matrix capability scope exceeds the bounded candidate limit",
        ));
    }

    let (manifest_bytes, manifest_digest) = file_digest(
        &config.manifest,
        "capability matrix manifest",
        2 * 1024 * 1024,
    )?;
    let manifest: Value = serde_json::from_slice(&manifest_bytes)
        .map_err(|e| fail("now_context.matrix_invalid", e.to_string()))?;
    if manifest["protocol"] != "ql-capability-matrix/1" {
        return Err(fail(
            "now_context.matrix_invalid",
            "Capability matrix manifest has an unsupported protocol",
        ));
    }
    let matrix_id = manifest["matrix_id"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            fail(
                "now_context.matrix_invalid",
                "Matrix manifest has no matrix_id",
            )
        })?
        .to_owned();
    let whole_account_ref = manifest["anchor_ref"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            fail(
                "now_context.matrix_invalid",
                "Matrix manifest has no anchor_ref",
            )
        })?
        .to_owned();
    let view_id = config
        .view_id
        .as_deref()
        .or_else(|| manifest["default_view"].as_str())
        .ok_or_else(|| {
            fail(
                "now_context.matrix_invalid",
                "Matrix manifest has no default_view",
            )
        })?
        .to_owned();
    let view = manifest["views"]
        .as_array()
        .and_then(|views| {
            views
                .iter()
                .find(|view| view["id"].as_str() == Some(view_id.as_str()))
        })
        .ok_or_else(|| {
            fail(
                "now_context.matrix_invalid",
                format!("Matrix manifest has no declared view {view_id}"),
            )
        })?;
    let view_title = view["title"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| fail("now_context.matrix_invalid", "Matrix view has no title"))?
        .to_owned();
    let view_semantics = view["semantics"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| fail("now_context.matrix_invalid", "Matrix view has no semantics"))?
        .to_owned();
    let row_axis = matrix_axis(view, "row_axis")?;
    let column_axis = matrix_axis(view, "column_axis")?;

    let (csv_bytes, csv_digest) =
        file_digest(&config.csv, "capability matrix CSV", 8 * 1024 * 1024)?;
    let csv_text = std::str::from_utf8(&csv_bytes)
        .map_err(|e| fail("now_context.matrix_invalid", e.to_string()))?;
    let rows = parse_csv_rows(csv_text)?;
    let headers = rows.first().ok_or_else(|| {
        fail(
            "now_context.matrix_invalid",
            "Capability matrix CSV is empty",
        )
    })?;
    let id_i = csv_header_index(headers, "id")?;
    let type_i = csv_header_index(headers, "record_type")?;
    let view_i = csv_header_index(headers, "view_id")?;
    let row_i = csv_header_index(headers, "row_id")?;
    let col_i = csv_header_index(headers, "column_id")?;
    let refs_i = csv_header_index(headers, "capability_refs")?;
    let need_i = csv_header_index(headers, "need")?;
    let operation_i = csv_header_index(headers, "operation")?;
    let outcome_i = csv_header_index(headers, "outcome")?;
    let status_i = csv_header_index(headers, "implementation_status")?;
    let standing_i = csv_header_index(headers, "standing")?;
    let sources_i = csv_header_index(headers, "source_refs")?;
    let code_refs_i = csv_header_index(headers, "code_refs")?;
    let test_refs_i = csv_header_index(headers, "test_refs")?;
    let account_i = csv_header_index(headers, "account_ref")?;
    let relation_i = csv_header_index(headers, "relation")?;
    let coverage_i = csv_header_index(headers, "coverage")?;
    let question_i = csv_header_index(headers, "question")?;

    let capability_rows = rows
        .iter()
        .skip(1)
        .filter(|row| csv_field(row, type_i) == "capability")
        .collect::<Vec<_>>();
    if capability_rows.is_empty() {
        return Err(fail(
            "now_context.matrix_invalid",
            "Capability matrix declares no capability records",
        ));
    }

    let selected_ids = if config.full_scope {
        capability_rows
            .iter()
            .map(|row| csv_field(row, id_i).to_owned())
            .collect::<Vec<_>>()
    } else {
        config.capability_refs.clone()
    };
    if selected_ids.is_empty() || selected_ids.len() > MAX_CANDIDATES {
        return Err(fail(
            "now_context.matrix_scope",
            "Selected matrix scope is empty or exceeds the bounded candidate limit",
        ));
    }
    let selected_set = selected_ids.iter().cloned().collect::<BTreeSet<_>>();
    if selected_set.len() != selected_ids.len() {
        return Err(fail(
            "now_context.matrix_scope",
            "Selected matrix scope repeats a capability identity",
        ));
    }

    let mut capabilities = BTreeMap::new();
    for row in &capability_rows {
        let id = csv_field(row, id_i);
        if id.trim().is_empty() {
            return Err(fail(
                "now_context.matrix_invalid",
                "Capability record has no stable id",
            ));
        }
        capabilities.insert(id.to_owned(), *row);
    }
    for id in &selected_ids {
        if !capabilities.contains_key(id) {
            return Err(fail(
                "now_context.matrix_scope",
                format!("Selected capability {id} is absent from the matrix inventory"),
            ));
        }
    }

    let relation_rows = rows
        .iter()
        .skip(1)
        .filter(|row| csv_field(row, type_i) != "capability" && csv_field(row, view_i) == view_id)
        .collect::<Vec<_>>();
    let mut account_refs = BTreeSet::new();
    let mut questions = BTreeSet::new();
    let mut items = Vec::new();
    let mut capability_rows = Vec::new();

    for id in &selected_ids {
        let row = capabilities[id];
        let account_ref = csv_field(row, account_i).trim().to_owned();
        if !account_ref.is_empty() {
            account_refs.insert(account_ref.clone());
        }
        let code_refs = matrix_ref_list(csv_field(row, code_refs_i));
        let test_refs = matrix_ref_list(csv_field(row, test_refs_i));
        let relations = relation_rows
            .iter()
            .filter(|relation| matrix_ref_list(csv_field(relation, refs_i)).contains(id))
            .map(|relation| {
                let question = csv_field(relation, question_i).trim().to_owned();
                if !question.is_empty() {
                    questions.insert(question.clone());
                }
                json!({
                    "row_id": csv_field(relation, row_i),
                    "column_id": csv_field(relation, col_i),
                    "relation": csv_field(relation, relation_i),
                    "coverage": csv_field(relation, coverage_i),
                    "question": question,
                })
            })
            .collect::<Vec<_>>();
        let excerpt = serde_json::to_string(&json!({
            "matrix_id": matrix_id,
            "whole_account_ref": whole_account_ref,
            "view": {
                "id": view_id,
                "title": view_title,
                "semantics": view_semantics,
                "row_axis": row_axis,
                "column_axis": column_axis,
            },
            "capability": {
                "id": id,
                "need": csv_field(row, need_i),
                "operation": csv_field(row, operation_i),
                "outcome": csv_field(row, outcome_i),
                "implementation_status": csv_field(row, status_i),
                "standing": csv_field(row, standing_i),
                "source_refs": matrix_ref_list(csv_field(row, sources_i)),
                "code_refs": code_refs,
                "test_refs": test_refs,
                "account_ref": account_ref,
            },
            "relations": relations,
        }))
        .map_err(|e| fail("now_context.matrix_invalid", e.to_string()))?;
        let identity = blake3::hash(format!("{matrix_id}:{id}").as_bytes()).to_hex();
        items.push(NowContextItem {
            source_ref: ResourceRef::parse(format!(
                "context-source/matrix/{}",
                &identity.as_str()[..24]
            ))?,
            source_revision: csv_digest.clone(),
            title: bounded_text(&format!("{id} — {}", csv_field(row, need_i)), 4096),
            excerpt: bounded_text(&excerpt, MAX_SOURCE_BYTES),
            route: Some(format!(
                "matrix:{matrix_id};view:{view_id};capability:{id};account:{account_ref}"
            )),
            agent_visibility: config.agent_visibility,
            external_egress: config.external_egress,
        });
        capability_rows.push(MatrixCapabilityRow {
            id: id.clone(),
            need: csv_field(row, need_i).to_owned(),
            operation: csv_field(row, operation_i).to_owned(),
            outcome: csv_field(row, outcome_i).to_owned(),
            implementation_status: csv_field(row, status_i).to_owned(),
            standing: csv_field(row, standing_i).to_owned(),
            source_refs: matrix_ref_list(csv_field(row, sources_i)),
            code_refs,
            test_refs,
            account_ref,
        });
    }

    let all_view_relations = rows
        .iter()
        .skip(1)
        .filter(|row| csv_field(row, type_i) != "capability")
        .map(|relation| {
            json!({
                "view_id": csv_field(relation, view_i),
                "row_id": csv_field(relation, row_i),
                "column_id": csv_field(relation, col_i),
                "relation": csv_field(relation, relation_i),
                "coverage": csv_field(relation, coverage_i),
                "question": csv_field(relation, question_i),
                "capability_refs": matrix_ref_list(csv_field(relation, refs_i)),
            })
        })
        .collect::<Vec<_>>();
    let grid_relations = relation_rows
        .iter()
        .map(|relation| {
            json!({
                "row_id": csv_field(relation, row_i),
                "column_id": csv_field(relation, col_i),
                "relation": csv_field(relation, relation_i),
                "coverage": csv_field(relation, coverage_i),
                "question": csv_field(relation, question_i),
                "capability_refs": matrix_ref_list(csv_field(relation, refs_i)),
            })
        })
        .collect::<Vec<_>>();

    Ok((
        items,
        MatrixEvidence {
            matrix_id,
            whole_account_ref,
            view_id,
            view_title,
            view_semantics,
            row_axis,
            column_axis,
            declared_capability_refs: selected_ids,
            expanded_account_refs: account_refs.into_iter().collect(),
            questions: questions.into_iter().collect(),
            manifest_digest,
            capability_rows,
            grid_relations,
            all_view_relations,
            csv_digest,
        },
    ))
}

pub(crate) fn revalidate_matrix(config: &MatrixPrepare, evidence: &MatrixEvidence) -> Result<()> {
    let (_, manifest_digest) = file_digest(
        &config.manifest,
        "capability matrix manifest",
        2 * 1024 * 1024,
    )?;
    let (_, csv_digest) = file_digest(&config.csv, "capability matrix CSV", 8 * 1024 * 1024)?;
    if manifest_digest != evidence.manifest_digest || csv_digest != evidence.csv_digest {
        return Err(fail(
            "now_context.matrix_stale",
            "Capability matrix manifest/CSV changed during context preparation",
        ));
    }
    Ok(())
}

pub(crate) fn bounded_text(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.to_owned();
    }
    let mut end = max;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

pub(crate) fn central_action(
    runner: &SystemRunner,
    ctrl: &Path,
    root: &Path,
    action: &str,
    input: &Value,
) -> Result<Value> {
    let argv = vec![
        ctrl.to_string_lossy().into_owned(),
        "--json".into(),
        "--root".into(),
        root.to_string_lossy().into_owned(),
        "action".into(),
        "run".into(),
        action.into(),
        input.to_string(),
    ];
    let output = runner.run(&argv)?;
    let envelope: Value = serde_json::from_str(&output.stdout)
        .map_err(|e| fail("now_context.central_invalid", format!("{action}: {e}")))?;
    if !output.ok() || envelope["ok"] != true {
        return Err(fail(
            "now_context.central_refused",
            envelope["error"]["message"]
                .as_str()
                .unwrap_or("Central owner operation failed"),
        ));
    }
    Ok(envelope["data"].clone())
}

fn factory_read(factory: &Path, args: &[String]) -> Result<Value> {
    let mut argv = vec![factory.to_string_lossy().into_owned(), "development".into()];
    argv.extend_from_slice(args);
    argv.push("--json".into());
    let output = SystemRunner::new()
        .with_timeout(Duration::from_secs(20))
        .run(&argv)?;
    if !output.ok() {
        return Err(fail(
            "now_context.factory_refused",
            if output.stderr.trim().is_empty() {
                output.stdout.trim()
            } else {
                output.stderr.trim()
            },
        ));
    }
    serde_json::from_str(&output.stdout)
        .map_err(|e| fail("now_context.factory_invalid", e.to_string()))
}

fn factory_revision_text(value: &Value, subject: &str) -> Result<String> {
    match value {
        Value::Number(number) if number.as_u64().is_some_and(|revision| revision > 0) => {
            Ok(number.to_string())
        }
        Value::String(revision) if !revision.trim().is_empty() => Ok(revision.clone()),
        _ => Err(fail(
            "now_context.factory_invalid",
            format!("{subject} omitted a valid revision"),
        )),
    }
}

fn factory_owner_basis(config: &FactoryPrepare) -> Result<(Value, Vec<Value>, String)> {
    let factory = config
        .factory_bin
        .clone()
        .unwrap_or_else(|| "factory".into());
    let state = config.state.to_string_lossy().into_owned();
    let run = factory_read(
        &factory,
        &["run".into(), state.clone(), config.run_ref.clone()],
    )?;
    let mut journeys = Vec::new();
    let mut basis = BTreeMap::new();
    let run_revision = factory_revision_text(&run["revision"], "Factory Run reading")?;
    basis.insert(format!("run:{}", config.run_ref), run_revision);
    for journey_ref in run["owningJourneyRefs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        let journey = factory_read(
            &factory,
            &["journey".into(), state.clone(), journey_ref.to_owned()],
        )?;
        let revision = factory_revision_text(&journey["revision"], "Factory Journey reading")?;
        basis.insert(format!("journey:{journey_ref}"), revision);
        journeys.push(journey);
    }
    let encoded = serde_json::to_vec(&basis)
        .map_err(|e| fail("now_context.factory_invalid", e.to_string()))?;
    let revision = format!("blake3:{}", blake3::hash(&encoded).to_hex());
    Ok((run, journeys, revision))
}

fn factory_evidence(config: &FactoryPrepare) -> Result<FactoryEvidence> {
    if config.workflow_unit_refs.len() > MAX_WORKFLOW_UNITS {
        return Err(fail(
            "now_context.factory_scope",
            "WorkflowUnit selection exceeds the bounded full-scope limit",
        ));
    }
    let factory = config
        .factory_bin
        .clone()
        .unwrap_or_else(|| "factory".into());
    let (run, journeys, owner_basis_revision) = factory_owner_basis(config)?;
    let revision = Some(owner_basis_revision.clone());
    let list = factory_read(
        &factory,
        &[
            "workflow-units".into(),
            config.state.to_string_lossy().into_owned(),
            config.run_ref.clone(),
        ],
    )?;
    let available = list["units"].as_array().cloned().unwrap_or_default();
    let selected: Vec<String> = if config.workflow_unit_refs.is_empty() {
        available
            .iter()
            .filter_map(|v| v["workflowUnitRef"].as_str().map(str::to_owned))
            .collect()
    } else {
        let present: BTreeSet<_> = available
            .iter()
            .filter_map(|v| v["workflowUnitRef"].as_str())
            .collect();
        if config
            .workflow_unit_refs
            .iter()
            .any(|r| !present.contains(r.as_str()))
        {
            return Err(fail(
                "now_context.factory_scope",
                "A selected WorkflowUnit is absent from the current Factory Run",
            ));
        }
        config.workflow_unit_refs.clone()
    };
    if selected.len() > MAX_WORKFLOW_UNITS {
        return Err(fail(
            "now_context.factory_scope",
            "Full declared WorkflowUnit scope exceeds 64 units; narrow explicitly",
        ));
    }
    let mut workflow_units = Vec::new();
    let mut dependency_revisions = BTreeMap::new();
    for unit_ref in selected {
        let unit = factory_read(
            &factory,
            &[
                "workflow-unit".into(),
                config.state.to_string_lossy().into_owned(),
                unit_ref.clone(),
                config.run_ref.clone(),
            ],
        )?;
        if let Some(basis) = unit["basisRevision"].as_str() {
            dependency_revisions.insert(unit_ref.clone(), basis.to_owned());
        }
        workflow_units.push(unit);
    }
    let prepared_units = workflow_units
        .iter()
        .map(prepared_factory_unit)
        .collect::<Result<Vec<_>>>()?;
    let journey_refs = run["owningJourneyRefs"]
        .as_array()
        .ok_or_else(|| {
            fail(
                "now_context.factory_invalid",
                "Factory Run reading omitted owningJourneyRefs",
            )
        })?
        .iter()
        .map(|value| {
            value
                .as_str()
                .filter(|text| !text.trim().is_empty())
                .map(str::to_owned)
                .ok_or_else(|| {
                    fail(
                        "now_context.factory_invalid",
                        "Factory owningJourneyRefs contains an invalid value",
                    )
                })
        })
        .collect::<Result<Vec<_>>>()?;
    let prepared = PreparedFactoryContext {
        run_ref: config.run_ref.clone(),
        owner_basis_revision,
        journey_refs,
        workflow_units: prepared_units,
    };

    let mut neighbours = Vec::new();
    if let Some(agencies) = run["agencies"].as_array() {
        for agency in agencies.iter().take(64) {
            let Some(agent) = agency["agentRef"].as_str() else {
                continue;
            };
            let Ok(participant_ref) = ResourceRef::parse(agent) else {
                continue;
            };
            if participant_ref == ResourceRef::parse(agent)? { /* validates once */ }
            let task_ref = ResourceRef::parse(config.run_ref.clone()).or_else(|_| {
                ResourceRef::parse(format!(
                    "factory-run/{}",
                    blake3::hash(config.run_ref.as_bytes()).to_hex()
                ))
            })?;
            let returned_refs = agency["returnRef"]
                .as_str()
                .and_then(|r| ResourceRef::parse(r).ok())
                .into_iter()
                .collect();
            neighbours.push(NowNeighbour {
                participant_ref,
                task_ref,
                relation: agency["position"]
                    .as_str()
                    .unwrap_or("factory-participant")
                    .to_owned(),
                dependency_revision: revision.clone(),
                write_scope: vec![],
                returned_refs,
            });
        }
    }
    Ok(FactoryEvidence {
        revision,
        run,
        journeys,
        workflow_units,
        prepared,
        neighbours,
        dependency_revisions,
    })
}

/// `central.now.read` input: a Project-scope NOW (`central:now:project:…`)
/// is read in that Project's scope, a root NOW in the root register.
fn now_read_input(now_ref: &str, project: Option<&str>) -> Value {
    match project {
        Some(project) if now_ref.starts_with("central:now:project:") => {
            json!({"now_ref": now_ref, "project": project})
        }
        _ => json!({"now_ref": now_ref}),
    }
}

pub(crate) fn extract_now_source_refs(data: &Value) -> Vec<String> {
    data["record"]["source_refs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

fn read_exact_sources(
    config: &CentralPrepare,
    now_ref: &ResourceRef,
) -> Result<(Value, Vec<NowContextItem>, BTreeMap<String, String>)> {
    let ctrl = config
        .ctrl_bin
        .clone()
        .unwrap_or_else(central_file_map::executable);
    let runner = SystemRunner::new().with_timeout(Duration::from_secs(20));
    let now = central_action(
        &runner,
        &ctrl,
        &config.root,
        "central.now.read",
        &now_read_input(now_ref.as_str(), config.project.as_deref()),
    )?;
    let mut refs: BTreeSet<String> = extract_now_source_refs(&now).into_iter().collect();
    refs.extend(config.source_refs.iter().cloned());
    let provider = CentralFileMapProvider::connect(
        SystemRunner::new().with_timeout(Duration::from_secs(20)),
        &ctrl,
        &config.root,
        config.project.as_deref(),
    )?;
    let mut items = Vec::new();
    let mut revisions = BTreeMap::new();
    for raw in refs {
        if items.len() >= 64 {
            return Err(fail(
                "now_context.source_scope",
                "Selected Central source inventory exceeds 64 items; narrow the undertaking",
            ));
        }
        let source = SourceRef::parse(&raw)?;
        let material = provider.read(&source)?.ok_or_else(|| {
            fail(
                "now_context.source_missing",
                format!("Central did not resolve {source}"),
            )
        })?;
        let resource = ResourceRef::parse(material.binding.source.as_str())?;
        let revision = material.binding.revision.as_str().to_owned();
        revisions.insert(resource.to_string(), revision.clone());
        let route = material
            .binding
            .locator
            .as_ref()
            .map(|locator| format!("{locator:?}"));
        items.push(NowContextItem {
            source_ref: resource,
            source_revision: revision,
            title: material.binding.title,
            excerpt: bounded_text(&material.body, MAX_SOURCE_BYTES),
            route,
            agent_visibility: AgentVisibility::Payload,
            // ProjectCentral's current authored ContextSource contract defaults
            // Central material to local-only egress. We preserve that fact here;
            // a future owner-authored allowed ContextSource can enter through
            // candidate_items with its actual privacy reading.
            external_egress: ExternalEgress::Denied,
        });
    }
    Ok((now, items, revisions))
}

fn revalidate_sources(config: &CentralPrepare, expected: &BTreeMap<String, String>) -> Result<()> {
    let ctrl = config
        .ctrl_bin
        .clone()
        .unwrap_or_else(central_file_map::executable);
    let runner = SystemRunner::new().with_timeout(Duration::from_secs(20));
    for (source_ref, revision) in expected {
        let mut input =
            json!({"source_ref":source_ref,"expected_revision":revision,"content":false});
        if let Some(project) = &config.project {
            input["project"] = json!(project);
        }
        let _ = central_action(
            &runner,
            &ctrl,
            &config.root,
            "central.file-map.resolve",
            &input,
        )?;
    }
    Ok(())
}

fn revalidate_factory(
    config: Option<&FactoryPrepare>,
    expected_revision: Option<&str>,
) -> Result<()> {
    let (Some(config), Some(expected)) = (config, expected_revision) else {
        return Ok(());
    };
    let (_, _, current) = factory_owner_basis(config)?;
    if current != expected {
        return Err(fail(
            "now_context.factory_stale",
            "Factory Run/Journey basis changed during context preparation",
        ));
    }
    Ok(())
}

fn selection_request(
    state: Value,
    candidates: &[NowContextItem],
    model: String,
) -> Result<JevRequest> {
    let mut questions = BTreeMap::new();
    for (index, item) in candidates.iter().enumerate() {
        if item.external_egress != ExternalEgress::Allowed {
            continue;
        }
        questions.insert(
            format!("candidate/{index:03}"),
            Question::Noul {
                instructions: json!({
                    "question":"Does this candidate materially contribute to the undertaking described by the shared state?",
                    "source_ref":item.source_ref,
                    "source_revision":item.source_revision,
                    "title":item.title,
                    "excerpt":item.excerpt,
                    "law":"Several complementary candidates may all be relevant; do not force a winner."
                }),
                criteria: None,
            },
        );
    }
    questions.insert(
        "catalogue-sufficient".into(),
        Question::Noul {
            instructions: json!({"question":"Does the supplied catalogue/state sufficiently represent the need, without inventing a missing capability?","false_is_legitimate":true}),
            criteria: None,
        },
    );
    JevRequest {
        model,
        state,
        questions,
    }
    .tap_validate()
}

trait ValidateRequestExt {
    fn tap_validate(self) -> Result<Self>
    where
        Self: Sized;
}
impl ValidateRequestExt for JevRequest {
    fn tap_validate(self) -> Result<Self> {
        self.validate()?;
        Ok(self)
    }
}

fn select_candidates(
    selection: &SelectionMode,
    candidates: &[NowContextItem],
    revalidate: &mut dyn FnMut() -> Result<()>,
) -> Result<(Vec<NowContextItem>, SelectionEvidence)> {
    match selection {
        SelectionMode::All => Ok((
            candidates.to_vec(),
            SelectionEvidence {
                mode: "all",
                invocation: None,
                selected_candidate_refs: candidates.iter().map(|i| i.source_ref.clone()).collect(),
                catalogue_sufficient_noul: None,
                withheld_from_jev: vec![],
            },
        )),
        SelectionMode::Jev {
            credential_ref,
            limits,
            state,
            relevance_threshold,
            allow_env_import,
            curl,
            controlled_endpoint,
        } => {
            if !relevance_threshold.is_finite() || !(0.0..=1.0).contains(relevance_threshold) {
                return Err(fail(
                    "now_context.selection_invalid",
                    "Jev relevance threshold must be between zero and one",
                ));
            }
            let eligible: Vec<_> = candidates
                .iter()
                .filter(|i| {
                    i.external_egress == ExternalEgress::Allowed
                        && i.agent_visibility == AgentVisibility::Payload
                })
                .cloned()
                .collect();
            let withheld_from_jev = candidates
                .iter()
                .filter(|i| !eligible.iter().any(|e| e.source_ref == i.source_ref))
                .map(|i| i.source_ref.clone())
                .collect::<Vec<_>>();
            let request = selection_request(
                state.clone(),
                &eligible,
                limits.tariff.model_version.clone(),
            )?;
            limits.validate(&request)?;
            let secret_ref = SecretRef::parse(credential_ref)?;
            let resolver = if *allow_env_import {
                SuiteSecretResolver::with_env_import()
            } else {
                SuiteSecretResolver::default()
            };
            let secret = resolver.resolve(&secret_ref)?;
            let initial_material_digest = blake3::hash(secret.expose().as_bytes());
            let endpoint = controlled_endpoint
                .map(JevEndpoint::Controlled)
                .unwrap_or(JevEndpoint::Official);
            let provider = CurlJevProvider::new(
                curl.clone().unwrap_or_else(|| PathBuf::from("curl")),
                endpoint,
            );
            let cancellation = JevCancellation::default();
            let invocation_ref = minted_invocation_ref(&request)?;
            let mut guard = |_: JevBoundary| -> Result<()> {
                revalidate()?;
                let current = resolver.resolve(&secret_ref)?;
                if blake3::hash(current.expose().as_bytes()) != initial_material_digest {
                    return Err(fail(
                        "jev.credential_changed",
                        "Jev credential changed during preparation",
                    ));
                }
                Ok(())
            };
            let invocation = provider.invoke(
                invocation_ref,
                &request,
                limits,
                &secret,
                &cancellation,
                &mut guard,
            )?;
            let answer = invocation.answer.as_ref().ok_or_else(|| {
                fail(
                    "now_context.jev_failed",
                    invocation
                        .failure
                        .as_ref()
                        .map(|f| f.message.as_str())
                        .unwrap_or("Jev produced no successful determination"),
                )
            })?;
            let mut selected = Vec::new();
            let mut selected_refs = Vec::new();
            for (index, item) in eligible.iter().enumerate() {
                let id = format!("candidate/{index:03}");
                if matches!(answer.answers.get(&id), Some(Answer::Noul{noul}) if *noul >= *relevance_threshold)
                {
                    selected.push(item.clone());
                    selected_refs.push(item.source_ref.clone());
                }
            }
            let catalogue_sufficient_noul = match answer.answers.get("catalogue-sufficient") {
                Some(Answer::Noul { noul }) => Some(*noul),
                _ => None,
            };
            Ok((
                selected,
                SelectionEvidence {
                    mode: "jev",
                    invocation: Some(invocation),
                    selected_candidate_refs: selected_refs,
                    catalogue_sufficient_noul,
                    withheld_from_jev,
                },
            ))
        }
    }
}

pub fn now_status(args: NowStatusArgs) -> Result<Value> {
    let config: RedisNowConfig = read_json(&args.config_file, "Redis NOW config", 256 * 1024)?;
    let secret = resolve_secret(&config, args.allow_env_import)?;
    let status = RedisNowStore::new(config)?.status(secret.as_ref())?;
    serde_json::to_value(status).map_err(|e| fail("jev_now.encode", e.to_string()))
}

pub fn now_inspect(args: NowInspectArgs) -> Result<Value> {
    let config: RedisNowConfig = read_json(&args.config_file, "Redis NOW config", 256 * 1024)?;
    let participant = ResourceRef::parse(&args.participant_ref)?;
    let secret = resolve_secret(&config, args.allow_env_import)?;
    let store = RedisNowStore::new(config)?;
    Ok(json!({
        "schema":"aikit.now-context-inspection/v1",
        "participantRef":participant,
        "currentVersion":store.current_version(&participant, secret.as_ref())?,
        "ackCursor":store.ack_cursor(&participant, secret.as_ref())?,
        "lastDelivery":store.last_delivery(&participant, secret.as_ref())?,
        "prepared":store.read_prepared(&participant, args.external_provider, secret.as_ref())?,
    }))
}

pub fn now_publish(args: NowPublishArgs) -> Result<Value> {
    let view: PreparedNowContext = read_json(&args.view_file, "Prepared NOW context", 1024 * 1024)?;
    let config: RedisNowConfig = read_json(&args.config_file, "Redis NOW config", 256 * 1024)?;
    let secret = resolve_secret(&config, args.allow_env_import)?;
    let store = RedisNowStore::new(config)?;
    let version = store.publish(&view, args.expected_version, secret.as_ref())?;
    Ok(
        json!({"schema":"aikit.now-context-publish/v1","participantRef":view.participant_ref,"version":version,"preparedDigest":view.digest()?,"basisDigest":view.basis_digest}),
    )
}

pub fn now_append_change(args: NowAppendChangeArgs) -> Result<Value> {
    let change: NowContextChange = read_json(&args.change_file, "NOW change", 256 * 1024)?;
    let config: RedisNowConfig = read_json(&args.config_file, "Redis NOW config", 256 * 1024)?;
    let participant = ResourceRef::parse(&args.participant_ref)?;
    let secret = resolve_secret(&config, args.allow_env_import)?;
    let cursor =
        RedisNowStore::new(config)?.append_change(&participant, &change, secret.as_ref())?;
    Ok(
        json!({"schema":"aikit.now-context-change-appended/v1","participantRef":participant,"cursor":cursor,"changeId":change.change_id}),
    )
}

pub fn now_revoke(args: NowRevokeArgs) -> Result<Value> {
    let config: RedisNowConfig = read_json(&args.config_file, "Redis NOW config", 256 * 1024)?;
    let participant = ResourceRef::parse(&args.participant_ref)?;
    let secret = resolve_secret(&config, args.allow_env_import)?;
    RedisNowStore::new(config)?.revoke(&participant, &args.disclosure_revision, secret.as_ref())?;
    Ok(
        json!({"schema":"aikit.now-context-revocation/v1","participantRef":participant,"disclosureRevision":args.disclosure_revision,"revoked":true}),
    )
}

pub fn now_prepare(cwd: &Path, args: NowPrepareArgs) -> Result<Value> {
    let request: NowPrepareRequest =
        read_json(&args.request_file, "NOW preparation request", 1024 * 1024)?;
    now_prepare_request(cwd, request)
}

/// Configured encounter entry uses the same native preparation path as the CLI.
/// It may refresh the volatile delivery/session/version basis, but never changes
/// the prepared participant, Project, NOW, disclosure or owner-source selection.
pub(super) fn prepare_for_encounter(
    cwd: &Path,
    request_file: &Path,
    redis: &RedisNowConfig,
    participant: &ResourceRef,
    session: &ResourceRef,
    external_provider: bool,
) -> Result<Value> {
    let mut request: NowPrepareRequest = read_json(
        request_file,
        "NOW encounter preparation request",
        1024 * 1024,
    )?;
    if request.redis != *redis
        || request.participant_ref != *participant
        || request.external_provider != external_provider
    {
        return Err(fail(
            "now_context.encounter_prepare_mismatch",
            "Configured encounter preparation does not match this Redis/participant/provider boundary",
        ));
    }
    let secret = resolve_secret(&request.redis, request.allow_redis_env_import)?;
    request.expected_version =
        RedisNowStore::new(request.redis.clone())?.current_version(participant, secret.as_ref())?;
    request.agent_session = session.clone();
    now_prepare_request(cwd, request)
}

fn now_prepare_request(cwd: &Path, request: NowPrepareRequest) -> Result<Value> {
    if request.schema != PREPARE_SCHEMA
        || request.concern.trim().is_empty()
        || request.candidate_items.len() > MAX_CANDIDATES
        || request
            .matrix
            .as_ref()
            .is_some_and(|matrix| matrix.capability_refs.len() > MAX_CANDIDATES)
        || request.wiki_queries.len() > 32
    {
        return Err(fail(
            "now_context.prepare_invalid",
            "NOW preparation request is malformed or outside bounded scope",
        ));
    }
    request.redis.validate()?;
    let secret = resolve_secret(&request.redis, request.allow_redis_env_import)?;
    let store = RedisNowStore::new(request.redis.clone())?;
    let current = store.current_version(&request.participant_ref, secret.as_ref())?;
    if current != request.expected_version {
        return Err(fail(
            "now_context.stale",
            format!(
                "Prepared version basis moved from {} to {current}",
                request.expected_version
            ),
        ));
    }

    let (central_now, mut exact_items, source_revisions) =
        read_exact_sources(&request.central, &request.now_ref)?;
    // The Central NOW record is an owner read, not a mere cache key. Its own
    // source revision participates in the semantic basis when supplied.
    let mut all_source_revisions = source_revisions;
    if let (Some(source), Some(revision)) = (
        central_now["source"]["ref"].as_str(),
        central_now["revision"]["revision"].as_str(),
    ) {
        all_source_revisions.insert(source.to_owned(), revision.to_owned());
    }

    let factory = request.factory.as_ref().map(factory_evidence).transpose()?;
    let factory_revision = factory.as_ref().and_then(|f| f.revision.clone());
    let dependency_revisions = factory
        .as_ref()
        .map(|f| f.dependency_revisions.clone())
        .unwrap_or_default();

    let matrix = request.matrix.as_ref().map(read_matrix).transpose()?;
    let (matrix_candidates, matrix_evidence) = match matrix {
        Some((items, evidence)) => (items, Some(evidence)),
        None => (Vec::new(), None),
    };
    let mut selection_candidates = matrix_candidates;
    selection_candidates.extend(request.candidate_items.clone());
    if selection_candidates.len() > MAX_CANDIDATES {
        return Err(fail(
            "now_context.matrix_scope",
            "Combined matrix and explicit candidate scope exceeds the bounded candidate limit",
        ));
    }
    if let Some(evidence) = &matrix_evidence {
        all_source_revisions.insert(
            format!("matrix:{}:manifest", evidence.matrix_id),
            evidence.manifest_digest.clone(),
        );
        all_source_revisions.insert(
            format!("matrix:{}:csv", evidence.matrix_id),
            evidence.csv_digest.clone(),
        );
    }

    let mut knowledge_frames = Vec::new();
    if !request.wiki_queries.is_empty() {
        let mut service = Service::discover(cwd)?;
        for query in &request.wiki_queries {
            let found = service.knowledge_search(query, 32)?;
            let addresses = found
                .hits
                .iter()
                .map(|hit| hit.address.clone())
                .collect::<Vec<_>>();
            let mut frame = service.knowledge_frame(Some(query), &addresses)?;
            if request.external_provider {
                // The Knowledge application has selected useful owner-backed
                // routes, but its generic reading does not carry ContextSource
                // external-egress policy. Preserve relationships/revisions and
                // evidence while withholding payload. Egress-approved passages
                // enter through NowContextItem, which has the explicit privacy
                // contract and is revalidated below.
                for reading in &mut frame.readings {
                    reading.content = None;
                }
                frame.explanations.clear();
                frame.contradictions.clear();
                frame.open_questions.clear();
            }
            knowledge_frames.push(frame);
        }
    }

    let mut revalidate = || -> Result<()> {
        revalidate_sources(
            &request.central,
            &all_source_revisions
                .iter()
                .filter(|(k, _)| k.starts_with("central:source:"))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        )?;
        revalidate_factory(request.factory.as_ref(), factory_revision.as_deref())?;
        if let (Some(config), Some(evidence)) = (request.matrix.as_ref(), matrix_evidence.as_ref())
        {
            revalidate_matrix(config, evidence)?;
        }
        Ok(())
    };
    revalidate()?;
    let (selected_candidates, selection) =
        select_candidates(&request.selection, &selection_candidates, &mut revalidate)?;
    revalidate()?;

    exact_items.extend(selected_candidates);
    // Stable de-duplication preserves the exact source version already selected.
    let mut seen = BTreeSet::new();
    exact_items.retain(|item| seen.insert((item.source_ref.clone(), item.source_revision.clone())));
    if exact_items.len() > 64 {
        return Err(fail(
            "now_context.source_scope",
            "Prepared source set exceeds 64 items",
        ));
    }

    let ack = store.ack_cursor(&request.participant_ref, secret.as_ref())?;
    let last_delivery = store.last_delivery(&request.participant_ref, secret.as_ref())?;
    let change_cursor = ack.max(last_delivery.as_ref().map(|r| r.change_cursor).unwrap_or(0));
    let basis = NowContextBasis {
        source_revisions: all_source_revisions.clone(),
        dependency_revisions,
        disclosure_revision: request.disclosure_revision,
        factory_revision: factory_revision.clone(),
        change_cursor,
    };
    let jev_invocation_ref = selection
        .invocation
        .as_ref()
        .map(|i| i.invocation_ref.clone());
    let view = PreparedNowContext {
        schema: NOW_PREPARED_SCHEMA.into(),
        project_ref: request.project_ref,
        now_ref: request.now_ref,
        participant_ref: request.participant_ref,
        agent_session: request.agent_session,
        version: request.expected_version.checked_add(1).ok_or_else(|| {
            fail(
                "now_context.version_exhausted",
                "Prepared version exhausted",
            )
        })?,
        basis_digest: basis.digest()?,
        basis,
        concern: request.concern,
        practice_refs: request.practice_refs,
        items: exact_items,
        neighbours: factory
            .as_ref()
            .map(|f| f.neighbours.clone())
            .unwrap_or_default(),
        factory: factory.as_ref().map(|f| f.prepared.clone()),
        knowledge_frames,
        continuation: request.continuation,
        jev_invocation_ref,
        prepared_at_unix_ms: now_ms()?,
    };
    view.validate(request.external_provider)?;
    // Final owner-source check immediately precedes the CAS publish. If a
    // source or Factory revision moved while Jev was thinking, this refuses;
    // the late determination never replaces a newer view.
    revalidate()?;
    let published = store.publish(&view, request.expected_version, secret.as_ref())?;
    Ok(json!({
        "schema":PREPARE_RESULT_SCHEMA,
        "publishedVersion":published,
        "preparedDigest":view.digest()?,
        "basisDigest":view.basis_digest,
        "participantRef":view.participant_ref,
        "agentSession":view.agent_session,
        "sourceCount":view.items.len(),
        "neighbourCount":view.neighbours.len(),
        "knowledge":view.knowledge_frames,
        "factory":factory.as_ref().map(|f|json!({"run":f.run,"journeys":f.journeys,"workflowUnits":f.workflow_units})),
        "matrix":matrix_evidence,
        "selection":selection,
        "standing":"prepared and atomically published against revalidated native source/Factory basis"
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_core::context_source::{AgentVisibility, ExternalEgress};
    use std::io::Write as _;

    fn candidate(name: &str, egress: ExternalEgress) -> NowContextItem {
        NowContextItem {
            source_ref: ResourceRef::parse(format!("context-source/{name}")).unwrap(),
            source_revision: "r1".into(),
            title: name.into(),
            excerpt: format!("{name} body"),
            route: None,
            agent_visibility: AgentVisibility::Payload,
            external_egress: egress,
        }
    }

    #[test]
    fn factory_owner_revisions_accept_native_numeric_revision_values() {
        assert_eq!(
            factory_revision_text(&json!(7), "Factory Run reading").unwrap(),
            "7"
        );
        assert_eq!(
            factory_revision_text(&json!("opaque-r7"), "Factory Run reading").unwrap(),
            "opaque-r7"
        );
        assert_eq!(
            factory_revision_text(&Value::Null, "Factory Run reading")
                .unwrap_err()
                .code(),
            "now_context.factory_invalid"
        );
    }

    #[test]
    fn selection_request_never_sends_egress_denied_candidate_payloads() {
        let allowed = candidate("allowed", ExternalEgress::Allowed);
        let denied = candidate("denied", ExternalEgress::Denied);
        let request = selection_request(
            json!({"public":"state"}),
            &[allowed, denied],
            "jev-1.2.3".into(),
        )
        .unwrap();
        assert!(request.questions.contains_key("candidate/000"));
        assert!(!request.questions.contains_key("candidate/001"));
        let encoded = serde_json::to_string(&request).unwrap();
        assert!(encoded.contains("allowed body"));
        assert!(!encoded.contains("denied body"));
    }

    fn matrix_fixture() -> (tempfile::TempDir, MatrixPrepare) {
        let temp = tempfile::tempdir().unwrap();
        let manifest = temp.path().join("matrix.json");
        let csv = temp.path().join("matrix.csv");
        std::fs::write(
            &manifest,
            serde_json::to_vec_pretty(&json!({
                "protocol":"ql-capability-matrix/1",
                "matrix_id":"matrix.test",
                "anchor_ref":"test:account:whole",
                "default_view":"product-field",
                "views":[{
                    "id":"product-field",
                    "title":"Test seed × field",
                    "semantics":"Which declared capability contributes here?",
                    "row_axis":{"id":"seed","label":"Seed","members":[{"id":"q0","label":"Why?","source_ref":"test:seed:q0"}]},
                    "column_axis":{"id":"field","label":"Field","members":[{"id":"S2","label":"AIKit"}]}
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            &csv,
            concat!(
                "id,record_type,view_id,row_id,column_id,capability_refs,need,operation,outcome,implementation_status,standing,source_refs,code_refs,test_refs,account_ref,relation,coverage,extensions,question\n",
                "cap.one,capability,,,,[],Need one,operate one,outcome one,implemented,implementation-fact,source/one,,,account.html#q1,,,,\n",
                "cap.two,capability,,,,[],Need two,operate two,outcome two,intended,design-commitment,source/two,,,account.html#q2,,,,\n",
                "rel.one,relation,product-field,q0,S2,\"[\"\"cap.one\"\"]\",,,,,,,,,,contributes,covered,{},Does cap one contribute?\n"
            ),
        )
        .unwrap();
        let config = MatrixPrepare {
            manifest,
            csv,
            view_id: None,
            capability_refs: Vec::new(),
            full_scope: true,
            agent_visibility: AgentVisibility::Payload,
            external_egress: ExternalEgress::Allowed,
        };
        (temp, config)
    }

    #[test]
    fn declared_matrix_full_scope_preserves_axes_need_operation_outcome_and_account_routes() {
        let (_temp, config) = matrix_fixture();
        let (items, evidence) = read_matrix(&config).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(evidence.declared_capability_refs, ["cap.one", "cap.two"]);
        assert_eq!(evidence.whole_account_ref, "test:account:whole");
        assert_eq!(evidence.view_id, "product-field");
        assert_eq!(evidence.row_axis["members"][0]["id"], "q0");
        assert_eq!(evidence.column_axis["members"][0]["id"], "S2");
        assert!(evidence
            .questions
            .iter()
            .any(|q| q == "Does cap one contribute?"));
        assert!(evidence
            .expanded_account_refs
            .iter()
            .any(|r| r == "account.html#q1"));
        assert!(items[0].excerpt.contains("\"need\":\"Need one\""));
        assert!(items[0].excerpt.contains("\"operation\":\"operate one\""));
        assert!(items[0].excerpt.contains("\"outcome\":\"outcome one\""));
        assert!(items[0]
            .excerpt
            .contains("\"implementation_status\":\"implemented\""));
        assert!(items[0]
            .excerpt
            .contains("\"standing\":\"implementation-fact\""));
        assert!(items[0].excerpt.contains("Does cap one contribute?"));
    }

    #[test]
    fn matrix_scope_rejects_both_or_neither_selection_forms() {
        let (_temp, mut config) = matrix_fixture();
        config.capability_refs = vec!["cap.one".into()];
        assert_eq!(
            read_matrix(&config).unwrap_err().code(),
            "now_context.matrix_scope"
        );

        config.full_scope = false;
        config.capability_refs.clear();
        assert_eq!(
            read_matrix(&config).unwrap_err().code(),
            "now_context.matrix_scope"
        );
    }

    #[test]
    fn matrix_scope_is_exact_and_semantic_change_invalidates_the_preparation_basis() {
        let (_temp, mut config) = matrix_fixture();
        config.full_scope = false;
        config.capability_refs = vec!["cap.two".into()];
        let (items, evidence) = read_matrix(&config).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(evidence.declared_capability_refs, ["cap.two"]);
        std::fs::OpenOptions::new()
            .append(true)
            .open(&config.csv)
            .unwrap()
            .write_all(b"\n")
            .unwrap();
        let error = revalidate_matrix(&config, &evidence).unwrap_err();
        assert_eq!(error.code(), "now_context.matrix_stale");
    }
    #[test]
    fn a_project_scope_now_is_read_in_its_project() {
        assert_eq!(
            now_read_input("central:now:project:O-I:abc", Some("O-I")),
            json!({"now_ref": "central:now:project:O-I:abc", "project": "O-I"})
        );
        assert_eq!(
            now_read_input("central:now:control:root:abc", Some("O-I")),
            json!({"now_ref": "central:now:control:root:abc"})
        );
        assert_eq!(
            now_read_input("central:now:project:O-I:abc", None),
            json!({"now_ref": "central:now:project:O-I:abc"})
        );
    }
}
