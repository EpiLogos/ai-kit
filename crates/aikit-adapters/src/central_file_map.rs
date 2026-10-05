//! Query attachment to Central's persistent file map. Reads never rebuild it.
use crate::runner::CommandRunner;
use aikit_core::context_source::{ContextSourcePrivacy, RetrievalTarget};
use aikit_core::knowledge_source_pool::*;
use aikit_core::resource::{
    ProviderRef, ResourceLocator, ResourceSource, SourceRef, SourceRevision, SourceState,
};
use aikit_core::{AikitError, Result};
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub const SCHEMA: &str = "central.file-map/v1";
pub fn executable() -> PathBuf {
    std::env::var_os("CENTRAL_CTRL_BIN")
        .or_else(|| std::env::var_os("OI_CENTRAL_CTRL_BIN"))
        .map(PathBuf::from)
        .unwrap_or_else(|| "ctrl".into())
}
pub fn call<R: CommandRunner>(
    runner: &R,
    executable: &Path,
    root: &Path,
    operation: &str,
    input: &Value,
) -> Result<Value> {
    call_with_timeout(runner, executable, root, operation, input, None)
}

fn call_with_timeout<R: CommandRunner>(
    runner: &R,
    executable: &Path,
    root: &Path,
    operation: &str,
    input: &Value,
    timeout: Option<Duration>,
) -> Result<Value> {
    // This runner's argv contract is text. A lossy conversion could address
    // another real executable or World; refuse before any owner effect.
    let executable = executable.to_str().ok_or_else(|| {
        AikitError::new(
            "central.file_map_coordinate_invalid",
            "Native executable coordinate is not representable by the text transport",
        )
        .with("coordinate", "executable")
    })?;
    let root = root.to_str().ok_or_else(|| {
        AikitError::new(
            "central.file_map_coordinate_invalid",
            "Native World coordinate is not representable by the text transport",
        )
        .with("coordinate", "root")
    })?;
    let argv = vec![
        executable.to_owned(),
        "--json".into(),
        "--root".into(),
        root.to_owned(),
        "action".into(),
        "run".into(),
        format!("central.file-map.{operation}"),
        input.to_string(),
    ];
    let output = match timeout {
        Some(timeout) => runner.run_with_timeout(&argv, timeout),
        None => runner.run(&argv),
    }
    .map_err(|error| {
        private_transport_failure("central.file_map_unavailable", &error)
            .with("owner_operation", format!("central.file-map.{operation}"))
    })?;
    let observed = |error: AikitError| {
        error
            .with("owner_operation", format!("central.file-map.{operation}"))
            .with("execution_status", output.status.to_string())
            .with("stdout_text_bytes", output.stdout.len().to_string())
            .with("stderr_text_bytes", output.stderr.len().to_string())
            .with_native_capture(
                Some(output.status),
                output.stdout.as_bytes().to_vec(),
                output.stderr.as_bytes().to_vec(),
            )
    };
    let envelope: Value = serde_json::from_str(&output.stdout).map_err(|error| {
        observed(
            invalid("Invalid owner JSON response")
                .with_private_native_cause(&invalid(format!("Invalid owner response: {error}"))),
        )
    })?;
    if !output.ok() || envelope["ok"] != true {
        return Err(observed(
            AikitError::new(
                match envelope["error"]["code"].as_str() {
                    Some("central.file_map_not_found") => "central.file_map_not_found",
                    Some("central.file_map_denied") => "central.file_map_denied",
                    Some("central.file_map_conflict") => "central.file_map_conflict",
                    _ => "central.file_map_unavailable",
                },
                "Central map operation failed",
            )
            .with(
                "native_error_code",
                native_failure_code(&envelope["error"]["code"]),
            )
            .with_native_result(envelope.clone()),
        ));
    }
    let data = &envelope["data"];
    if data["schema"] != SCHEMA || data["operation"] != operation || !data["result"].is_object() {
        return Err(observed(
            invalid("Unsupported Central file-map envelope").with_native_result(envelope.clone()),
        ));
    }
    Ok(data["result"].clone())
}
fn invalid(message: impl Into<String>) -> AikitError {
    AikitError::new("central.file_map_invalid", message)
}

// These are declared diagnostic codes, not Source ownership or audience grants.
// Unknown owner strings remain in the exact private native envelope.
pub(crate) fn native_failure_code(value: &Value) -> &'static str {
    match value.as_str() {
        Some("central.file_map_not_found") => "central.file_map_not_found",
        Some("central.file_map_denied") => "central.file_map_denied",
        Some("central.file_map_conflict") => "central.file_map_conflict",
        Some("central.world_declaration_absent") => "central.world_declaration_absent",
        Some("central.world_ancestry_unavailable") => "central.world_ancestry_unavailable",
        Some("io_error") => "io_error",
        Some("invalid_input") => "invalid_input",
        _ => "unrecognized_native_failure",
    }
}

// Keep the actual runner error, IO and bounded native capture private. Public
// transport summaries retain only static codes and observed boolean lifecycle facts.
pub(crate) fn private_transport_failure(code: &'static str, cause: &AikitError) -> AikitError {
    let facts: BTreeMap<&str, &str> = ["execution_started", "direct_child_reaped"]
        .iter()
        .filter_map(|key| {
            cause.details().get(*key).and_then(|value| {
                matches!(value.as_str(), "true" | "false").then_some((*key, value.as_str()))
            })
        })
        .collect();
    let mut error = AikitError::new(code, "Native owner transport failed")
        .with(
            "transport_error",
            json!({
                "code": cause.code(), "message": "Native owner transport failed", "details": facts,
            })
            .to_string(),
        )
        .with_io_source_from(cause)
        .with_private_native_cause(cause);
    if let Some(capture) = cause.native_capture() {
        error = error
            .with("stdout_bytes", capture.stdout.len().to_string())
            .with("stderr_bytes", capture.stderr.len().to_string());
        if let Some(status) = capture.status {
            error = error.with("execution_status", status.to_string());
        }
    }
    error
}

fn private_reading_failure(cause: AikitError, binding: &Value, reading: &Value) -> AikitError {
    let original = invalid("Actual native binding before current payload qualification")
        .with_native_result(binding.clone())
        .with_private_native_cause(&cause);
    AikitError::new(cause.code(), "Native Source reading could not be qualified")
        .with_io_source_from(&cause)
        .with_private_native_cause(&original)
        .with_native_result(reading.clone())
}
fn string<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v[key]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| invalid(format!("Missing {key}")))
}
// Native participation is an owner verdict, never a SourceRef grammar.
// This validates only transient metadata; it does not mint a payload revision.
fn owned_binding(reading: &Value, requested_key: &str, requested: &str) -> Result<bool> {
    let failure = |message| invalid(message).with_native_result(reading.clone());
    if reading["binding_only"] != true {
        return Err(failure(
            "Owner did not return the selected binding-only contract",
        ));
    }
    match reading["ownership"].as_str() {
        Some("unregistered") => {
            if reading[requested_key] != requested
                || [
                    "source",
                    "world_ref",
                    "project",
                    "path",
                    "kind",
                    "revision",
                    "content",
                    "content_encoding",
                    "relation_revision",
                    "material_metadata_basis",
                ]
                .iter()
                .any(|key| reading.get(*key).is_some())
            {
                return Err(failure(
                    "Owner returned a contradictory unregistered Source disposition",
                ));
            }
            Ok(false)
        }
        Some("owned") => {
            let source = &reading["source"];
            let reference =
                string(source, "ref").map_err(|_| failure("Owner binding has no SourceRef"))?;
            SourceRef::parse(reference)
                .map_err(|_| failure("Owner binding has an invalid SourceRef"))?;
            if requested_key == "source_ref" && reference != requested {
                return Err(failure("Owner returned another SourceRef"));
            }
            for key in ["path", "provenance", "standing", "treatment"] {
                string(source, key)
                    .map_err(|_| failure("Owner returned an incomplete SourceBinding"))?;
            }
            if source["agent_retrieval_allowed"] != true
                || !source["roles"]
                    .as_array()
                    .is_some_and(|roles| roles.iter().all(Value::is_string))
                || !matches!(reading["kind"].as_str(), Some("file" | "directory"))
                || !(reading["project"].is_null()
                    || reading["project"]
                        .as_str()
                        .is_some_and(|value| !value.is_empty()))
                || reading.get("project").is_none()
                || ["revision", "content", "content_encoding"]
                    .iter()
                    .any(|key| reading.get(*key).is_some())
            {
                return Err(failure(
                    "Owner returned an unadmitted or unsupported Source descriptor",
                ));
            }
            for key in ["world_ref", "path", "relation_revision"] {
                string(reading, key)
                    .map_err(|_| failure("Owner returned incomplete binding metadata"))?;
            }
            if !Path::new(string(reading, "path")?).is_absolute() {
                return Err(failure(
                    "Owner returned a non-absolute admitted Source path",
                ));
            }
            let physical = &reading["material_metadata_basis"];
            if !["device", "inode", "byte_len"]
                .iter()
                .all(|key| physical[*key].as_u64().is_some())
                || !["mtime_seconds", "mtime_nanoseconds"]
                    .iter()
                    .all(|key| physical[*key].as_i64().is_some())
            {
                return Err(failure(
                    "Owner returned incomplete material metadata evidence",
                ));
            }
            Ok(true)
        }
        _ => Err(failure(
            "Owner returned no explicit Source ownership disposition",
        )),
    }
}

fn remaining_read(deadline: Instant) -> Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|remaining| !remaining.is_zero())
        .ok_or_else(|| {
            AikitError::new(
                "central.file_map_operation_timed_out",
                "The selected native Source read has no remaining operation budget",
            )
        })
}

fn provider() -> ProviderRef {
    ProviderRef::parse("provider/source-pool/central-bkmr").expect("static ref")
}
fn material(v: &Value, body: String) -> Result<SourceMaterial> {
    if v["source"]["agent_retrieval_allowed"] != true {
        return Err(AikitError::new(
            "central.file_map_denied",
            "Owner withheld source retrieval",
        ));
    }
    let source = SourceRef::parse(string(&v["source"], "ref")?)?;
    let revision = SourceRevision::parse(string(v, "revision")?)?;
    let origin = SourceOrigin {
        schema: SOURCE_ORIGIN_METADATA.into(),
        origin: SourceOriginKind::NativeSource {
            world_ref: string(v, "world_ref")?.into(),
            source: ResourceSource {
                source: source.clone(),
                revision: Some(revision.clone()),
                authority: None,
                locator: None,
                state: SourceState::Available,
            },
            observed_binding: NativeOriginBinding {
                roles: serde_json::from_value(v["source"]["roles"].clone())
                    .map_err(|error| invalid(error.to_string()))?,
                provenance: string(&v["source"], "provenance")?.into(),
                standing: string(&v["source"], "standing")?.into(),
                treatment: string(&v["source"], "treatment")?.into(),
                agent_retrieval_allowed: true,
            },
        },
    };
    Ok(SourceMaterial {
        binding: SourceBinding {
            source,
            revision,
            title: string(v, "title")?.into(),
            tags: serde_json::from_value(v["tags"].clone()).map_err(|e| invalid(e.to_string()))?,
            // This is owner-authorised material, not an actor-independent team grant.
            visibility: SourceVisibility::Personal,
            owners: vec![],
            media_type: if v["kind"] == "directory" {
                "inode/directory"
            } else if let Some(media_type) = v["media_type"].as_str() {
                media_type
            } else if std::path::Path::new(string(v, "path")?)
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "md" | "markdown"))
            {
                "text/markdown"
            } else {
                "text/plain"
            }
            .into(),
            locator: Some(ResourceLocator::Path(string(v, "path")?.into())),
            metadata: BTreeMap::from([
                (
                    SOURCE_ORIGIN_METADATA.into(),
                    origin.disclosure_projection()?,
                ),
                ("owner_read_required".into(), json!(true)),
            ]),
        },
        body,
    })
}
pub struct CentralFileMapProvider<R> {
    runner: R,
    executable: PathBuf,
    root: PathBuf,
    input: Value,
    material: Vec<SourceMaterial>,
    capabilities: SourceProviderCapabilities,
}
impl<R: CommandRunner> CentralFileMapProvider<R> {
    pub fn connect(
        runner: R,
        executable: impl Into<PathBuf>,
        root: impl Into<PathBuf>,
        project: Option<&str>,
    ) -> Result<Self> {
        let executable = executable.into();
        let root = root.into();
        // Capabilities only: the owner keeps the roster, and `read()`
        // resolves sources live. Materialising every pooled source here made
        // attachment cost scale with the whole world.
        let input = json!({"project":project,"federated":project.is_none(),"resources":false});
        let reading = call(&runner, &executable, &root, "inspect", &input)?;
        let native = &reading["provider"];
        let available = native["available"] == true;
        let capabilities = SourceProviderCapabilities {
            provider: provider(),
            version: native["version"].as_str().map(str::to_owned),
            fulltext: available && native["fulltext"] == true,
            fuzzy_interactive: false,
            semantic: false,
            hybrid: available && native["hybrid"] == true,
            tags: true,
            structured_output: true,
            reasons: BTreeMap::from([
                (
                    "owner".into(),
                    if available {
                        "Central persistent map: query attachment, no rebuild"
                    } else {
                        "Owner map unavailable or uninitialized; refresh explicitly through Central"
                    }
                    .into(),
                ),
                (
                    "semantic".into(),
                    "bkmr sem-search has no JSON contract; use prepared hybrid search".into(),
                ),
            ]),
        };
        let material = Vec::new();
        Ok(Self {
            runner,
            executable,
            root,
            input,
            material,
            capabilities,
        })
    }
    /// A view borrows the actual attached owner; it neither connects nor
    /// changes provider identity. Native membership/linkage remains decisive.
    pub fn for_project(&self, member: &str) -> Result<CentralFileMapView<'_, R>> {
        if member.is_empty()
            || Path::new(member).components().count() != 1
            || !matches!(
                Path::new(member).components().next(),
                Some(std::path::Component::Normal(_))
            )
        {
            return Err(invalid(
                "Native Project selection must be one actual Work member",
            ));
        }
        let mut input = self.input.clone();
        input["project"] = json!(member);
        input["federated"] = json!(false);
        Ok(CentralFileMapView { owner: self, input })
    }

    pub(crate) fn view(&self) -> CentralFileMapView<'_, R> {
        CentralFileMapView {
            owner: self,
            input: self.input.clone(),
        }
    }

    pub fn read_selected_path_for(
        &self,
        path: &Path,
        target: RetrievalTarget,
        timeout: Duration,
    ) -> Result<Option<SourcePoolReading>> {
        self.read_selected_path_with_input(path, target, timeout, &self.input)
    }

    fn read_selected_path_with_input(
        &self,
        path: &Path,
        target: RetrievalTarget,
        timeout: Duration,
        base: &Value,
    ) -> Result<Option<SourcePoolReading>> {
        let budget = self
            .runner
            .configured_timeout()
            .map_or(timeout, |limit| limit.min(timeout));
        let deadline = Instant::now()
            .checked_add(budget)
            .ok_or_else(|| invalid("Native selected-path budget is not representable"))?;
        let path = path
            .to_str()
            .ok_or_else(|| invalid("Native selected path is not representable"))?;
        let mut input = base.clone();
        input["path"] = json!(path);
        input["binding_only"] = json!(true);
        let selected = call_with_timeout(
            &self.runner,
            &self.executable,
            &self.root,
            "locate",
            &input,
            Some(remaining_read(deadline)?),
        )?;
        if !owned_binding(&selected, "requested_path", path)? {
            return Ok(None);
        }
        let source = SourceRef::parse(string(&selected["source"], "ref")?)?;
        let (reading, receipt) = self
            .read_current_receipt(&source, target, Some(remaining_read(deadline)?), base)?
            .ok_or_else(|| {
                AikitError::new(
                    "central.file_map_conflict",
                    "Selected native ownership changed before payload",
                )
            })?;
        if ["source", "world_ref", "project", "path", "kind"]
            .iter()
            .any(|key| receipt[*key] != selected[*key])
        {
            return Err(AikitError::new(
                "central.file_map_conflict",
                "Selected native path changed identity before payload",
            )
            .with_private_native_cause(
                &invalid("Selected native binding before payload")
                    .with_native_result(selected.clone()),
            )
            .with("native_reading_unconfirmed", "true")
            .with(
                "observed_payload_bytes",
                reading.material.body.len().to_string(),
            )
            .with_native_result(receipt));
        }
        remaining_read(deadline)?;
        Ok(Some(reading))
    }

    pub fn descriptors(&self) -> &[SourceMaterial] {
        &self.material
    }

    /// Operation-local native binding metadata retains kind, World, Project
    /// and Source member for the selected recipient. It is not a payload
    /// revision or authority; healthy unregistered remains the owner's verdict.
    pub(crate) fn source_binding_only_with_timeout(
        &self,
        source: &SourceRef,
        timeout: Duration,
    ) -> Result<Value> {
        self.source_binding_only_with_input(source, timeout, &self.input)
    }

    fn source_binding_only_with_input(
        &self,
        source: &SourceRef,
        timeout: Duration,
        base: &Value,
    ) -> Result<Value> {
        let mut input = base.clone();
        input["source_ref"] = json!(source.as_str());
        input["binding_only"] = json!(true);
        let reading = call_with_timeout(
            &self.runner,
            &self.executable,
            &self.root,
            "resolve",
            &input,
            Some(timeout),
        )?;
        owned_binding(&reading, "source_ref", source.as_str())?;
        Ok(reading)
    }

    pub(crate) fn locate_binding_only_with_timeout(
        &self,
        path: &Path,
        timeout: Duration,
    ) -> Result<Value> {
        self.locate_binding_only_with_input(path, timeout, &self.input)
    }

    fn locate_binding_only_with_input(
        &self,
        path: &Path,
        timeout: Duration,
        base: &Value,
    ) -> Result<Value> {
        let path = path.to_str().ok_or_else(|| {
            AikitError::new(
                "central.file_map_coordinate_invalid",
                "Native Source path is not representable by the owner text contract",
            )
            .with("coordinate", "path")
        })?;
        let mut input = base.clone();
        input["path"] = json!(path);
        input["binding_only"] = json!(true);
        let reading = call_with_timeout(
            &self.runner,
            &self.executable,
            &self.root,
            "locate",
            &input,
            Some(timeout),
        )?;
        owned_binding(&reading, "requested_path", path)?;
        Ok(reading)
    }

    pub(crate) fn configured_timeout(&self) -> Option<Duration> {
        self.runner.configured_timeout()
    }

    pub fn operation_timeout(&self) -> Option<Duration> {
        self.runner.configured_timeout()
    }

    /// Current allowed descriptor metadata only. Inspect's metadata revision
    /// is not a payload SourceRevision and is never forwarded as one.
    pub(crate) fn visit_source_roster(
        &self,
        timeout: Duration,
        visit: impl FnMut(&str, &Path) -> Result<()>,
    ) -> Result<()> {
        self.visit_source_roster_with_input(timeout, visit, &self.input)
    }

    fn visit_source_roster_with_input(
        &self,
        timeout: Duration,
        mut visit: impl FnMut(&str, &Path) -> Result<()>,
        base: &Value,
    ) -> Result<()> {
        let mut input = base.clone();
        input["resources"] = json!(true);
        let reading = call_with_timeout(
            &self.runner,
            &self.executable,
            &self.root,
            "inspect",
            &input,
            Some(timeout),
        )?;
        let resources = reading["resources"].as_array().ok_or_else(|| {
            invalid("Owner returned no Source descriptor roster")
                .with_native_result(reading.clone())
        })?;
        // The native capture/JSON has its own finite transport capacity. Do
        // not clone the whole World into another roster before a consumer
        // can charge its selected identities and paths.
        for entry in resources {
            if entry["source"]["agent_retrieval_allowed"] != true {
                return Err(invalid("Owner roster contains an unadmitted Source")
                    .with_native_result(reading.clone()));
            }
            let reference = string(&entry["source"], "ref")?;
            // Preserve the former roster's native identity validation without
            // retaining another collection of unselected World identities.
            let _ = SourceRef::parse(reference)
                .map_err(|error| private_reading_failure(error, &reading, &reading))?;
            visit(reference, Path::new(string(entry, "path")?))?;
        }
        Ok(())
    }

    pub(crate) fn read_for_with_timeout(
        &self,
        source: &SourceRef,
        target: RetrievalTarget,
        timeout: Duration,
    ) -> Result<Option<SourcePoolReading>> {
        self.read_current_for(source, target, Some(timeout))
    }

    fn read_current_for(
        &self,
        source: &SourceRef,
        target: RetrievalTarget,
        timeout: Option<Duration>,
    ) -> Result<Option<SourcePoolReading>> {
        Ok(self
            .read_current_receipt(source, target, timeout, &self.input)?
            .map(|(reading, _receipt)| reading))
    }

    fn read_current_receipt(
        &self,
        source: &SourceRef,
        target: RetrievalTarget,
        timeout: Option<Duration>,
        base: &Value,
    ) -> Result<Option<(SourcePoolReading, Value)>> {
        // Both native requests borrow one deadline. Unconfigured attachments
        // have a finite transport ceiling; explicit/configured smaller budgets win.
        let budget = match (timeout, self.runner.configured_timeout()) {
            (Some(requested), Some(configured)) => requested.min(configured),
            (Some(requested), None) => requested,
            (None, Some(configured)) => configured,
            (None, None) => Duration::from_secs(60),
        };
        let deadline = Instant::now()
            .checked_add(budget)
            .ok_or_else(|| invalid("Native Source read budget is not representable"))?;
        let mut input = base.clone();
        input["source_ref"] = json!(source);
        input["binding_only"] = json!(true);
        let binding = call_with_timeout(
            &self.runner,
            &self.executable,
            &self.root,
            "resolve",
            &input,
            Some(remaining_read(deadline)?),
        )?;
        let owned = owned_binding(&binding, "source_ref", source.as_str())?;
        remaining_read(deadline)
            .map_err(|error| private_reading_failure(error, &binding, &binding))?;
        if !owned {
            return Ok(None);
        }
        let privacy = ContextSourcePrivacy::default();
        SourcePoolReading::check_target(privacy, target)?;
        input
            .as_object_mut()
            .expect("attachment input is an object")
            .remove("binding_only");
        input["content"] = json!(true);
        let remaining = remaining_read(deadline)
            .map_err(|error| private_reading_failure(error, &binding, &binding))?;
        let reading = call_with_timeout(
            &self.runner,
            &self.executable,
            &self.root,
            "resolve",
            &input,
            Some(remaining),
        )?;
        remaining_read(deadline)
            .map_err(|error| private_reading_failure(error, &binding, &reading))?;
        // The latest payload remains its owner's reading. A changed identity,
        // scope or selected binding cannot be returned under earlier routing.
        if ["source", "world_ref", "project", "path", "kind"]
            .iter()
            .any(|key| reading[*key] != binding[*key])
        {
            return Err(private_reading_failure(
                AikitError::new(
                    "central.file_map_conflict",
                    "Native Source binding changed between metadata admission and payload read",
                ),
                &binding,
                &reading,
            ));
        }
        let body = reading["content"].as_str().ok_or_else(|| {
            private_reading_failure(
                invalid("Owner returned no text payload"),
                &binding,
                &reading,
            )
        })?;
        let material = material(&reading, body.into())
            .map_err(|error| private_reading_failure(error, &binding, &reading))?;
        remaining_read(deadline)
            .map_err(|error| private_reading_failure(error, &binding, &reading))?;
        Ok(Some((SourcePoolReading { material, privacy }, reading)))
    }

    fn search_with_input(
        &self,
        query: &str,
        mode: SourceSearchMode,
        tags: &[String],
        limit: usize,
        base: &Value,
    ) -> Result<Vec<SourceHit>> {
        if limit == 0 {
            return Ok(vec![]);
        }
        if mode == SourceSearchMode::Semantic {
            return Err(invalid("Pure semantic JSON is unsupported"));
        }
        let mut input = base.clone();
        input["query"] = json!(query);
        input["mode"] = json!(mode.as_str());
        input["tags"] = json!(tags);
        input["limit"] = json!(limit);
        let reading = call(&self.runner, &self.executable, &self.root, "search", &input)?;
        let hits = reading["hits"]
            .as_array()
            .ok_or_else(|| invalid("Missing search hits"))?;
        if hits.is_empty()
            && reading["absences"]
                .as_array()
                .is_some_and(|v| !v.is_empty())
        {
            return Err(AikitError::new(
                "central.file_map_unavailable",
                "Native Source search is unavailable",
            )
            .with(
                "absence_count",
                reading["absences"]
                    .as_array()
                    .map_or(0, Vec::len)
                    .to_string(),
            )
            .with_native_result(reading.clone()));
        }
        hits.iter()
            .map(|hit| {
                if hit["source"]["agent_retrieval_allowed"] != true {
                    return Err(invalid("Hit is not authorised by owner"));
                }
                Ok(SourceHit {
                    source: SourceRef::parse(string(&hit["source"], "ref")?)?,
                    revision: Some(SourceRevision::parse(string(hit, "revision")?)?),
                    provider: provider(),
                    score: hit["score"].as_f64(),
                    title: string(hit, "title")?.into(),
                    snippet: hit["snippet"].as_str().unwrap_or_default().into(),
                    tags: serde_json::from_value(hit["tags"].clone())
                        .map_err(|e| invalid(e.to_string()))?,
                    provider_binding: Some(format!(
                        "{}:{}",
                        string(hit, "world_ref")?,
                        string(hit, "provider_binding")?
                    )),
                    retrieval_mode: mode,
                })
            })
            .collect::<Result<Vec<_>>>()
            .map_err(|error| private_reading_failure(error, &reading, &reading))
    }
}
impl<R: CommandRunner> SourcePoolProvider for CentralFileMapProvider<R> {
    fn capabilities(&self) -> SourceProviderCapabilities {
        self.capabilities.clone()
    }
    fn rebuild(&mut self, _: &[SourceMaterial]) -> Result<()> {
        Err(AikitError::new(
            "central.map_owner_only",
            "Central owns persistent refresh; AIKit cannot rebuild it",
        ))
    }
    fn read(&self, source: &SourceRef) -> Result<Option<SourceMaterial>> {
        Ok(self
            .read_for(source, RetrievalTarget::LocalAgent)?
            .map(|reading| reading.material))
    }
    fn read_for(
        &self,
        source: &SourceRef,
        target: RetrievalTarget,
    ) -> Result<Option<SourcePoolReading>> {
        // Nonowners decline before the owning target predicate. Native errors
        // and denials stay on this exact owner route without replica fallback.
        self.read_current_for(source, target, None)
    }
    fn search(
        &self,
        query: &str,
        mode: SourceSearchMode,
        tags: &[String],
        limit: usize,
    ) -> Result<Vec<SourceHit>> {
        self.search_with_input(query, mode, tags, limit, &self.input)
    }
}

/// Operation-local selection on the same current native owner.
pub struct CentralFileMapView<'a, R> {
    owner: &'a CentralFileMapProvider<R>,
    input: Value,
}

impl<R: CommandRunner> CentralFileMapView<'_, R> {
    pub fn read_selected_path_for(
        &self,
        path: &Path,
        target: RetrievalTarget,
        timeout: Duration,
    ) -> Result<Option<SourcePoolReading>> {
        self.owner
            .read_selected_path_with_input(path, target, timeout, &self.input)
    }
    pub(crate) fn source_binding_only_with_timeout(
        &self,
        source: &SourceRef,
        timeout: Duration,
    ) -> Result<Value> {
        if self.input == self.owner.input {
            self.owner.source_binding_only_with_timeout(source, timeout)
        } else {
            self.owner
                .source_binding_only_with_input(source, timeout, &self.input)
        }
    }
    pub(crate) fn locate_binding_only_with_timeout(
        &self,
        path: &Path,
        timeout: Duration,
    ) -> Result<Value> {
        if self.input == self.owner.input {
            self.owner.locate_binding_only_with_timeout(path, timeout)
        } else {
            self.owner
                .locate_binding_only_with_input(path, timeout, &self.input)
        }
    }
    pub(crate) fn configured_timeout(&self) -> Option<Duration> {
        self.owner.configured_timeout()
    }
    pub(crate) fn visit_source_roster(
        &self,
        timeout: Duration,
        visit: impl FnMut(&str, &Path) -> Result<()>,
    ) -> Result<()> {
        if self.input == self.owner.input {
            self.owner.visit_source_roster(timeout, visit)
        } else {
            self.owner
                .visit_source_roster_with_input(timeout, visit, &self.input)
        }
    }
    pub(crate) fn read_for_with_timeout(
        &self,
        source: &SourceRef,
        target: RetrievalTarget,
        timeout: Duration,
    ) -> Result<Option<SourcePoolReading>> {
        if self.input == self.owner.input {
            self.owner.read_for_with_timeout(source, target, timeout)
        } else {
            Ok(self
                .owner
                .read_current_receipt(source, target, Some(timeout), &self.input)?
                .map(|(reading, _receipt)| reading))
        }
    }
}

impl<R: CommandRunner> SourcePoolProvider for CentralFileMapView<'_, R> {
    fn capabilities(&self) -> SourceProviderCapabilities {
        self.owner.capabilities()
    }
    fn rebuild(&mut self, _: &[SourceMaterial]) -> Result<()> {
        Err(AikitError::new(
            "central.map_owner_only",
            "Central owns persistent refresh; AIKit cannot rebuild it",
        ))
    }
    fn read(&self, source: &SourceRef) -> Result<Option<SourceMaterial>> {
        Ok(self
            .read_for(source, RetrievalTarget::LocalAgent)?
            .map(|reading| reading.material))
    }
    fn read_for(
        &self,
        source: &SourceRef,
        target: RetrievalTarget,
    ) -> Result<Option<SourcePoolReading>> {
        Ok(self
            .owner
            .read_current_receipt(source, target, None, &self.input)?
            .map(|(reading, _receipt)| reading))
    }
    fn search(
        &self,
        query: &str,
        mode: SourceSearchMode,
        tags: &[String],
        limit: usize,
    ) -> Result<Vec<SourceHit>> {
        self.owner
            .search_with_input(query, mode, tags, limit, &self.input)
    }
}
