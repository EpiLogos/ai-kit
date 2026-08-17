//! Real bkmr SourcePool adapter.
//!
//! This is a CLI adapter over upstream bkmr, not a reimplementation of its
//! retrieval algorithms. Canonical [`SourceRef`](aikit_core::resource::SourceRef)
//! identity is carried through bkmr's description field and recovered from every
//! hit; bkmr row/document IDs remain operational provider bindings only.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use aikit_core::knowledge_source_pool::{
    SourceBinding, SourceHit, SourceMaterial, SourcePoolProvider, SourceProviderCapabilities,
    SourceProviderStatus, SourceSearchMode, BKMR_GLADE_CONFORMANCE_VERSION,
};
use aikit_core::resource::{ProviderRef, SourceRef};
use aikit_core::{AikitError, Result};
use serde_json::{Map, Value};

use crate::runner::CommandRunner;

const REF_PREFIX: &str = "aikit-source-ref:";

#[derive(Debug, Clone, PartialEq, Eq)]
struct BkmrCliSurface {
    available: bool,
    version: Option<String>,
    fulltext_json: bool,
    fuzzy_interactive: bool,
    semantic_cli: bool,
    hybrid_json: bool,
    tags: bool,
    reason: Option<String>,
}

/// Provider-neutral SourcePool implementation backed by the real upstream bkmr
/// command line interface.
///
/// Capability discovery is performed once at construction. This deliberately
/// separates "the CLI advertises semantic/hybrid" from "this provider database
/// was materialised with embeddings": semantic/hybrid are exposed only when both
/// are true.
pub struct BkmrSourcePoolProvider<R> {
    runner: R,
    binary: String,
    db_path: PathBuf,
    enable_embeddings: bool,
    provider: ProviderRef,
    cli: BkmrCliSurface,
    bindings: BTreeMap<String, SourceBinding>,
    rebuilt: bool,
}

impl<R: CommandRunner> BkmrSourcePoolProvider<R> {
    pub fn new(runner: R, db_path: impl AsRef<Path>, enable_embeddings: bool) -> Self {
        Self::with_binary(runner, "bkmr", db_path, enable_embeddings)
    }

    pub fn with_binary(
        runner: R,
        binary: impl Into<String>,
        db_path: impl AsRef<Path>,
        enable_embeddings: bool,
    ) -> Self {
        let binary = binary.into();
        let cli = discover_cli(&runner, &binary);
        Self {
            runner,
            binary,
            db_path: db_path.as_ref().to_path_buf(),
            enable_embeddings,
            provider: ProviderRef::parse("provider/source-pool/bkmr")
                .expect("static bkmr provider ref must be valid"),
            cli,
            bindings: BTreeMap::new(),
            rebuilt: false,
        }
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    fn run(&self, args: &[String], include_db: bool, code: &'static str) -> Result<String> {
        let argv = self.argv(args, include_db);
        self.runner
            .run(&argv)?
            .require(&argv, code)
            .map(|output| output.stdout)
    }

    fn argv(&self, args: &[String], include_db: bool) -> Vec<String> {
        let mut argv = vec![self.binary.clone()];
        if include_db {
            argv.push("--db".into());
            argv.push(self.db_path.display().to_string());
        }
        argv.extend(args.iter().cloned());
        argv
    }

    fn hit_from_record(
        &self,
        record: &Map<String, Value>,
        mode: SourceSearchMode,
        rank: usize,
    ) -> Option<SourceHit> {
        let bookmark = record
            .get("bookmark")
            .and_then(Value::as_object)
            .unwrap_or(record);
        let description = bookmark
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let source = marker_ref(description)?;
        let binding = self.bindings.get(source.as_str())?;
        let tags = provider_tags(bookmark.get("tags")).unwrap_or_else(|| binding.tags.clone());
        let score = ["rrf_score", "score", "similarity", "semantic_score"]
            .iter()
            .find_map(|key| {
                record
                    .get(*key)
                    .or_else(|| bookmark.get(*key))
                    .and_then(Value::as_f64)
            })
            .or_else(|| Some(1.0 / (rank as f64 + 1.0)));
        let snippet = ["url", "content", "description"]
            .iter()
            .find_map(|key| bookmark.get(*key).and_then(Value::as_str))
            .unwrap_or_default()
            .chars()
            .take(1000)
            .collect();
        let provider_binding = bookmark.get("id").map(value_string);

        Some(SourceHit {
            source,
            provider: self.provider.clone(),
            score,
            title: bookmark
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or(&binding.title)
                .to_string(),
            snippet,
            tags,
            provider_binding,
            retrieval_mode: mode,
        })
    }

    fn hits_from_json(
        &self,
        stdout: &str,
        mode: SourceSearchMode,
        tags: &[String],
        limit: usize,
    ) -> Result<Vec<SourceHit>> {
        let records = json_records(stdout)?;
        let required = tags.iter().map(String::as_str).collect::<BTreeSet<_>>();
        Ok(records
            .iter()
            .enumerate()
            .filter_map(|(rank, record)| self.hit_from_record(record, mode, rank))
            .filter(|hit| {
                let actual = hit.tags.iter().map(String::as_str).collect::<BTreeSet<_>>();
                required.is_subset(&actual)
            })
            .take(limit)
            .collect())
    }
}

impl<R: CommandRunner> SourcePoolProvider for BkmrSourcePoolProvider<R> {
    fn capabilities(&self) -> SourceProviderCapabilities {
        let semantic = self.cli.semantic_cli && self.enable_embeddings;
        let hybrid = self.cli.hybrid_json && self.enable_embeddings;
        let mut reasons = BTreeMap::new();
        if !self.cli.available {
            reasons.insert(
                "provider".into(),
                self.cli
                    .reason
                    .clone()
                    .unwrap_or_else(|| "bkmr executable is unavailable".into()),
            );
        }
        if self.cli.semantic_cli && !self.enable_embeddings {
            reasons.insert(
                "semantic".into(),
                "bkmr supports sem-search, but this provider database is configured without embeddings"
                    .into(),
            );
        }
        if self.cli.hybrid_json && !self.enable_embeddings {
            reasons.insert(
                "hybrid".into(),
                "bkmr supports hsearch, but this provider database is configured without embeddings"
                    .into(),
            );
        }
        SourceProviderCapabilities {
            provider: self.provider.clone(),
            version: self.cli.version.clone(),
            fulltext: self.cli.available && self.cli.fulltext_json,
            fuzzy_interactive: self.cli.available && self.cli.fuzzy_interactive,
            semantic: self.cli.available && semantic,
            hybrid: self.cli.available && hybrid,
            tags: self.cli.available && self.cli.tags,
            structured_output: self.cli.available && self.cli.fulltext_json && self.cli.hybrid_json,
            reasons,
        }
    }

    fn rebuild(&mut self, material: &[SourceMaterial]) -> Result<()> {
        if !self.cli.available {
            return Err(AikitError::new(
                "knowledge.bkmr_unavailable",
                self.cli
                    .reason
                    .clone()
                    .unwrap_or_else(|| "bkmr executable is unavailable".into()),
            ));
        }

        let mut refs = BTreeSet::new();
        for item in material {
            if !refs.insert(item.binding.source.clone()) {
                return Err(AikitError::new(
                    "knowledge.source_pool_duplicate_ref",
                    "bkmr materialisation received duplicate stable SourceRefs",
                )
                .with("source", item.binding.source.to_string()));
            }
        }

        if let Some(parent) = self.db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                AikitError::new(
                    "knowledge.bkmr_db_prepare_failed",
                    format!("could not create bkmr database directory: {error}"),
                )
            })?;
        }
        for suffix in ["", "-wal", "-shm"] {
            let candidate = PathBuf::from(format!("{}{}", self.db_path.display(), suffix));
            if candidate.exists() {
                std::fs::remove_file(&candidate).map_err(|error| {
                    AikitError::new(
                        "knowledge.bkmr_db_prepare_failed",
                        format!("could not remove stale bkmr database state: {error}"),
                    )
                    .with("path", candidate.display().to_string())
                })?;
            }
        }

        self.run(
            &["create-db".into(), self.db_path.display().to_string()],
            false,
            "knowledge.bkmr_create_failed",
        )?;
        self.bindings = material
            .iter()
            .map(|item| (item.binding.source.to_string(), item.binding.clone()))
            .collect();

        for item in material {
            let mut args = vec![
                "add".into(),
                item.body.clone(),
                "--title".into(),
                item.binding.title.clone(),
                "--description".into(),
                format!("{REF_PREFIX}{}", item.binding.source),
                "--type".into(),
                "text".into(),
                "--no-web".into(),
            ];
            if !self.enable_embeddings {
                args.push("--no-embed".into());
            }
            if !item.binding.tags.is_empty() {
                args.push(item.binding.tags.join(","));
            }
            self.run(&args, true, "knowledge.bkmr_add_failed")?;
        }
        self.rebuilt = true;
        Ok(())
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
        let caps = self.capabilities();
        if !caps.supports(mode) {
            return Err(AikitError::new(
                "knowledge.source_provider_capability",
                format!(
                    "bkmr provider does not currently support {}: {}",
                    mode.as_str(),
                    caps.reasons
                        .get(mode.as_str())
                        .map(String::as_str)
                        .unwrap_or("capability absent from running CLI")
                ),
            ));
        }
        if !self.rebuilt {
            return Err(AikitError::new(
                "knowledge.bkmr_not_built",
                "bkmr SourcePool provider has not been rebuilt",
            ));
        }

        match mode {
            SourceSearchMode::Fulltext => {
                let mut args = vec!["search".into(), query.into()];
                if !tags.is_empty() {
                    args.extend(["--tags".into(), tags.join(",")]);
                }
                args.extend(["--json".into(), "--np".into(), "--no-color".into()]);
                let stdout = self.run(&args, true, "knowledge.bkmr_search_failed")?;
                self.hits_from_json(&stdout, mode, tags, limit)
            }
            SourceSearchMode::Hybrid => {
                let mut args = vec!["hsearch".into(), query.into()];
                if !tags.is_empty() {
                    args.extend(["--tags".into(), tags.join(",")]);
                }
                args.extend([
                    "--limit".into(),
                    limit.saturating_mul(3).max(limit).to_string(),
                    "--json".into(),
                    "--np".into(),
                ]);
                let stdout = self.run(&args, true, "knowledge.bkmr_hsearch_failed")?;
                self.hits_from_json(&stdout, mode, tags, limit)
            }
            SourceSearchMode::Semantic => {
                let stdout = self.run(
                    &["sem-search".into(), query.into(), "--np".into()],
                    true,
                    "knowledge.bkmr_sem_search_failed",
                )?;
                let mut ids = Vec::new();
                for line in stdout.lines() {
                    if let Some(id) = line.split_whitespace().next().filter(|raw| {
                        !raw.is_empty() && raw.chars().all(|ch| ch.is_ascii_digit())
                    }) {
                        if !ids.iter().any(|seen| seen == id) {
                            ids.push(id.to_string());
                        }
                    }
                }
                let mut hits = Vec::new();
                for (rank, id) in ids.iter().enumerate() {
                    let shown = self.run(
                        &["show".into(), id.clone(), "--json".into()],
                        true,
                        "knowledge.bkmr_show_failed",
                    )?;
                    let records = json_records(&shown)?;
                    for record in records {
                        if let Some(hit) = self.hit_from_record(&record, mode, rank) {
                            let required = tags.iter().map(String::as_str).collect::<BTreeSet<_>>();
                            let actual = hit
                                .tags
                                .iter()
                                .map(String::as_str)
                                .collect::<BTreeSet<_>>();
                            if required.is_subset(&actual) {
                                hits.push(hit);
                            }
                        }
                    }
                    if hits.len() >= limit {
                        break;
                    }
                }
                if hits.is_empty() && !stdout.trim().is_empty() {
                    return Err(AikitError::new(
                        "knowledge.bkmr_output_drift",
                        "bkmr sem-search output could not be mapped back to stable SourceRefs",
                    ));
                }
                hits.truncate(limit);
                Ok(hits)
            }
        }
    }

    fn status(&self) -> SourceProviderStatus {
        let capabilities = self.capabilities();
        let version = capabilities.version.clone();
        SourceProviderStatus {
            provider: self.provider.clone(),
            available: self.cli.available,
            version: version.clone(),
            tested_version: Some(BKMR_GLADE_CONFORMANCE_VERSION.into()),
            version_drift: version
                .as_deref()
                .is_some_and(|value| value != BKMR_GLADE_CONFORMANCE_VERSION),
            capabilities,
            detail: format!("db={}", self.db_path.display()),
        }
    }
}

fn discover_cli<R: CommandRunner>(runner: &R, binary: &str) -> BkmrCliSurface {
    let version_argv = vec![binary.to_string(), "--version".into()];
    let version_output = match runner.run(&version_argv) {
        Ok(output) if output.ok() => output,
        Ok(output) => {
            return BkmrCliSurface {
                available: false,
                version: None,
                fulltext_json: false,
                fuzzy_interactive: false,
                semantic_cli: false,
                hybrid_json: false,
                tags: false,
                reason: Some(format!("bkmr --version exited with status {}", output.status)),
            }
        }
        Err(error) => {
            return BkmrCliSurface {
                available: false,
                version: None,
                fulltext_json: false,
                fuzzy_interactive: false,
                semantic_cli: false,
                hybrid_json: false,
                tags: false,
                reason: Some(error.to_string()),
            }
        }
    };
    let version = parse_version(&format!("{} {}", version_output.stdout, version_output.stderr));
    let top = probe_help(runner, binary, &["--help"]);
    let search = probe_help(runner, binary, &["search", "--help"]);
    let hybrid = probe_help(runner, binary, &["hsearch", "--help"]);
    BkmrCliSurface {
        available: true,
        version,
        fulltext_json: top.contains("search") && search.contains("--json"),
        fuzzy_interactive: search.contains("--fzf"),
        semantic_cli: top.contains("sem-search"),
        hybrid_json: top.contains("hsearch") && hybrid.contains("--json"),
        tags: top.contains("tag") && top.contains("tags"),
        reason: None,
    }
}

fn probe_help<R: CommandRunner>(runner: &R, binary: &str, args: &[&str]) -> String {
    let mut argv = vec![binary.to_string()];
    argv.extend(args.iter().map(|arg| (*arg).to_string()));
    runner
        .run(&argv)
        .ok()
        .filter(|output| output.ok())
        .map(|output| output.stdout)
        .unwrap_or_default()
}

fn parse_version(text: &str) -> Option<String> {
    text.split_whitespace()
        .map(|token| token.trim_matches(|ch: char| !ch.is_ascii_digit() && ch != '.'))
        .find(|token| {
            let parts = token.split('.').collect::<Vec<_>>();
            parts.len() == 3
                && parts
                    .iter()
                    .all(|part| !part.is_empty() && part.chars().all(|ch| ch.is_ascii_digit()))
        })
        .map(str::to_string)
}

fn marker_ref(description: &str) -> Option<SourceRef> {
    description
        .split_whitespace()
        .find_map(|part| part.strip_prefix(REF_PREFIX))
        .and_then(|raw| SourceRef::parse(raw).ok())
}

fn provider_tags(value: Option<&Value>) -> Option<Vec<String>> {
    match value {
        Some(Value::Array(values)) => Some(
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect(),
        ),
        Some(Value::String(raw)) => Some(
            raw.split(|ch: char| ch == ',' || ch.is_whitespace())
                .filter(|tag| !tag.is_empty())
                .map(str::to_string)
                .collect(),
        ),
        _ => None,
    }
}

fn value_string(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string())
}

fn json_records(stdout: &str) -> Result<Vec<Map<String, Value>>> {
    let value: Value = serde_json::from_str(stdout).map_err(|error| {
        AikitError::new(
            "knowledge.bkmr_invalid_json",
            format!("bkmr returned invalid JSON: {error}"),
        )
    })?;
    let candidates = match value {
        Value::Array(values) => values,
        Value::Object(mut object) => {
            for key in ["hits", "results", "bookmarks"] {
                if let Some(Value::Array(values)) = object.remove(key) {
                    return Ok(values.into_iter().filter_map(ValueObjectOwned::into_object).collect());
                }
            }
            vec![Value::Object(object)]
        }
        _ => Vec::new(),
    };
    Ok(candidates
        .into_iter()
        .filter_map(ValueObjectOwned::into_object)
        .collect())
}

trait ValueObjectOwned {
    fn into_object(self) -> Option<Map<String, Value>>;
}

impl ValueObjectOwned for Value {
    fn into_object(self) -> Option<Map<String, Value>> {
        match self {
            Value::Object(object) => Some(object),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use aikit_core::knowledge_source_pool::{SourceVisibility, SourceProviderStatus};
    use aikit_core::resource::SourceRevision;

    use super::*;
    use crate::runner::ScriptedRunner;

    fn scripted(search_json: &str) -> Arc<ScriptedRunner> {
        Arc::new(
            ScriptedRunner::new()
                .on("bkmr --version", "bkmr 7.6.7\n")
                .on(
                    "bkmr --help",
                    "commands: search sem-search hsearch tag tags create-db add show info\n",
                )
                .on("bkmr search --help", "options: --json --fzf --tags --np --no-color\n")
                .on("bkmr hsearch --help", "options: --json --tags --limit --np\n")
                .on("create-db", "created\n")
                .on(" add ", "added\n")
                .on(" search quasars ", search_json),
        )
    }

    fn astronomy() -> SourceMaterial {
        SourceMaterial {
            binding: SourceBinding {
                source: SourceRef::parse("source:astronomy").unwrap(),
                revision: SourceRevision::parse("sha256:abc").unwrap(),
                title: "Astronomy".into(),
                tags: vec!["astronomy".into(), "science".into()],
                visibility: SourceVisibility::Team,
                owners: Vec::new(),
                media_type: "text/markdown".into(),
                locator: None,
                metadata: BTreeMap::new(),
            },
            body: "Astronomy uses a telescope to observe distant quasars.".into(),
        }
    }

    #[test]
    fn discovery_matches_the_glade_767_contract() {
        let runner = scripted("[]");
        let provider = BkmrSourcePoolProvider::new(runner, "/tmp/aikit-bkmr-discovery.db", false);
        let status: SourceProviderStatus = provider.status();
        assert!(status.available);
        assert_eq!(status.version.as_deref(), Some("7.6.7"));
        assert_eq!(status.tested_version.as_deref(), Some("7.6.7"));
        assert!(!status.version_drift);
        assert!(status.capabilities.fulltext);
        assert!(status.capabilities.fuzzy_interactive);
        assert!(status.capabilities.tags);
        assert!(status.capabilities.structured_output);
        assert!(!status.capabilities.semantic);
        assert!(!status.capabilities.hybrid);
        assert!(status.capabilities.reasons["semantic"].contains("without embeddings"));
    }

    #[test]
    fn rebuild_and_search_preserve_canonical_source_refs() {
        let response = r#"[{"bookmark":{"id":41,"title":"Astronomy","description":"aikit-source-ref:source:astronomy","tags":["astronomy","science"],"content":"quasars"},"score":0.9}]"#;
        let runner = scripted(response);
        let calls = Arc::clone(&runner);
        let mut provider = BkmrSourcePoolProvider::new(runner, "/tmp/aikit-bkmr-contract.db", false);
        provider.rebuild(&[astronomy()]).unwrap();
        let hits = provider
            .search("quasars", SourceSearchMode::Fulltext, &[], 20)
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].source.as_str(), "source:astronomy");
        assert_eq!(hits[0].provider_binding.as_deref(), Some("41"));
        assert!(calls.call_lines().iter().any(|line| {
            line.contains("--description aikit-source-ref:source:astronomy")
                && line.contains("--no-embed")
        }));
        assert!(calls
            .call_lines()
            .iter()
            .any(|line| line.contains("search quasars --json --np --no-color")));
    }
}
