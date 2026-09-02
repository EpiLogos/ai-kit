//! GitNexus-backed ProjectMap code intelligence.
//!
//! AIKit consumes the current direct CLI tool surface. Git remains canonical;
//! the GitNexus graph is explicitly derived intelligence and may be rebuilt or
//! absent without changing [`CodeReference`] identity.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use aikit_core::knowledge_code::{
    CodeChanges, CodeContext, CodeImpact, CodeIndexCapabilities, CodeIndexProvider,
    CodeIndexStatus, CodeReference, CodeSearchHit, CodeStructuralCheck, CodeTrace,
    GITNEXUS_TESTED_VERSION,
};
use aikit_core::resource::{ProviderRef, SourceRef, SourceRevision};
use aikit_core::{AikitError, Result};
use serde_json::{Map, Value};

use crate::runner::CommandRunner;

#[derive(Debug, Clone, PartialEq, Eq)]
struct GitNexusCliSurface {
    available: bool,
    version: Option<String>,
    index: bool,
    search: bool,
    context: bool,
    impact: bool,
    trace: bool,
    detect_changes: bool,
    structural_check: bool,
    cypher: bool,
    pdg_impact: bool,
    reason: Option<String>,
}

pub struct GitNexusCodeIndexProvider<R> {
    runner: R,
    binary: String,
    repo_name: String,
    source: SourceRef,
    revision: Option<SourceRevision>,
    provider: ProviderRef,
    root: Option<PathBuf>,
    indexed: bool,
    cli: GitNexusCliSurface,
}

impl<R: CommandRunner> GitNexusCodeIndexProvider<R> {
    pub fn new(
        runner: R,
        repo_name: impl Into<String>,
        source: SourceRef,
        revision: Option<SourceRevision>,
    ) -> Self {
        Self::with_binary(runner, "gitnexus", repo_name, source, revision)
    }

    pub fn with_binary(
        runner: R,
        binary: impl Into<String>,
        repo_name: impl Into<String>,
        source: SourceRef,
        revision: Option<SourceRevision>,
    ) -> Self {
        let binary = binary.into();
        let cli = discover_cli(&runner, &binary);
        Self {
            runner,
            binary,
            repo_name: repo_name.into(),
            source,
            revision,
            provider: ProviderRef::parse("provider/code-index/gitnexus")
                .expect("static GitNexus provider ref must be valid"),
            root: None,
            indexed: false,
            cli,
        }
    }

    fn argv(&self, args: &[String]) -> Vec<String> {
        let mut argv = vec![self.binary.clone()];
        argv.extend(args.iter().cloned());
        argv
    }

    fn run_json(&self, args: &[String], code: &'static str) -> Result<Value> {
        let argv = self.argv(args);
        let stdout = self.runner.run(&argv)?.require(&argv, code)?.stdout;
        serde_json::from_str(stdout.trim()).map_err(|error| {
            AikitError::new(
                "knowledge.gitnexus_invalid_json",
                format!("GitNexus returned invalid JSON: {error}"),
            )
            .with("command", argv.join(" "))
        })
    }

    fn run_text(&self, args: &[String], code: &'static str) -> Result<String> {
        let argv = self.argv(args);
        Ok(self
            .runner
            .run(&argv)?
            .require(&argv, code)?
            .stdout
            .trim()
            .to_string())
    }

    fn require_capability(&self, supported: bool, operation: &str) -> Result<()> {
        if !self.cli.available {
            return Err(AikitError::new(
                "knowledge.gitnexus_unavailable",
                self.cli
                    .reason
                    .clone()
                    .unwrap_or_else(|| "GitNexus executable is unavailable".into()),
            ));
        }
        if !supported {
            return Err(AikitError::new(
                "knowledge.gitnexus_capability",
                format!("running GitNexus does not expose `{operation}`"),
            ));
        }
        Ok(())
    }

    fn require_indexed(&self) -> Result<()> {
        if self.indexed {
            Ok(())
        } else {
            Err(AikitError::new(
                "knowledge.gitnexus_not_indexed",
                "GitNexus code provider has not indexed the Project source in this AIKit provider instance",
            ))
        }
    }

    fn reference_from_object(&self, object: &Map<String, Value>) -> Option<CodeReference> {
        let path = string_field(object, &["filePath", "file_path", "path"])?;
        let symbol = string_field(
            object,
            &["name", "symbol", "qualifiedName", "qualified_name"],
        );
        let kind = string_field(object, &["kind", "type", "label"]);
        let line = integer_field(object, &["line", "startLine", "start_line"])
            .and_then(|line| u32::try_from(line).ok());
        Some(CodeReference {
            source: self.source.clone(),
            revision: self.revision.clone(),
            path,
            symbol,
            kind,
            line,
        })
    }

    fn search_hits(&self, value: &Value, limit: usize) -> Vec<CodeSearchHit> {
        let mut objects = Vec::new();
        collect_objects(value, &mut objects);
        let mut seen = BTreeSet::new();
        objects
            .into_iter()
            .filter_map(|object| {
                let reference = self.reference_from_object(object)?;
                let key = format!(
                    "{}\0{}\0{}",
                    reference.path,
                    reference.symbol.as_deref().unwrap_or(""),
                    reference.kind.as_deref().unwrap_or("")
                );
                if !seen.insert(key) {
                    return None;
                }
                let provider_binding = string_field(object, &["uid", "id", "nodeId", "node_id"]);
                let score = float_field(object, &["score", "relevance", "similarity"]);
                let title = reference
                    .symbol
                    .clone()
                    .unwrap_or_else(|| reference.path.clone());
                let snippet = string_field(object, &["content", "snippet", "description"])
                    .unwrap_or_default()
                    .chars()
                    .take(1200)
                    .collect::<String>();
                Some(CodeSearchHit {
                    resource: reference.resource_ref(),
                    reference,
                    title,
                    score,
                    snippet,
                    provider: self.provider.clone(),
                    provider_binding,
                })
            })
            .take(limit)
            .collect()
    }

    fn symbol_args(&self, command: &str, reference: &CodeReference) -> Result<Vec<String>> {
        let symbol = reference.symbol.as_deref().ok_or_else(|| {
            AikitError::new(
                "knowledge.code_symbol_required",
                format!("`{command}` requires a symbol-level CodeReference"),
            )
            .with("path", reference.path.clone())
        })?;
        Ok(vec![
            command.into(),
            symbol.into(),
            "--file".into(),
            reference.path.clone(),
            "--repo".into(),
            self.repo_name.clone(),
        ])
    }
}

impl<R: CommandRunner> CodeIndexProvider for GitNexusCodeIndexProvider<R> {
    fn capabilities(&self) -> CodeIndexCapabilities {
        CodeIndexCapabilities {
            provider: self.provider.clone(),
            version: self.cli.version.clone(),
            index: self.cli.available && self.cli.index,
            search: self.cli.available && self.cli.search,
            context: self.cli.available && self.cli.context,
            impact: self.cli.available && self.cli.impact,
            trace: self.cli.available && self.cli.trace,
            detect_changes: self.cli.available && self.cli.detect_changes,
            structural_check: self.cli.available && self.cli.structural_check,
            cypher: self.cli.available && self.cli.cypher,
            pdg_impact: self.cli.available && self.cli.pdg_impact,
            // GitNexus 1.6.9 intentionally renders direct-CLI detect-changes as
            // human-readable text, so the complete direct surface is mixed-format.
            structured_output: false,
        }
    }

    fn status(&self) -> CodeIndexStatus {
        let capabilities = self.capabilities();
        let version = capabilities.version.clone();
        CodeIndexStatus {
            provider: self.provider.clone(),
            available: self.cli.available,
            version: version.clone(),
            tested_version: Some(GITNEXUS_TESTED_VERSION.into()),
            version_drift: version
                .as_deref()
                .is_some_and(|value| value != GITNEXUS_TESTED_VERSION),
            indexed: self.indexed,
            capabilities,
            detail: self
                .root
                .as_ref()
                .map(|root| format!("repo={} root={}", self.repo_name, root.display()))
                .unwrap_or_else(|| format!("repo={} root=unmaterialised", self.repo_name)),
        }
    }

    fn index(&mut self, root: &Path, force: bool) -> Result<CodeIndexStatus> {
        self.require_capability(self.cli.index, "analyze")?;
        let mut args = vec![
            "analyze".into(),
            root.display().to_string(),
            "--index-only".into(),
            "--name".into(),
            self.repo_name.clone(),
        ];
        if force {
            args.push("--force".into());
        }
        let argv = self.argv(&args);
        self.runner
            .run(&argv)?
            .require(&argv, "knowledge.gitnexus_index_failed")?;
        self.root = Some(root.to_path_buf());
        self.indexed = true;
        Ok(self.status())
    }

    fn search(&self, query: &str, limit: usize) -> Result<Vec<CodeSearchHit>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        self.require_capability(self.cli.search, "query")?;
        self.require_indexed()?;
        let value = self.run_json(
            &[
                "query".into(),
                query.into(),
                "--repo".into(),
                self.repo_name.clone(),
                "--limit".into(),
                limit.to_string(),
            ],
            "knowledge.gitnexus_query_failed",
        )?;
        Ok(self.search_hits(&value, limit))
    }

    fn context(&self, reference: &CodeReference) -> Result<CodeContext> {
        self.require_capability(self.cli.context, "context")?;
        self.require_indexed()?;
        let args = self.symbol_args("context", reference)?;
        Ok(CodeContext {
            reference: reference.clone(),
            provider: self.provider.clone(),
            detail: self.run_json(&args, "knowledge.gitnexus_context_failed")?,
        })
    }

    fn impact(&self, reference: &CodeReference, direction: &str) -> Result<CodeImpact> {
        self.require_capability(self.cli.impact, "impact")?;
        self.require_indexed()?;
        if !matches!(direction, "upstream" | "downstream") {
            return Err(AikitError::new(
                "knowledge.gitnexus_direction",
                "GitNexus impact direction must be `upstream` or `downstream`",
            )
            .with("direction", direction.to_string()));
        }
        let mut args = self.symbol_args("impact", reference)?;
        args.extend(["--direction".into(), direction.into()]);
        Ok(CodeImpact {
            reference: reference.clone(),
            provider: self.provider.clone(),
            detail: self.run_json(&args, "knowledge.gitnexus_impact_failed")?,
        })
    }

    fn trace(&self, from: &CodeReference, to: &CodeReference) -> Result<CodeTrace> {
        self.require_capability(self.cli.trace, "trace")?;
        self.require_indexed()?;
        let from_symbol = from.symbol.as_deref().ok_or_else(|| {
            AikitError::new(
                "knowledge.code_symbol_required",
                "trace source requires a symbol",
            )
        })?;
        let to_symbol = to.symbol.as_deref().ok_or_else(|| {
            AikitError::new(
                "knowledge.code_symbol_required",
                "trace target requires a symbol",
            )
        })?;
        let detail = self.run_json(
            &[
                "trace".into(),
                from_symbol.into(),
                to_symbol.into(),
                "--from-file".into(),
                from.path.clone(),
                "--to-file".into(),
                to.path.clone(),
                "--repo".into(),
                self.repo_name.clone(),
            ],
            "knowledge.gitnexus_trace_failed",
        )?;
        Ok(CodeTrace {
            from: from.clone(),
            to: to.clone(),
            provider: self.provider.clone(),
            detail,
        })
    }

    fn detect_changes(&self, scope: &str, base_ref: Option<&str>) -> Result<CodeChanges> {
        self.require_capability(self.cli.detect_changes, "detect-changes")?;
        self.require_indexed()?;
        if !matches!(scope, "unstaged" | "staged" | "all" | "compare") {
            return Err(AikitError::new(
                "knowledge.gitnexus_change_scope",
                "GitNexus change scope must be unstaged, staged, all, or compare",
            )
            .with("scope", scope.to_string()));
        }
        let mut args = vec![
            "detect-changes".into(),
            "--scope".into(),
            scope.into(),
            "--repo".into(),
            self.repo_name.clone(),
        ];
        if let Some(base_ref) = base_ref {
            args.extend(["--base-ref".into(), base_ref.into()]);
        }
        Ok(CodeChanges {
            provider: self.provider.clone(),
            scope: scope.into(),
            base_ref: base_ref.map(str::to_string),
            // Current upstream deliberately formats this direct-CLI operation for
            // humans. Preserve that provider truth instead of pretending it is JSON.
            detail: Value::String(
                self.run_text(&args, "knowledge.gitnexus_detect_changes_failed")?,
            ),
        })
    }

    fn structural_check(&self) -> Result<CodeStructuralCheck> {
        self.require_capability(self.cli.structural_check, "check")?;
        self.require_indexed()?;
        Ok(CodeStructuralCheck {
            provider: self.provider.clone(),
            detail: self.run_json(
                &[
                    "check".into(),
                    "--cycles".into(),
                    "--json".into(),
                    "--repo".into(),
                    self.repo_name.clone(),
                ],
                "knowledge.gitnexus_check_failed",
            )?,
        })
    }
}

fn discover_cli<R: CommandRunner>(runner: &R, binary: &str) -> GitNexusCliSurface {
    let version_argv = vec![binary.to_string(), "--version".into()];
    let version_output = match runner.run(&version_argv) {
        Ok(output) if output.ok() => output,
        Ok(output) => {
            return unavailable_surface(format!(
                "gitnexus --version exited with status {}",
                output.status
            ))
        }
        Err(error) => return unavailable_surface(error.to_string()),
    };
    let help = probe_help(runner, binary, &["--help"]);
    let impact_help = probe_help(runner, binary, &["impact", "--help"]);
    GitNexusCliSurface {
        available: true,
        version: parse_version(&format!(
            "{} {}",
            version_output.stdout, version_output.stderr
        )),
        index: help.contains("analyze"),
        search: help.contains("query"),
        context: help.contains("context"),
        impact: help.contains("impact"),
        trace: help.contains("trace"),
        detect_changes: help.contains("detect-changes") || help.contains("detect_changes"),
        structural_check: help.contains("check"),
        cypher: help.contains("cypher"),
        pdg_impact: impact_help.contains("--mode") && impact_help.contains("pdg"),
        reason: None,
    }
}

fn unavailable_surface(reason: String) -> GitNexusCliSurface {
    GitNexusCliSurface {
        available: false,
        version: None,
        index: false,
        search: false,
        context: false,
        impact: false,
        trace: false,
        detect_changes: false,
        structural_check: false,
        cypher: false,
        pdg_impact: false,
        reason: Some(reason),
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

fn string_field(object: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        object.get(*key).and_then(|value| match value {
            Value::String(value) if !value.is_empty() => Some(value.clone()),
            Value::Number(value) => Some(value.to_string()),
            _ => None,
        })
    })
}

fn integer_field(object: &Map<String, Value>, keys: &[&str]) -> Option<i64> {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(Value::as_i64))
}

fn float_field(object: &Map<String, Value>, keys: &[&str]) -> Option<f64> {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(Value::as_f64))
}

fn collect_objects<'a>(value: &'a Value, out: &mut Vec<&'a Map<String, Value>>) {
    match value {
        Value::Object(object) => {
            out.push(object);
            for value in object.values() {
                collect_objects(value, out);
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_objects(value, out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::runner::ScriptedRunner;

    use super::*;

    fn runner(query: &str) -> Arc<ScriptedRunner> {
        Arc::new(
            ScriptedRunner::new()
                .on("gitnexus --version", "1.6.9\n")
                .on(
                    "gitnexus --help",
                    "analyze query context impact trace detect-changes check cypher\n",
                )
                .on("gitnexus impact --help", "--mode <callgraph|pdg>\n")
                .on("analyze /tmp/project", "Indexed\n")
                .on("query auth", query)
                .on("context login", r#"{"symbol":{"name":"login"}}"#)
                .on("impact login", r#"{"risk":"LOW"}"#)
                .on("trace login validate", r#"{"status":"found","path":[]}"#)
                .on(
                    "detect-changes",
                    "Changed symbols: 0\nAffected processes: 0\n",
                )
                .on("check --cycles --json", r#"{"status":"clean","cycles":[]}"#),
        )
    }

    fn provider(query: &str) -> GitNexusCodeIndexProvider<Arc<ScriptedRunner>> {
        GitNexusCodeIndexProvider::new(
            runner(query),
            "demo",
            SourceRef::parse("source:git/demo").unwrap(),
            Some(SourceRevision::parse("git:abc").unwrap()),
        )
    }

    #[test]
    fn current_cli_surface_is_discovered() {
        let provider = provider("{}");
        let status = provider.status();
        assert!(status.available);
        assert_eq!(status.version.as_deref(), Some("1.6.9"));
        assert!(status.capabilities.index);
        assert!(status.capabilities.search);
        assert!(status.capabilities.context);
        assert!(status.capabilities.impact);
        assert!(status.capabilities.trace);
        assert!(status.capabilities.detect_changes);
        assert!(status.capabilities.structural_check);
        assert!(status.capabilities.cypher);
        assert!(status.capabilities.pdg_impact);
        assert!(!status.capabilities.structured_output);
    }

    #[test]
    fn query_provider_ids_never_become_code_identity() {
        let mut provider = provider(
            r#"{"processes":[{"symbols":[{"uid":"provider-77","name":"login","kind":"Function","filePath":"src/auth.rs","score":0.93}]}]}"#,
        );
        provider.index(Path::new("/tmp/project"), false).unwrap();
        let hits = provider.search("auth", 5).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].reference.path, "src/auth.rs");
        assert_eq!(hits[0].reference.symbol.as_deref(), Some("login"));
        assert_eq!(hits[0].provider_binding.as_deref(), Some("provider-77"));
        assert!(hits[0].resource.as_str().starts_with("code:"));
        assert!(!hits[0].resource.as_str().contains("provider-77"));
    }

    #[test]
    fn context_impact_trace_changes_and_checks_bind_to_current_cli() {
        let calls = runner("{}");
        let observed = Arc::clone(&calls);
        let mut provider = GitNexusCodeIndexProvider::new(
            calls,
            "demo",
            SourceRef::parse("source:git/demo").unwrap(),
            None,
        );
        provider.index(Path::new("/tmp/project"), false).unwrap();
        let login = CodeReference {
            source: SourceRef::parse("source:git/demo").unwrap(),
            revision: None,
            path: "src/auth.rs".into(),
            symbol: Some("login".into()),
            kind: Some("Function".into()),
            line: None,
        };
        let validate = CodeReference {
            path: "src/validate.rs".into(),
            symbol: Some("validate".into()),
            ..login.clone()
        };
        provider.context(&login).unwrap();
        provider.impact(&login, "upstream").unwrap();
        provider.trace(&login, &validate).unwrap();
        let changes = provider.detect_changes("compare", Some("main")).unwrap();
        assert!(changes.detail.is_string());
        provider.structural_check().unwrap();
        let lines = observed.call_lines();
        assert!(lines
            .iter()
            .any(|line| { line.contains("context login --file src/auth.rs --repo demo") }));
        assert!(lines.iter().any(|line| {
            line.contains("impact login --file src/auth.rs --repo demo --direction upstream")
        }));
        assert!(lines.iter().any(|line| {
            line.contains("trace login validate --from-file src/auth.rs --to-file src/validate.rs --repo demo")
        }));
        assert!(lines.iter().any(|line| {
            line.contains("detect-changes --scope compare --repo demo --base-ref main")
        }));
        assert!(lines
            .iter()
            .any(|line| { line.contains("check --cycles --json --repo demo") }));
    }
}
