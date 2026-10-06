//! Query attachment to Central's persistent file map. Reads never rebuild it.
use crate::runner::CommandRunner;
use aikit_core::context_source::{ContextSourcePrivacy, RetrievalTarget};
use aikit_core::knowledge_source_pool::*;
use aikit_core::resource::{
    ProviderRef, ResourceLocator, ResourceSource, SourceRef, SourceRevision, SourceState,
};
use aikit_core::{AikitError, Result};
use serde_json::{json, Value};
use std::time::Duration;
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
        AikitError::new("central.file_map_unavailable", error.message())
            .with("owner_operation", format!("central.file-map.{operation}"))
            .with(
                "transport_error",
                json!({"code":error.code(), "message":error.message(), "details":error.details()})
                    .to_string(),
            )
            .with_io_source_from(&error)
    })?;
    let envelope: Value = serde_json::from_str(&output.stdout).map_err(|error| {
        invalid(format!("Invalid owner response: {error}"))
            .with("owner_operation", format!("central.file-map.{operation}"))
            .with("execution_status", output.status.to_string())
            .with("stdout", &output.stdout)
            .with("stderr", &output.stderr)
    })?;
    if !output.ok() || envelope["ok"] != true {
        return Err(AikitError::new(
            match envelope["error"]["code"].as_str() {
                Some("central.file_map_not_found") => "central.file_map_not_found",
                Some("central.file_map_denied") => "central.file_map_denied",
                Some("central.file_map_conflict") => "central.file_map_conflict",
                _ => "central.file_map_unavailable",
            },
            envelope["error"]["message"]
                .as_str()
                .unwrap_or("Central map operation failed"),
        )
        .with("native_result", envelope.to_string())
        .with("native_error", envelope["error"].to_string())
        .with(
            "native_error_code",
            envelope["error"]["code"].as_str().unwrap_or_default(),
        )
        .with("owner_operation", format!("central.file-map.{operation}"))
        .with("execution_status", output.status.to_string())
        .with("stderr", &output.stderr));
    }
    let data = &envelope["data"];
    if data["schema"] != SCHEMA || data["operation"] != operation || !data["result"].is_object() {
        return Err(invalid("Unsupported Central file-map envelope")
            .with("native_result", envelope.to_string())
            .with("owner_operation", format!("central.file-map.{operation}"))
            .with("execution_status", output.status.to_string())
            .with("stderr", &output.stderr));
    }
    Ok(data["result"].clone())
}
fn invalid(message: impl Into<String>) -> AikitError {
    AikitError::new("central.file_map_invalid", message)
}
fn string<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v[key]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| invalid(format!("Missing {key}")))
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
    pub fn descriptors(&self) -> &[SourceMaterial] {
        &self.material
    }

    /// Metadata-only routing for an existing projection consumer. The owner
    /// supplies identity and location; the consumer still checks its aperture
    /// before requesting content through this same owner's read_for.
    pub(crate) fn locate_source(&self, path: &Path) -> Result<(SourceRef, PathBuf)> {
        self.locate_source_with_timeout(path, None)
    }

    pub(crate) fn locate_source_with_timeout(
        &self,
        path: &Path,
        timeout: Option<Duration>,
    ) -> Result<(SourceRef, PathBuf)> {
        let path = path.to_str().ok_or_else(|| {
            AikitError::new(
                "central.file_map_coordinate_invalid",
                "Native Source path is not representable by the owner text contract",
            )
            .with("coordinate", "path")
        })?;
        let mut input = self.input.clone();
        input["path"] = json!(path);
        input["content"] = json!(false);
        let reading = call_with_timeout(
            &self.runner,
            &self.executable,
            &self.root,
            "locate",
            &input,
            timeout,
        )?;
        if reading["source"]["agent_retrieval_allowed"] != true {
            return Err(AikitError::new(
                "central.file_map_denied",
                "Owner withheld source retrieval",
            )
            .with("native_reading", reading.to_string()));
        }
        Ok((
            SourceRef::parse(string(&reading["source"], "ref")?)?,
            PathBuf::from(string(&reading, "path")?),
        ))
    }

    pub(crate) fn source_path(&self, source: &SourceRef) -> Result<PathBuf> {
        let mut input = self.input.clone();
        input["source_ref"] = json!(source.as_str());
        input["content"] = json!(false);
        let reading = call(
            &self.runner,
            &self.executable,
            &self.root,
            "resolve",
            &input,
        )?;
        if reading["source"]["ref"] != source.as_str() {
            return Err(invalid("Owner returned another SourceRef")
                .with("native_reading", reading.to_string()));
        }
        if reading["source"]["agent_retrieval_allowed"] != true {
            return Err(AikitError::new(
                "central.file_map_denied",
                "Owner withheld source retrieval",
            )
            .with("native_reading", reading.to_string()));
        }
        Ok(PathBuf::from(string(&reading, "path")?))
    }

    pub(crate) fn configured_timeout(&self) -> Option<Duration> {
        self.runner.configured_timeout()
    }

    /// Current allowed descriptor metadata only. Inspect's metadata revision
    /// is not a payload SourceRevision and is never forwarded as one.
    pub(crate) fn visit_source_roster(
        &self,
        timeout: Duration,
        mut visit: impl FnMut(&str, &Path) -> Result<()>,
    ) -> Result<()> {
        let mut input = self.input.clone();
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
                .with("native_reading", reading.to_string())
        })?;
        // The native capture/JSON has its own finite transport capacity. Do
        // not clone the whole World into another roster before a consumer
        // can charge its selected identities and paths.
        for entry in resources {
            if entry["source"]["agent_retrieval_allowed"] != true {
                return Err(invalid("Owner roster contains an unadmitted Source")
                    .with("native_reading", reading.to_string()));
            }
            let reference = string(&entry["source"], "ref")?;
            // Preserve the former roster's native identity validation without
            // retaining another collection of unselected World identities.
            let _ = SourceRef::parse(reference)?;
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
        if !source.as_str().starts_with("central:source:") {
            return Ok(None);
        }
        let privacy = ContextSourcePrivacy::default();
        SourcePoolReading::check_target(privacy, target)?;
        let mut input = self.input.clone();
        input["source_ref"] = json!(source);
        input["content"] = json!(true);
        let reading = call_with_timeout(
            &self.runner,
            &self.executable,
            &self.root,
            "resolve",
            &input,
            timeout,
        )?;
        if reading["source"]["ref"] != source.as_str() {
            return Err(invalid("Owner returned another SourceRef")
                .with("native_reading", reading.to_string()));
        }
        let body = reading["content"].as_str().ok_or_else(|| {
            invalid("Owner returned no text payload").with("native_reading", reading.to_string())
        })?;
        Ok(Some(SourcePoolReading {
            material: material(&reading, body.into())?,
            privacy,
        }))
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
        if limit == 0 {
            return Ok(vec![]);
        }
        if mode == SourceSearchMode::Semantic {
            return Err(invalid("Pure semantic JSON is unsupported"));
        }
        let mut input = self.input.clone();
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
                reading["absences"].to_string(),
            ));
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
            .collect()
    }
}
