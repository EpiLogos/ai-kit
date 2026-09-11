//! Query attachment to Central's persistent file map. Reads never rebuild it.
use crate::runner::CommandRunner;
use aikit_core::knowledge_source_pool::*;
use aikit_core::resource::{ProviderRef, ResourceLocator, SourceRef, SourceRevision};
use aikit_core::{AikitError, Result};
use serde_json::{json, Value};
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
    let argv = vec![
        executable.to_string_lossy().into_owned(),
        "--json".into(),
        "--root".into(),
        root.to_string_lossy().into_owned(),
        "action".into(),
        "run".into(),
        format!("central.file-map.{operation}"),
        input.to_string(),
    ];
    let output = runner.run(&argv)?;
    let envelope: Value = serde_json::from_str(&output.stdout)
        .map_err(|e| invalid(format!("Invalid owner response: {e}")))?;
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
        ));
    }
    let data = &envelope["data"];
    if data["schema"] != SCHEMA || data["operation"] != operation || !data["result"].is_object() {
        return Err(invalid("Unsupported Central file-map envelope"));
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
    Ok(SourceMaterial {
        binding: SourceBinding {
            source: SourceRef::parse(string(&v["source"], "ref")?)?,
            revision: SourceRevision::parse(string(v, "revision")?)?,
            title: string(v, "title")?.into(),
            tags: serde_json::from_value(v["tags"].clone()).map_err(|e| invalid(e.to_string()))?,
            // This is owner-authorised material, not an actor-independent team grant.
            visibility: SourceVisibility::Personal,
            owners: vec![],
            media_type: if v["kind"] == "directory" {
                "inode/directory"
            } else {
                "text/plain"
            }
            .into(),
            locator: Some(ResourceLocator::Path(string(v, "path")?.into())),
            metadata: BTreeMap::from([
                ("central".into(), v.clone()),
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
        let input = json!({"project":project,"federated":project.is_none()});
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
        let material = reading["resources"]
            .as_array()
            .ok_or_else(|| invalid("Missing source roster"))?
            .iter()
            .map(|v| material(v, String::new()))
            .collect::<Result<Vec<_>>>()?;
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
        // A search can discover a newly registered source after attachment. The
        // owner resolves it live; a stale attachment roster is not an authority.
        let mut input = self.input.clone();
        input["source_ref"] = json!(source);
        input["content"] = json!(true);
        let reading = call(
            &self.runner,
            &self.executable,
            &self.root,
            "resolve",
            &input,
        )?;
        if reading["source"]["ref"] != source.as_str() {
            return Err(invalid("Owner returned another SourceRef"));
        }
        let body = reading["content"]
            .as_str()
            .ok_or_else(|| invalid("Owner returned no text payload"))?;
        Ok(Some(material(&reading, body.into())?))
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
