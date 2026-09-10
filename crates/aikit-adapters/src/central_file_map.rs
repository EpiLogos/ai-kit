//! Query-only Central file-map consumer. No database path, create or rebuild
//! operation is accepted here. Payload reads always return to the live owner.
use crate::runner::CommandRunner;
use aikit_core::knowledge_source_pool::*;
use aikit_core::resource::{ProviderRef, SourceRef, SourceRevision};
use aikit_core::{AikitError, Result};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
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
        format!("central.map.{operation}"),
        input.to_string(),
    ];
    let output = runner
        .run(&argv)?
        .require(&argv, "central.map_unavailable")?;
    let envelope: Value = serde_json::from_str(&output.stdout)
        .map_err(|e| AikitError::new("central.map_invalid", e.to_string()))?;
    if envelope["ok"] != true || envelope["data"]["schema"] != SCHEMA {
        return Err(AikitError::new(
            "central.map_invalid",
            "Central did not return its supported file-map contract",
        ));
    }
    Ok(envelope["data"].clone())
}
fn string<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v[key]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AikitError::new("central.map_invalid", format!("missing {key}")))
}
fn provider() -> ProviderRef {
    ProviderRef::parse("provider/source-pool/central-bkmr").expect("static provider ref")
}
fn material(v: &Value, body: String) -> Result<SourceMaterial> {
    if v["retrieval_allowed"] != true {
        return Err(AikitError::new(
            "central.map_denied",
            "Central withheld source retrieval",
        ));
    }
    let source = SourceRef::parse(string(v, "source_ref")?)?;
    let revision = SourceRevision::parse(string(v, "revision")?)?;
    let mut metadata = BTreeMap::new();
    metadata.insert("central".into(), v.clone());
    metadata.insert("owner_read_required".into(), json!(true));
    Ok(SourceMaterial {
        binding: SourceBinding {
            source,
            revision,
            title: v["options"]["title"]
                .as_str()
                .unwrap_or(string(v, "path")?)
                .into(),
            tags: v["options"]["tags"]
                .as_array()
                .map(|v| {
                    v.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
            visibility: SourceVisibility::Team,
            owners: vec![],
            media_type: if v["kind"] == "directory" {
                "inode/directory"
            } else {
                "text/plain"
            }
            .into(),
            locator: None,
            metadata,
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
    available: bool,
    hybrid: bool,
    version: Option<String>,
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
        let input = match project {
            Some(project) => json!({"scope":"project","project":project,"include_root":true}),
            None => json!({"scope":"root","federated":true}),
        };
        let reading = call(&runner, &executable, &root, "inspect", &input)?;
        let maps = reading["maps"]
            .as_array()
            .ok_or_else(|| AikitError::new("central.map_invalid", "map roster is absent"))?;
        let mut sources = Vec::new();
        let mut seen = BTreeSet::new();
        let mut available = false;
        let mut hybrid = !maps.is_empty();
        let mut version = None;
        for map in maps {
            available |= map["configured"] == true && map["version"].is_string();
            hybrid &= map["configured"] == true
                && map["version"].is_string()
                && map["embeddings_ready"] == true;
            version = version.or_else(|| map["version"].as_str().map(str::to_owned));
            for source in map["sources"]
                .as_array()
                .ok_or_else(|| AikitError::new("central.map_invalid", "source roster absent"))?
            {
                let source = material(source, String::new())?;
                if !seen.insert(source.binding.source.clone()) {
                    return Err(AikitError::new(
                        "central.map_duplicate",
                        "owner returned duplicate source refs",
                    ));
                }
                sources.push(source);
            }
        }
        Ok(Self {
            runner,
            executable,
            root,
            input,
            material: sources,
            available,
            hybrid,
            version,
        })
    }
    /// Metadata only; the body is deliberately not an eagerly copied owner file.
    pub fn descriptors(&self) -> &[SourceMaterial] {
        &self.material
    }
}
impl<R: CommandRunner> SourcePoolProvider for CentralFileMapProvider<R> {
    fn capabilities(&self) -> SourceProviderCapabilities {
        SourceProviderCapabilities {
            provider: provider(),
            version: self.version.clone(),
            fulltext: self.available,
            fuzzy_interactive: false,
            semantic: false,
            hybrid: self.hybrid,
            tags: true,
            structured_output: true,
            reasons: BTreeMap::from([
                (
                    "semantic".into(),
                    "semantic-only bkmr CLI has no structured contract".into(),
                ),
                (
                    "hybrid".into(),
                    "requires owner-prepared embeddings and an explicit owner capability".into(),
                ),
            ]),
        }
    }
    fn rebuild(&mut self, _: &[SourceMaterial]) -> Result<()> {
        Err(AikitError::new(
            "central.map_owner_only",
            "AIKit cannot rebuild a Central-owned map; use central.map.refresh",
        ))
    }
    fn read(&self, source: &SourceRef) -> Result<Option<SourceMaterial>> {
        if !self.material.iter().any(|m| &m.binding.source == source) {
            return Err(AikitError::new(
                "central.map_source_unknown",
                "source is not in the disclosed map",
            ));
        }
        let mut input = self.input.clone();
        input["source_ref"] = json!(source.as_str());
        input["content"] = json!(true);
        let reading = call(
            &self.runner,
            &self.executable,
            &self.root,
            "resolve",
            &input,
        )?;
        if reading["source"]["source_ref"] != source.as_str() {
            return Err(AikitError::new(
                "central.map_source_mismatch",
                "owner resolved a different source",
            ));
        }
        let body = reading["content"].as_str().ok_or_else(|| {
            AikitError::new(
                "central.map_nontext",
                "source is addressable but does not have a bounded UTF-8 payload",
            )
        })?;
        Ok(Some(material(&reading["source"], body.into())?))
    }
    fn search(
        &self,
        query: &str,
        mode: SourceSearchMode,
        tags: &[String],
        limit: usize,
    ) -> Result<Vec<SourceHit>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut input = self.input.clone();
        input["query"] = json!(query);
        input["mode"] = json!(mode.as_str());
        input["tags"] = json!(tags);
        input["limit"] = json!(limit);
        let reading = call(&self.runner, &self.executable, &self.root, "search", &input)?;
        if reading["absences"]
            .as_array()
            .is_some_and(|a| !a.is_empty())
            && reading["hits"].as_array().is_some_and(Vec::is_empty)
        {
            return Err(AikitError::new(
                "central.map_degraded",
                reading["absences"].to_string(),
            ));
        }
        let hits = reading["hits"]
            .as_array()
            .ok_or_else(|| AikitError::new("central.map_invalid", "search hits absent"))?;
        hits.iter()
            .map(|hit| {
                Ok(SourceHit {
                    source: SourceRef::parse(string(&hit["source"], "source_ref")?)?,
                    provider: provider(),
                    score: hit["score"].as_f64(),
                    title: string(hit, "title")?.into(),
                    snippet: hit["snippet"].as_str().unwrap_or("").into(),
                    tags: hit["tags"]
                        .as_array()
                        .map(|v| {
                            v.iter()
                                .filter_map(Value::as_str)
                                .map(str::to_owned)
                                .collect()
                        })
                        .unwrap_or_default(),
                    provider_binding: Some(format!(
                        "{}:{}",
                        string(&hit["source"], "world_ref")?,
                        hit["record_id"]
                    )),
                    retrieval_mode: mode,
                })
            })
            .collect()
    }
}
