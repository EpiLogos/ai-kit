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
    SourceProviderStatus, SourceSearchMode, SourceVisibility, BKMR_GLADE_CONFORMANCE_VERSION,
};
use aikit_core::resource::{ProviderRef, SourceRef, SourceRevision};
use aikit_core::{AikitError, Result};
use serde_json::{json, Map, Value};

use crate::now_field::content_revision;
use crate::runner::CommandRunner;

const REF_PREFIX: &str = "aikit-source-ref:";

#[path = "bkmr_snapshot.rs"]
mod snapshot;

#[derive(Debug, Clone, PartialEq, Eq)]
struct BkmrCliSurface {
    available: bool,
    version: Option<String>,
    fulltext_json: bool,
    fuzzy_interactive: bool,
    semantic_cli: bool,
    hybrid_json: bool,
    tags: bool,
    /// Whether the global `--db <PATH>` selector is present. The provider's
    /// isolated-view integration requires it; older bkmr releases select the
    /// database only through config/env and are reported as unavailable with
    /// this gap named, never silently invoked.
    db_selector: bool,
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

    /// Why this provider cannot operate against the discovered CLI surface, if
    /// it cannot. Absence means the surface is operable.
    fn surface_reason(&self) -> Option<String> {
        if !self.cli.available {
            return Some(
                self.cli
                    .reason
                    .clone()
                    .unwrap_or_else(|| "bkmr executable is unavailable".into()),
            );
        }
        if !self.cli.db_selector {
            return Some(
                "installed bkmr exposes no --db selector; this provider's isolated-view integration requires the bkmr CLI surface that provides it".into(),
            );
        }
        None
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
        let operable = self.surface_reason().is_none();
        let mut reasons = BTreeMap::new();
        if let Some(reason) = self.surface_reason() {
            reasons.insert("provider".into(), reason);
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
            fulltext: operable && self.cli.fulltext_json,
            fuzzy_interactive: operable && self.cli.fuzzy_interactive,
            semantic: operable && semantic,
            hybrid: operable && hybrid,
            tags: operable && self.cli.tags,
            structured_output: operable && self.cli.fulltext_json && self.cli.hybrid_json,
            reasons,
        }
    }

    fn rebuild(&mut self, material: &[SourceMaterial]) -> Result<()> {
        // Central's persistent map is never a disposable SourcePool, even when
        // a stale standalone configuration still points at that database.
        let resolved = self
            .db_path
            .canonicalize()
            .unwrap_or_else(|_| self.db_path.clone());
        let parts: Vec<_> = resolved.components().collect();
        if parts
            .windows(2)
            .any(|p| p[0].as_os_str() == ".central" && p[1].as_os_str() == "bkmr")
        {
            return Err(AikitError::new(
                "knowledge.bkmr_owner_only",
                "Central-owned bkmr storage cannot be rebuilt by AIKit",
            ));
        }

        if let Some(reason) = self.surface_reason() {
            return Err(AikitError::new("knowledge.bkmr_unavailable", reason));
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
        // Only a view whose first creation this adapter owns can be replaced.
        // An explicit provider configuration is not ownership of existing data.
        let ownership = self.db_path.with_extension("aikit-disposable-owner");
        let owner_text = format!("aikit.bkmr-disposable/v1\n{}\n", self.db_path.display());
        let owner_is_regular = std::fs::symlink_metadata(&ownership)
            .is_ok_and(|metadata| metadata.file_type().is_file());
        if std::fs::symlink_metadata(&self.db_path).is_ok()
            && (!owner_is_regular
                || std::fs::read_to_string(&ownership).ok().as_deref() != Some(&owner_text))
        {
            return Err(AikitError::new(
                "knowledge.bkmr_not_disposable",
                "Existing bkmr database is not this adapter's disposable view; adopt it through Central",
            ));
        }
        if !owner_is_regular {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&ownership)
                .map_err(|e| AikitError::new("knowledge.bkmr_owner_failed", e.to_string()))?;
            file.write_all(owner_text.as_bytes())
                .and_then(|_| file.sync_all())
                .map_err(|e| AikitError::new("knowledge.bkmr_owner_failed", e.to_string()))?;
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
                    if let Some(id) = line
                        .split_whitespace()
                        .next()
                        .filter(|raw| !raw.is_empty() && raw.chars().all(|ch| ch.is_ascii_digit()))
                    {
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
                            let actual =
                                hit.tags.iter().map(String::as_str).collect::<BTreeSet<_>>();
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
        let detail = match self.surface_reason() {
            Some(reason) => format!("db={}; {reason}", self.db_path.display()),
            None => format!("db={}", self.db_path.display()),
        };
        SourceProviderStatus {
            provider: self.provider.clone(),
            available: self.surface_reason().is_none(),
            version: version.clone(),
            tested_version: Some(BKMR_GLADE_CONFORMANCE_VERSION.into()),
            version_drift: version
                .as_deref()
                .is_some_and(|value| value != BKMR_GLADE_CONFORMANCE_VERSION),
            capabilities,
            detail,
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
                db_selector: false,
                reason: Some(format!(
                    "bkmr --version exited with status {}",
                    output.status
                )),
            };
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
                db_selector: false,
                reason: Some(error.to_string()),
            };
        }
    };
    let version = parse_version(&format!(
        "{} {}",
        version_output.stdout, version_output.stderr
    ));
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
        // Exact whitespace-delimited token: "--debug" on older releases must
        // not false-positive the "--db" probe.
        db_selector: top.split_whitespace().any(|token| token == "--db"),
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
                    return Ok(values
                        .into_iter()
                        .filter_map(ValueObjectOwned::into_object)
                        .collect());
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

// ---------------------------------------------------------------------------
// The default read-only store pool (owner-corrected A-2)
// ---------------------------------------------------------------------------

/// bkmr is part of agent context sourcing by default (owner correction,
/// 2026-09-23): the faculty searches the configured bkmr stores — personal
/// stores included — read-only, without any opt-in capsule. What stays fenced
/// is *writing*: nothing here ever rebuilds, creates or adds; the disposable
/// rebuild path above remains the only write surface and refuses databases it
/// does not own.
pub const BKMR_STORES_PROVIDER_REF: &str = "provider/source-pool/bkmr-stores";

/// One configured bkmr store: a database file bkmr can open through `--db`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BkmrStore {
    /// The store's file stem (`books`, `agent-payment-protocol`).
    pub name: String,
    pub path: PathBuf,
}

/// Where bkmr's own configuration lives. `AIKIT_BKMR_CONFIG_DIR` exists so a
/// test or an isolated context can point the discovery at its own ground;
/// the default is the user's `~/.config/bkmr`.
pub fn bkmr_config_dir() -> PathBuf {
    std::env::var_os("AIKIT_BKMR_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(|home| PathBuf::from(home).join(".config/bkmr"))
                .unwrap_or_else(|| PathBuf::from(".config/bkmr"))
        })
}

/// Discover the configured stores: the active database named by
/// `config.toml` (`db_url`) plus every per-project database under
/// `projects/*.db`, deduplicated by resolved path. Distinct databases may
/// share a file stem (the active store and a per-project copy of the same
/// name), so display names are disambiguated by the parent directory. Only
/// stores that exist are listed; discovery never opens a database (opening
/// one can trigger bkmr's automatic schema migration, and that is bkmr's
/// business, not ours).
pub fn discover_bkmr_stores(config_dir: &Path) -> Vec<BkmrStore> {
    let mut stores: Vec<BkmrStore> = Vec::new();
    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
    let mut names: BTreeSet<String> = BTreeSet::new();
    let config_path = config_dir.join("config.toml");
    if let Ok(text) = std::fs::read_to_string(&config_path) {
        let active = toml::from_str::<toml::Value>(&text).ok().and_then(|value| {
            value
                .get("db_url")
                .and_then(|db| db.as_str())
                .map(str::to_owned)
        });
        if let Some(db_url) = active {
            let path = PathBuf::from(&db_url);
            let path = if path.is_absolute() {
                path
            } else {
                config_dir.join(path)
            };
            let resolved = path.canonicalize().unwrap_or_else(|_| path.clone());
            if resolved.is_file() && seen.insert(resolved) {
                let stem = path
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "active".into());
                let name = unique_display_name(stem, &path, &mut names);
                stores.push(BkmrStore { name, path });
            }
        }
    }
    if let Ok(entries) = std::fs::read_dir(config_dir.join("projects")) {
        let mut project_stores: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("db")
            })
            .collect();
        project_stores.sort();
        for path in project_stores {
            let stem = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default();
            if stem.is_empty() {
                continue;
            }
            // bkmr's own backup artefacts (`<name>_backup_<date>.db`, sometimes
            // nested) are never stores: they carry pre-migration schemas, so
            // opening one teaches bkmr to migrate it and mint yet another
            // backup — a feedback loop with the owner's human space.
            if is_backup_store_name(&stem) {
                continue;
            }
            let resolved = path.canonicalize().unwrap_or_else(|_| path.clone());
            if resolved.is_file() && seen.insert(resolved) {
                let name = unique_display_name(stem, &path, &mut names);
                stores.push(BkmrStore { name, path });
            }
        }
    }
    stores
}

/// A display name for a store: the file stem when no other store claims it,
/// otherwise qualified by the parent directory (a per-project copy of the
/// active store's name reads `projects/<stem>`), with a numeric suffix as
/// the last resort.
fn unique_display_name(stem: String, path: &Path, used: &mut BTreeSet<String>) -> String {
    if stem.is_empty() || used.insert(stem.clone()) {
        return stem;
    }
    let parent = path
        .parent()
        .and_then(|parent| parent.file_name())
        .map(|parent| parent.to_string_lossy().into_owned())
        .unwrap_or_default();
    let qualified = if parent.is_empty() {
        format!("{stem}-2")
    } else {
        format!("{parent}/{stem}")
    };
    if used.insert(qualified.clone()) {
        return qualified;
    }
    let mut suffix = 2;
    loop {
        let candidate = format!("{qualified}-{suffix}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        suffix += 1;
    }
}

/// Whether a store file stem names one of bkmr's automatic backups.
fn is_backup_store_name(name: &str) -> bool {
    name.contains("_backup_") || name.ends_with("-backup") || name.contains(".backup")
}

/// FTS5-safe query form: every whitespace-separated term is wrapped in double
/// quotes with embedded quotes doubled, so bkmr's SQLite FTS5 reads each term
/// as a literal instead of query syntax. A hyphenated term passed raw is FTS5
/// column syntax and fails the whole search — the same defect Central's own
/// file-map backend carries (`central.file_map_failure`); this pool does not.
pub fn fts5_quote_query(query: &str) -> String {
    query
        .split_whitespace()
        .filter(|term| !term.is_empty())
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The first line of an error message, bounded — bkmr failure output can run
/// to thousands of pool-diagnostic lines, and the disclosure only needs the
/// cause.
fn first_line(message: &str) -> String {
    let line = message.lines().next().unwrap_or_default();
    line.chars().take(200).collect()
}

/// Read-only search over the configured bkmr stores. Canonical SourceRefs
/// ride the bookmark description (`aikit-source-ref:`) when a store carries
/// them; personal bookmarks are minted refs in the pool's own namespace
/// (`source:bkmr:<store>:<id>`), readable back through `bkmr show`.
pub struct BkmrStoreSearchProvider<R> {
    runner: R,
    binary: String,
    stores: Vec<BkmrStore>,
    provider: ProviderRef,
    cli: BkmrCliSurface,
    /// Stores that failed this session's searches, named with a bounded
    /// reason. One broken store must not take the whole pool's answers down,
    /// and a skipped store must not stay silent: status carries the record.
    skipped: std::sync::Mutex<Vec<String>>,
}

impl<R: CommandRunner> BkmrStoreSearchProvider<R> {
    pub fn connect(runner: R, binary: impl Into<String>, stores: Vec<BkmrStore>) -> Self {
        let binary = binary.into();
        let cli = discover_cli(&runner, &binary);
        Self {
            runner,
            binary,
            stores,
            provider: ProviderRef::parse(BKMR_STORES_PROVIDER_REF).expect("static ref"),
            cli,
            skipped: std::sync::Mutex::new(Vec::new()),
        }
    }

    pub fn stores(&self) -> &[BkmrStore] {
        &self.stores
    }

    fn surface_reason(&self) -> Option<String> {
        if !self.cli.available {
            return Some(
                self.cli
                    .reason
                    .clone()
                    .unwrap_or_else(|| "bkmr executable is unavailable".into()),
            );
        }
        if !self.cli.db_selector {
            return Some(
                "installed bkmr exposes no --db selector; per-store search requires it".into(),
            );
        }
        if !self.cli.fulltext_json {
            return Some("installed bkmr exposes no `search --json` surface".into());
        }
        None
    }

    fn run_in_store(&self, store: &BkmrStore, args: &[String], code: &'static str) -> Result<String> {
        let snapshot = snapshot::Snapshot::read_only(&store.path)?;
        let mut argv = vec![
            self.binary.clone(),
            "--config".into(),
            snapshot.config.display().to_string(),
            "--db".into(),
            snapshot.database.display().to_string(),
        ];
        argv.extend(args.iter().cloned());
        self.runner
            .run_with_timeout(&argv, std::time::Duration::from_secs(30))?
            .require(&argv, code)
            .map(|output| output.stdout)
    }

    fn store_for(&self, name: &str) -> Option<&BkmrStore> {
        self.stores.iter().find(|store| store.name == name)
    }

    fn hit_from_record(
        &self,
        store: &BkmrStore,
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
        let source = match marker_ref(description) {
            Some(source) => source,
            None => {
                let id = bookmark.get("id").map(value_string).unwrap_or_default();
                SourceRef::parse(format!("source:bkmr:{}:{id}", store.name)).ok()?
            }
        };
        let score = ["rrf_score", "score", "similarity", "semantic_score"]
            .iter()
            .find_map(|key| {
                record
                    .get(*key)
                    .or_else(|| bookmark.get(*key))
                    .and_then(Value::as_f64)
            })
            .or_else(|| Some(1.0 / (rank as f64 + 1.0)));
        let mut tags = provider_tags(bookmark.get("tags")).unwrap_or_default();
        tags.push("bkmr-stores".into());
        tags.push(store.name.to_lowercase());
        Some(SourceHit {
            source,
            provider: self.provider.clone(),
            score,
            title: bookmark
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or(&store.name)
                .to_string(),
            snippet: ["content", "url", "description"]
                .iter()
                .find_map(|key| bookmark.get(*key).and_then(Value::as_str))
                .unwrap_or_default()
                .chars()
                .take(1000)
                .collect(),
            tags,
            provider_binding: bookmark.get("id").map(value_string),
            retrieval_mode: mode,
        })
    }
}

impl<R: CommandRunner> SourcePoolProvider for BkmrStoreSearchProvider<R> {
    fn capabilities(&self) -> SourceProviderCapabilities {
        let operable = self.surface_reason().is_none() && !self.stores.is_empty();
        let mut reasons = BTreeMap::new();
        if let Some(reason) = self.surface_reason() {
            reasons.insert("provider".into(), reason);
        } else if self.stores.is_empty() {
            reasons.insert(
                "stores".into(),
                "no configured bkmr stores were found; the faculty searches stores the owner configured, read-only".into(),
            );
        }
        SourceProviderCapabilities {
            provider: self.provider.clone(),
            version: self.cli.version.clone(),
            fulltext: operable,
            fuzzy_interactive: false,
            semantic: false,
            hybrid: false,
            tags: operable,
            structured_output: operable,
            reasons,
        }
    }

    /// Writes stay fenced: the store pool is search-only and can never
    /// materialise into a database, whatever it is pointed at.
    fn rebuild(&mut self, _: &[SourceMaterial]) -> Result<()> {
        Err(AikitError::new(
            "knowledge.bkmr_stores_owner_only",
            "bkmr stores are searched read-only; AIKit never creates, adds or rebuilds in them",
        ))
    }

    fn read(&self, source: &SourceRef) -> Result<Option<SourceMaterial>> {
        let raw = source.as_str();
        let prefix = "source:bkmr:";
        let Some(rest) = raw.strip_prefix(prefix) else {
            return Err(AikitError::new(
                "knowledge.bkmr_stores_out_of_scope",
                format!("{raw} is not a bkmr store ref"),
            ));
        };
        let Some((store_name, id)) = rest.split_once(':') else {
            return Err(AikitError::new(
                "knowledge.bkmr_stores_out_of_scope",
                format!("{raw} carries no store and id"),
            ));
        };
        let store = self.store_for(store_name).ok_or_else(|| {
            AikitError::new(
                "knowledge.bkmr_stores_out_of_scope",
                format!("store {store_name:?} is not configured"),
            )
        })?;
        if !id.chars().all(|ch| ch.is_ascii_digit()) {
            return Err(AikitError::new(
                "knowledge.bkmr_stores_out_of_scope",
                format!("{raw} does not name a bookmark id"),
            ));
        }
        let stdout = self.run_in_store(
            store,
            &[
                "show".into(),
                id.to_string(),
                "--json".into(),
            ],
            "knowledge.bkmr_stores_read_failed",
        )?;
        let records = json_records(&stdout)?;
        let record = records.first().ok_or_else(|| {
            AikitError::new(
                "knowledge.bkmr_stores_read_failed",
                format!("bkmr returned no record for {raw}"),
            )
        })?;
        let bookmark = record
            .get("bookmark")
            .and_then(Value::as_object)
            .unwrap_or(record);
        let body = ["content", "url", "description"]
            .iter()
            .find_map(|key| bookmark.get(*key).and_then(Value::as_str))
            .unwrap_or_default()
            .to_string();
        let title = bookmark
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or(&store.name)
            .to_string();
        let mut tags = provider_tags(bookmark.get("tags")).unwrap_or_default();
        tags.push("bkmr-stores".into());
        tags.push(store.name.to_lowercase());
        Ok(Some(SourceMaterial {
            binding: SourceBinding {
                source: source.clone(),
                revision: SourceRevision::parse(content_revision(body.as_bytes()))?,
                title,
                tags,
                visibility: SourceVisibility::Personal,
                owners: vec![],
                media_type: "text/plain".into(),
                locator: None,
                metadata: BTreeMap::from([
                    ("bkmr-stores".into(), json!({"store": store.name})),
                    ("owner_read_required".into(), json!(false)),
                ]),
            },
            body,
        }))
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
        if mode != SourceSearchMode::Fulltext {
            return Err(AikitError::new(
                "knowledge.source_provider_capability",
                format!(
                    "the bkmr store pool does not support {}: stores are searched full-text and read-only",
                    mode.as_str()
                ),
            ));
        }
        let caps = self.capabilities();
        if !caps.fulltext {
            let reason = caps
                .reasons
                .get("provider")
                .or_else(|| caps.reasons.get("stores"))
                .map(String::as_str)
                .unwrap_or("the store pool is not operable");
            return Err(AikitError::new(
                "knowledge.bkmr_stores_unavailable",
                reason.to_string(),
            ));
        }
        let fts_query = fts5_quote_query(query);
        if fts_query.is_empty() {
            return Ok(Vec::new());
        }
        let required: BTreeSet<&str> = tags.iter().map(String::as_str).collect();
        let mut merged: Vec<SourceHit> = Vec::new();
        for store in &self.stores {
            let stdout = match self.run_in_store(
                store,
                &[
                    "search".into(),
                    fts_query.clone(),
                    "--json".into(),
                    "--np".into(),
                    "--no-color".into(),
                ],
                "knowledge.bkmr_stores_search_failed",
            ) {
                Ok(stdout) => stdout,
                Err(error) => {
                    // One broken store (a locked database, a failed
                    // migration) skips that store and is named in status;
                    // the other stores still answer the query.
                    let reason = first_line(error.message());
                    self.skipped
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .push(format!("{}: {reason}", store.name));
                    continue;
                }
            };
            let records = match json_records(&stdout) {
                Ok(records) => records,
                Err(error) => {
                    let reason = first_line(error.message());
                    self.skipped
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .push(format!("{}: {reason}", store.name));
                    continue;
                }
            };
            merged.extend(
                records
                    .iter()
                    .enumerate()
                    .filter_map(|(rank, record)| self.hit_from_record(store, record, mode, rank))
                    .filter(|hit| {
                        required.is_subset(&hit.tags.iter().map(String::as_str).collect())
                    }),
            );
        }
        merged.sort_by(|left, right| {
            right
                .score
                .unwrap_or(0.0)
                .total_cmp(&left.score.unwrap_or(0.0))
                .then_with(|| left.source.cmp(&right.source))
        });
        merged.truncate(limit);
        Ok(merged)
    }

    fn status(&self) -> SourceProviderStatus {
        let capabilities = self.capabilities();
        let version = capabilities.version.clone();
        let names: Vec<&str> = self
            .stores
            .iter()
            .map(|store| store.name.as_str())
            .collect();
        let skipped = self
            .skipped
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .join("; ");
        let detail = match self.surface_reason() {
            Some(reason) => format!("read-only store pool unavailable: {reason}"),
            None => {
                let mut detail = format!(
                    "read-only search over {} configured bkmr store(s): {}; no write ever reaches them",
                    self.stores.len(),
                    names.join(", ")
                );
                if !skipped.is_empty() {
                    detail.push_str(&format!("; stores skipped this session: {skipped}"));
                }
                detail
            }
        };
        SourceProviderStatus {
            provider: self.provider.clone(),
            available: capabilities.fulltext,
            version: version.clone(),
            tested_version: Some(BKMR_GLADE_CONFORMANCE_VERSION.into()),
            version_drift: version
                .as_deref()
                .is_some_and(|value| value != BKMR_GLADE_CONFORMANCE_VERSION),
            capabilities,
            detail,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use aikit_core::knowledge_source_pool::{SourceProviderStatus, SourceVisibility};
    use aikit_core::resource::SourceRevision;

    use super::*;
    use crate::runner::ScriptedRunner;

    fn scripted(search_json: &str) -> Arc<ScriptedRunner> {
        Arc::new(
            ScriptedRunner::new()
                .on("bkmr --version", "bkmr 7.6.7\n")
                .on(
                    "bkmr --help",
                    "options: --db <DB>\ncommands: search sem-search hsearch tag tags create-db add show info\n",
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
    fn existing_unowned_database_is_never_rebuilt_even_with_working_cli() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hand-authored.db");
        std::fs::write(&path, b"retained database").unwrap();
        let runner = scripted("[]");
        let mut provider = BkmrSourcePoolProvider::new(Arc::clone(&runner), &path, false);
        assert!(provider.status().available);
        assert_eq!(
            provider.rebuild(&[astronomy()]).unwrap_err().code(),
            "knowledge.bkmr_not_disposable"
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"retained database");
        assert!(!runner
            .call_lines()
            .iter()
            .any(|line| line.contains("create-db ")));
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
        let mut provider =
            BkmrSourcePoolProvider::new(runner, "/tmp/aikit-bkmr-contract.db", false);
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

    #[test]
    fn cli_without_the_db_selector_is_unavailable_with_the_gap_named() {
        let runner = Arc::new(
            ScriptedRunner::new()
                .on("bkmr --version", "bkmr 6.5.0\n")
                .on(
                    "bkmr --help",
                    "options: --debug\ncommands: search sem-search tag tags create-db add show info\n",
                )
                .on(
                    "bkmr search --help",
                    "options: --json --fzf --tags --np --no-color\n",
                ),
        );
        let provider = BkmrSourcePoolProvider::new(runner, "/tmp/aikit-bkmr-nodb.db", false);
        let status = provider.status();
        assert!(!status.available);
        assert_eq!(status.version.as_deref(), Some("6.5.0"));
        assert_eq!(status.tested_version.as_deref(), Some("7.6.7"));
        assert!(status.version_drift);
        assert!(
            status.detail.contains("--db"),
            "the report names the missing selector: {}",
            status.detail
        );
        assert!(
            status.capabilities.reasons["provider"].contains("--db"),
            "the machine-readable report names the gap"
        );
        assert!(!status.capabilities.fulltext);
        let mut provider = provider;
        assert!(provider.rebuild(&[astronomy()]).is_err());
    }

    fn store_cli_scripted() -> ScriptedRunner {
        ScriptedRunner::new()
            .on("bkmr --version", "bkmr 7.6.7\n")
            .on(
                "bkmr --help",
                "options: --db <DB>\ncommands: search sem-search hsearch tag tags create-db add show info\n",
            )
            .on("bkmr search --help", "options: --json --fzf --tags --np --no-color\n")
            .on("bkmr hsearch --help", "options: --json --tags --limit --np\n")
    }

    fn sqlite_store(directory: &Path, name: &str) -> BkmrStore {
        let path = directory.join(format!("{name}.db"));
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch("CREATE TABLE bookmarks (id INTEGER PRIMARY KEY);").unwrap();
        store(name, &path)
    }

    fn store(name: &str, path: &Path) -> BkmrStore {
        BkmrStore {
            name: name.to_owned(),
            path: path.to_path_buf(),
        }
    }

    fn store_record_json(id: u64, title: &str, content: &str) -> String {
        format!(
            r#"[{{"bookmark":{{"id":{id},"title":"{title}","description":"human bookmark","tags":["reading"],"content":"{content}"}},"score":0.9}}]"#
        )
    }

    #[test]
    fn store_discovery_reads_the_active_config_and_project_stores_without_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path();
        let projects = config.join("projects");
        std::fs::create_dir_all(&projects).unwrap();
        let books = projects.join("books.db");
        std::fs::write(&books, b"db").unwrap();
        // The active database IS a project store: one file, one store.
        std::fs::write(
            config.join("config.toml"),
            format!("db_url = \"{}\"\n", books.display()),
        )
        .unwrap();
        std::fs::write(projects.join("alpha.db"), b"db").unwrap();
        std::fs::write(projects.join("notes.tsv"), b"not a db").unwrap();

        let stores = discover_bkmr_stores(config);
        let names: Vec<&str> = stores.iter().map(|store| store.name.as_str()).collect();
        assert_eq!(names, vec!["books", "alpha"], "{stores:?}");
        assert_eq!(stores[0].path, books);
    }

    #[test]
    fn discovery_never_lists_a_missing_database() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("config.toml"),
            "db_url = \"/nowhere/absent.db\"\n",
        )
        .unwrap();
        assert!(discover_bkmr_stores(dir.path()).is_empty());
    }

    #[test]
    fn stores_sharing_a_stem_are_disambiguated_not_silently_twinned() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path();
        let projects = config.join("projects");
        std::fs::create_dir_all(&projects).unwrap();
        // The live shape: the active database at the config root and a
        // distinct per-project database with the same stem.
        let active = config.join("agent-payment-protocol.db");
        std::fs::write(&active, b"db").unwrap();
        let project_copy = projects.join("agent-payment-protocol.db");
        std::fs::write(&project_copy, b"db2").unwrap();
        std::fs::write(
            config.join("config.toml"),
            format!("db_url = \"{}\"\n", active.display()),
        )
        .unwrap();

        let stores = discover_bkmr_stores(config);
        let names: Vec<&str> = stores.iter().map(|store| store.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["agent-payment-protocol", "projects/agent-payment-protocol"],
            "{stores:?}"
        );
        assert_eq!(stores[0].path, active);
        assert_eq!(stores[1].path, project_copy);
    }

    #[test]
    fn discovery_skips_bkmr_backup_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        let projects = dir.path().join("projects");
        std::fs::create_dir_all(&projects).unwrap();
        std::fs::write(projects.join("books_backup_20260923.db"), b"db").unwrap();
        std::fs::write(
            projects.join("books_backup_20260923_backup_20260923.db"),
            b"db",
        )
        .unwrap();
        std::fs::write(projects.join("books.db"), b"db").unwrap();
        let stores = discover_bkmr_stores(dir.path());
        let names: Vec<&str> = stores.iter().map(|store| store.name.as_str()).collect();
        assert_eq!(names, vec!["books"], "backups are never stores: {names:?}");
    }

    #[test]
    fn a_broken_store_is_skipped_and_named_while_the_others_answer() {
        let directory = tempfile::tempdir().unwrap();
        let healthy = sqlite_store(directory.path(), "kept");
        let runner = store_cli_scripted()
            .on(
                "--db",
                store_record_json(7, "Kept", "reachable content").as_str(),
            );
        let provider = BkmrStoreSearchProvider::connect(
            runner,
            "bkmr",
            vec![
                store("broken", &directory.path().join("missing.db")),
                healthy,
            ],
        );
        let hits = provider
            .search("reachable", SourceSearchMode::Fulltext, &[], 10)
            .unwrap();
        assert_eq!(hits.len(), 1, "the healthy store still answers: {hits:?}");
        let status = provider.status();
        assert!(
            status
                .detail
                .contains("stores skipped this session: broken"),
            "the skipped store is named: {}",
            status.detail
        );
    }

    #[test]
    fn personal_bookmarks_are_searched_read_only_with_minted_refs() {
        let directory = tempfile::tempdir().unwrap();
        let response = store_record_json(41, "A title", "civil time close discipline");
        let recorder =
            crate::runner::RecordingRunner::new(store_cli_scripted().on("--db", &response));
        let provider = BkmrStoreSearchProvider::connect(
            recorder,
            "bkmr",
            vec![sqlite_store(directory.path(), "books")],
        );
        let hits = provider
            .search("civil time", SourceSearchMode::Fulltext, &[], 10)
            .unwrap();
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].source.as_str(), "source:bkmr:books:41");
        assert!(hits[0].tags.contains(&"bkmr-stores".to_string()));
        assert!(hits[0].tags.contains(&"books".to_string()));
        assert_eq!(hits[0].provider.as_str(), BKMR_STORES_PROVIDER_REF);
        assert!(hits[0].snippet.contains("civil time close discipline"));
    }

    #[test]
    fn hyphenated_terms_reach_bkmr_as_quoted_fts5_literals() {
        let directory = tempfile::tempdir().unwrap();
        let runner = store_cli_scripted().on("--db", "[]");
        let recorder = crate::runner::RecordingRunner::new(runner);
        let provider = BkmrStoreSearchProvider::connect(
            &recorder,
            "bkmr",
            vec![sqlite_store(directory.path(), "books")],
        );
        let hits = provider
            .search(
                "aikit-knowledge-work-coverage-spec",
                SourceSearchMode::Fulltext,
                &[],
                10,
            )
            .unwrap();
        assert!(hits.is_empty());
        let search = recorder
            .calls()
            .into_iter()
            .find(|argv| {
                argv.iter().any(|arg| arg == "search")
                    && argv.iter().any(|arg| arg.contains("aikit-knowledge"))
            })
            .expect("a store search ran");
        let position = search
            .iter()
            .position(|arg| arg.contains("aikit-knowledge"))
            .expect("the query travelled");
        assert_eq!(
            search[position], "\"aikit-knowledge-work-coverage-spec\"",
            "the hyphenated term is FTS5-quoted, not raw column syntax"
        );
    }

    #[test]
    fn a_canonical_source_ref_in_a_description_is_kept() {
        let directory = tempfile::tempdir().unwrap();
        let response = r#"[{"bookmark":{"id":7,"title":"S","description":"aikit-source-ref:source:astronomy","tags":[],"content":"quasars"},"score":0.8}]"#;
        let runner = store_cli_scripted().on("--db", response);
        let provider = BkmrStoreSearchProvider::connect(
            runner,
            "bkmr",
            vec![sqlite_store(directory.path(), "books")],
        );
        let hits = provider
            .search("quasars", SourceSearchMode::Fulltext, &[], 10)
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].source.as_str(), "source:astronomy");
    }

    #[test]
    fn store_reads_and_writes_stay_on_their_sides_of_the_fence() {
        let directory = tempfile::tempdir().unwrap();
        let runner = store_cli_scripted().on(
            "--db",
            r#"[{"bookmark":{"id":41,"title":"A","description":"human bookmark","tags":[],"content":"the body"}}]"#,
        );
        let mut provider = BkmrStoreSearchProvider::connect(
            runner,
            "bkmr",
            vec![sqlite_store(directory.path(), "books")],
        );
        let reading = provider
            .read(&SourceRef::parse("source:bkmr:books:41").unwrap())
            .unwrap()
            .expect("a personal bookmark reads back live");
        assert_eq!(reading.body, "the body");
        assert_eq!(
            reading.binding.revision.as_str(),
            content_revision(b"the body")
        );

        let error = provider
            .read(&SourceRef::parse("source:bkmr:other:41").unwrap())
            .unwrap_err();
        assert_eq!(error.code(), "knowledge.bkmr_stores_out_of_scope");
        let error = provider
            .read(&SourceRef::parse("source:bkmr:books:not-an-id").unwrap())
            .unwrap_err();
        assert_eq!(error.code(), "knowledge.bkmr_stores_out_of_scope");

        let error = provider.rebuild(&[astronomy()]).unwrap_err();
        assert_eq!(error.code(), "knowledge.bkmr_stores_owner_only");
    }

    #[test]
    fn store_pool_status_discloses_the_stores_and_the_read_only_posture() {
        let provider = BkmrStoreSearchProvider::connect(
            store_cli_scripted(),
            "bkmr",
            vec![
                store("books", Path::new("/stores/books.db")),
                store("epi-logos", Path::new("/stores/epi-logos.db")),
            ],
        );
        let status = provider.status();
        assert!(status.available);
        assert!(status.detail.contains("2 configured bkmr store(s)"));
        assert!(status.detail.contains("books, epi-logos"));
        assert!(status.detail.contains("read-only"));
        let empty = BkmrStoreSearchProvider::connect(store_cli_scripted(), "bkmr", vec![]);
        assert!(!empty.status().available);
        assert!(empty.status().capabilities.reasons["stores"].contains("no configured bkmr stores"));
    }
}
