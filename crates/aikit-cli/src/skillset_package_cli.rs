//! `aikit set package {inspect|plan|export|verify|diff}` — export a SkillSet
//! as a native agent package (PRAXIS-ARCHITECTURE §5).
//!
//! The SkillSet is the source. This module resolves its members at exact
//! capsule revisions, hands the pure `aikit_core::skillset_package` adapters a
//! [`PortableSkillPackage`], writes the rendered tree, and runs native
//! validation. It never writes into the canonical SkillSet or a capsule, and
//! it proves that by hashing both before and after an export.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use clap::{Args, Subcommand};
use serde_json::{json, Value};

use aikit_adapters::clients::agent_skills;
use aikit_core::capsule::Payload;
use aikit_core::catalog::Catalog;
use aikit_core::skillset::SkillSet;
use aikit_core::skillset_package::{
    self as pkgsdk, diff_provenance, plan_and_render, sha256_hex, target_for, CheckStatus,
    Discovery, FileMap, NativeValidation, PackageFile, PackageMember, PackageMetadata, PackagePlan,
    PackageSource, PackageTarget, PortableSkillPackage, Receipt, RenderedContent, RenderedFile,
    Severity, TargetId, UnresolvedMember, Validation, PROVENANCE_FILE,
};
use aikit_core::{AikitError, Result};
use aikit_store::home::AikitHome;
use aikit_store::{registry_skillsets, skillsets};

use crate::app::Service;

// ---------------------------------------------------------------------------
// CLI surface
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct SetPackageCmd {
    #[command(subcommand)]
    pub command: SetPackageSub,
}

#[derive(Debug, Subcommand)]
pub enum SetPackageSub {
    /// The resolved PortableSkillPackage (aikit.portable-skill-package/v1).
    Inspect(SetPackageArgs),
    /// The target plan: portable | translated | target-addition | unsupported.
    Plan(SetPackageArgs),
    /// Render the native package tree and its receipt. Never touches the set.
    Export(SetPackageArgs),
    /// Check an exported tree: structure, provenance, file hashes; `--native`
    /// runs the target's own validator / disposable discovery.
    Verify(SetPackageArgs),
    /// Compare an exported tree's provenance and files against the current source.
    Diff(SetPackageArgs),
}

#[derive(Debug, Args)]
pub struct SetPackageArgs {
    /// Home set name or registry semantic ref (`aikit:project-author`).
    #[arg(value_name = "SET")]
    pub set: String,
    /// openai | codex | claude | pi. Required except for `inspect`.
    #[arg(long, value_name = "TARGET")]
    pub target: Option<String>,
    /// Package directory (default: `./<package>-<target>`).
    #[arg(long, value_name = "DIR")]
    pub out: Option<PathBuf>,
    /// Run the target's native validation / disposable discovery.
    #[arg(long)]
    pub native: bool,
    /// Add the `.codex-plugin/plugin.json` compatibility overlay to `openai`.
    #[arg(long)]
    pub with_codex_overlay: bool,
    /// Export even when some members are unresolved (they stay `unsupported`).
    #[arg(long)]
    pub allow_partial: bool,
    /// Also write the receipt JSON to this file.
    #[arg(long, value_name = "FILE")]
    pub receipt: Option<PathBuf>,
}

/// Dispatch one `aikit set package` subcommand. Returns the reply data.
pub fn run(service: &Service, cmd: SetPackageCmd) -> Result<Value> {
    match cmd.command {
        SetPackageSub::Inspect(a) => {
            let loaded = load_package(service, &a.set)?;
            let mut value = serde_json::to_value(&loaded.package).map_err(json_error)?;
            if let Some(target) = a.target.as_deref() {
                let target = target_for(TargetId::parse(target)?, a.with_codex_overlay);
                value["capabilities"] =
                    serde_json::to_value(target.capabilities()).map_err(json_error)?;
            }
            Ok(value)
        }
        SetPackageSub::Plan(a) => {
            let loaded = load_package(service, &a.set)?;
            let target = target_of(&a)?;
            let plan = target.plan(&loaded.package);
            let mut value = serde_json::to_value(&plan).map_err(json_error)?;
            value["capabilities"] =
                serde_json::to_value(target.capabilities()).map_err(json_error)?;
            value["complete"] = json!(loaded.package.is_complete());
            Ok(value)
        }
        SetPackageSub::Export(a) => export(service, &a),
        SetPackageSub::Verify(a) => verify(service, &a),
        SetPackageSub::Diff(a) => diff(service, &a),
    }
}

fn target_of(a: &SetPackageArgs) -> Result<Box<dyn PackageTarget>> {
    let raw = a.target.as_deref().ok_or_else(|| {
        AikitError::new(
            "skillset.package.target_required",
            "pass --target openai|codex|claude|pi",
        )
    })?;
    Ok(target_for(TargetId::parse(raw)?, a.with_codex_overlay))
}

fn json_error(e: serde_json::Error) -> AikitError {
    AikitError::new("skillset.package.json", e.to_string())
}

fn io_err(code: &'static str, path: &Path, e: std::io::Error) -> AikitError {
    AikitError::new(code, format!("{}: {e}", path.display()))
        .with("path", path.display().to_string())
}

// ---------------------------------------------------------------------------
// Resolution
// ---------------------------------------------------------------------------

/// A resolved package plus the canonical source paths it was read from.
pub struct LoadedPackage {
    pub package: PortableSkillPackage,
    /// Set source (directory or index) and every member capsule root. Hashed
    /// before and after export to prove the source was not written.
    pub source_paths: Vec<PathBuf>,
}

/// Load a SkillSet by home name or registry semantic ref, with its neutral
/// `[package]` metadata, and resolve it into a [`PortableSkillPackage`].
pub fn load_package(service: &Service, set_ref: &str) -> Result<LoadedPackage> {
    let (set, metadata, mut source_paths) = load_set(service.home(), set_ref)?;
    let package = package_from_skillset(service.snapshot(), &set, set_ref, metadata)?;
    for member in &package.members {
        if let Some(root) = service
            .snapshot()
            .get(
                &member
                    .id
                    .parse()
                    .map_err(|_| AikitError::new("skillset.package.bad_id", member.id.clone()))?,
            )
            .and_then(|c| c.root.clone())
        {
            source_paths.push(root);
        }
    }
    Ok(LoadedPackage {
        package,
        source_paths,
    })
}

/// A home set (`<home>/skillsets/<name>/`, `[package]` in `set.toml`) or a
/// registry set (`[skillset.package]` in its index entry).
pub fn load_set(
    home: &AikitHome,
    set_ref: &str,
) -> Result<(SkillSet, Option<PackageMetadata>, Vec<PathBuf>)> {
    if registry_skillsets::is_semantic_ref(set_ref) {
        let found = registry_skillsets::find_entry(home, set_ref)?.ok_or_else(|| {
            AikitError::new(
                "skillset.unknown",
                format!("no registry declares the SkillSet `{set_ref}`"),
            )
            .with("set", set_ref.to_string())
        })?;
        let metadata = found
            .entry
            .package
            .clone()
            .map(PackageMetadata::from_toml_value)
            .transpose()?;
        let paths = vec![
            found.root.join(registry_skillsets::REGISTRY_SKILLSET_INDEX),
            found.root.join("skillsets").join(&found.entry.directory),
        ];
        let mut set = found.set;
        skillsets::resolve_references(home, &mut set)?;
        return Ok((set, metadata, paths));
    }
    let set = skillsets::load(home, set_ref)?;
    let dir = skillsets::dir(home, set_ref);
    let note = dir.join("set.toml");
    let metadata = if note.is_file() {
        let text =
            std::fs::read_to_string(&note).map_err(|e| io_err("skillset.unreadable", &note, e))?;
        let file: skillsets::SetFile = toml::from_str(&text).map_err(|e| {
            AikitError::new(
                "skillset.malformed",
                format!("{} is not a readable set note: {e}", note.display()),
            )
        })?;
        file.package
    } else {
        None
    };
    Ok((set, metadata, vec![dir]))
}

/// Resolve every member (flattened through children) against a catalogue at
/// its exact revision. Members that cannot be resolved are carried as
/// `unresolved` with a reason — never dropped.
///
/// This is the entry point for any loader that already holds a `SkillSet`
/// and its (optional) neutral package metadata, e.g. a registry-set loader.
pub fn package_from_skillset(
    catalog: &dyn Catalog,
    set: &SkillSet,
    skillset_ref: &str,
    metadata: Option<PackageMetadata>,
) -> Result<PortableSkillPackage> {
    let mut members = Vec::new();
    let mut unresolved = Vec::new();
    for id in set.all_members() {
        match resolve_member(catalog, &id) {
            Ok(member) => members.push(member),
            Err(reason) => unresolved.push(UnresolvedMember {
                id: id.to_string(),
                reason,
            }),
        }
    }
    PortableSkillPackage::build(PackageSource {
        skillset_ref: skillset_ref.to_string(),
        set_name: set.name.clone(),
        set_description: set.description.clone(),
        metadata,
        members,
        unresolved,
    })
}

fn resolve_member(
    catalog: &dyn Catalog,
    id: &aikit_core::CapsuleId,
) -> std::result::Result<PackageMember, String> {
    let capsule = catalog
        .get(id)
        .ok_or_else(|| "not in the resolved catalogue on this machine".to_string())?;
    let Payload::Skill(section) = &capsule.payload else {
        return Err(format!(
            "is a {} capsule; packages carry Skills",
            capsule.kind.as_str()
        ));
    };
    let root = capsule
        .root
        .clone()
        .ok_or_else(|| "has no payload directory on this machine".to_string())?;
    let revision = capsule
        .revision
        .as_ref()
        .map(|r| r.to_string())
        .ok_or_else(|| "has no content revision".to_string())?;
    let payload = root.join(if section.root.is_empty() {
        "payload"
    } else {
        section.root.as_str()
    });
    let skill = agent_skills::validate(&payload).map_err(|e| e.to_string())?;
    let mut files = Vec::new();
    for relative in &skill.files {
        let path = payload.join(relative);
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        files.push(PackageFile {
            path: relative.clone(),
            sha256: sha256_hex(&bytes),
            bytes: bytes.len() as u64,
            source: Some(path),
            inline: None,
        });
    }
    Ok(PackageMember {
        id: id.to_string(),
        form: aikit_core::method::praxis_form(&skill.description),
        name: skill.name,
        description: skill.description,
        revision,
        tools: section.tools.clone(),
        files,
    })
}

// ---------------------------------------------------------------------------
// Export / verify / diff
// ---------------------------------------------------------------------------

fn default_out(service: &Service, pkg: &PortableSkillPackage, target: TargetId) -> PathBuf {
    service
        .invocation_cwd()
        .join(format!("{}-{}", pkg.identity.name, target.as_str()))
}

fn absolute(service: &Service, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        service.invocation_cwd().join(path)
    }
}

fn export(service: &Service, a: &SetPackageArgs) -> Result<Value> {
    let loaded = load_package(service, &a.set)?;
    let pkg = &loaded.package;
    let target = target_of(a)?;
    if !pkg.is_complete() && !a.allow_partial {
        return Err(AikitError::new(
            "skillset.package.unresolved",
            format!(
                "{} member(s) of `{}` could not be resolved: {} — resolve them or pass --allow-partial",
                pkg.unresolved.len(),
                pkg.skillset_ref,
                pkg.unresolved
                    .iter()
                    .map(|u| format!("{} ({})", u.id, u.reason))
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
        ));
    }
    let out = a
        .out
        .as_deref()
        .map(|p| absolute(service, p))
        .unwrap_or_else(|| default_out(service, pkg, target.id()));
    guard_out(&out, &loaded.source_paths, pkg, target.id())?;

    let before = fingerprint(&loaded.source_paths);
    let (plan, rendered) = plan_and_render(target.as_ref(), pkg)?;
    write_tree(&out, &rendered)?;
    let after = fingerprint(&loaded.source_paths);

    let files = read_tree(&out)?;
    let mut validation = Validation::from_findings(target.structural_validation(&files));
    let mut discovery = None;
    if a.native {
        let (native, found) = native_check(target.as_ref(), &out, pkg, &plan);
        validation.native = native;
        discovery = found;
    }
    let mut receipt = Receipt::new(pkg, &plan, &rendered, validation);
    receipt.discovery = discovery;
    receipt.out_dir = Some(out.display().to_string());
    receipt.source_unchanged = Some(before == after);
    finish(a, receipt, &out)
}

fn finish(a: &SetPackageArgs, receipt: Receipt, out: &Path) -> Result<Value> {
    let value = serde_json::to_value(&receipt).map_err(json_error)?;
    if let Some(path) = &a.receipt {
        let text = serde_json::to_string_pretty(&value).map_err(json_error)? + "\n";
        std::fs::write(path, text).map_err(|e| io_err("skillset.package.write_failed", path, e))?;
    }
    if receipt.source_unchanged == Some(false) {
        return Err(AikitError::new(
            "skillset.package.source_changed",
            "the canonical SkillSet or a member capsule changed during export",
        ));
    }
    if !receipt.validation.ok() {
        let first = receipt
            .validation
            .findings
            .iter()
            .find(|f| f.severity == Severity::Error)
            .map(|f| format!("{}: {}", f.path, f.message))
            .unwrap_or_else(|| receipt.validation.native.summary.clone());
        return Err(AikitError::new(
            "skillset.package.validation_failed",
            format!(
                "{} package at {} did not validate: {first}",
                receipt.target,
                out.display()
            ),
        )
        .with("structural", format!("{:?}", receipt.validation.structural))
        .with("native", format!("{:?}", receipt.validation.native.status)));
    }
    if receipt
        .discovery
        .as_ref()
        .is_some_and(|d| d.status == CheckStatus::Failed)
    {
        return Err(AikitError::new(
            "skillset.package.discovery_failed",
            format!(
                "the host did not discover every exported Skill: missing {:?}",
                receipt.discovery.as_ref().map(|d| &d.missing_skills)
            ),
        ));
    }
    Ok(value)
}

fn verify(service: &Service, a: &SetPackageArgs) -> Result<Value> {
    let loaded = load_package(service, &a.set)?;
    let pkg = &loaded.package;
    let target = target_of(a)?;
    let out = a
        .out
        .as_deref()
        .map(|p| absolute(service, p))
        .unwrap_or_else(|| default_out(service, pkg, target.id()));
    let files = read_tree(&out)?;
    let (plan, rendered) = plan_and_render(target.as_ref(), pkg)?;
    let mut findings = target.structural_validation(&files);
    let drift = file_drift(&rendered, &files);
    for path in drift["changed"].as_array().into_iter().flatten() {
        findings.push(pkgsdk::Finding {
            path: path.as_str().unwrap_or_default().to_string(),
            severity: Severity::Error,
            message: "differs from what the current source renders".into(),
        });
    }
    for path in drift["missing"].as_array().into_iter().flatten() {
        findings.push(pkgsdk::Finding {
            path: path.as_str().unwrap_or_default().to_string(),
            severity: Severity::Error,
            message: "expected file is missing".into(),
        });
    }
    let mut validation = Validation::from_findings(findings);
    let mut discovery = None;
    if a.native {
        let (native, found) = native_check(target.as_ref(), &out, pkg, &plan);
        validation.native = native;
        discovery = found;
    }
    let mut receipt = Receipt::new(pkg, &plan, &rendered, validation);
    receipt.discovery = discovery;
    receipt.out_dir = Some(out.display().to_string());
    finish(a, receipt, &out)
}

fn diff(service: &Service, a: &SetPackageArgs) -> Result<Value> {
    let loaded = load_package(service, &a.set)?;
    let pkg = &loaded.package;
    let target = target_of(a)?;
    let out = a
        .out
        .as_deref()
        .map(|p| absolute(service, p))
        .unwrap_or_else(|| default_out(service, pkg, target.id()));
    let files = read_tree(&out)?;
    let provenance: Value = files
        .get(PROVENANCE_FILE)
        .and_then(|b| serde_json::from_slice(b).ok())
        .ok_or_else(|| {
            AikitError::new(
                "skillset.package.no_provenance",
                format!(
                    "{} has no readable {PROVENANCE_FILE}; it was not exported by aikit",
                    out.display()
                ),
            )
        })?;
    let (_, rendered) = plan_and_render(target.as_ref(), pkg)?;
    let mut value = serde_json::to_value(diff_provenance(&provenance, pkg)).map_err(json_error)?;
    let drift = file_drift(&rendered, &files);
    value["files"] = drift.clone();
    let files_current = ["changed", "missing", "extra"]
        .iter()
        .all(|k| drift[*k].as_array().is_none_or(|a| a.is_empty()));
    value["current"] = json!(value["current"] == json!(true) && files_current);
    value["out_dir"] = json!(out.display().to_string());
    Ok(value)
}

/// Rendered-vs-disk file comparison. The provenance file is compared too.
fn file_drift(rendered: &[RenderedFile], files: &FileMap) -> Value {
    let mut changed = Vec::new();
    let mut missing = Vec::new();
    let expected: BTreeMap<&str, &str> = rendered
        .iter()
        .map(|f| (f.path.as_str(), f.sha256.as_str()))
        .collect();
    for (path, sha) in &expected {
        match files.get(*path) {
            None => missing.push(path.to_string()),
            Some(bytes) if sha256_hex(bytes) != *sha => changed.push(path.to_string()),
            _ => {}
        }
    }
    let extra: Vec<String> = files
        .keys()
        .filter(|k| !expected.contains_key(k.as_str()))
        .cloned()
        .collect();
    json!({"changed": changed, "missing": missing, "extra": extra})
}

/// Refuse to write into the canonical source, or over a directory that is not
/// a previous aikit export of the same package and target.
fn guard_out(
    out: &Path,
    sources: &[PathBuf],
    pkg: &PortableSkillPackage,
    target: TargetId,
) -> Result<()> {
    let canon_out = canonical_or_self(out);
    for source in sources {
        let canon_source = canonical_or_self(source);
        if canon_out.starts_with(&canon_source) || canon_source.starts_with(&canon_out) {
            return Err(AikitError::new(
                "skillset.package.out_overlaps_source",
                format!(
                    "{} overlaps the canonical source {}; export elsewhere",
                    out.display(),
                    source.display()
                ),
            ));
        }
    }
    if !out.exists() {
        return Ok(());
    }
    let empty = std::fs::read_dir(out)
        .map(|mut entries| entries.next().is_none())
        .unwrap_or(false);
    if empty {
        return Ok(());
    }
    let provenance: Option<Value> = std::fs::read(out.join(PROVENANCE_FILE))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
    let same = provenance.as_ref().is_some_and(|p| {
        p["package"]["name"] == json!(pkg.identity.name) && p["target"] == json!(target.as_str())
    });
    if !same {
        return Err(AikitError::new(
            "skillset.package.out_not_empty",
            format!(
                "{} is not empty and is not a previous aikit export of `{}` for {}; choose another --out",
                out.display(),
                pkg.identity.name,
                target.as_str()
            ),
        ));
    }
    // A previous export of this same package: it is generated material, so it
    // is replaced whole rather than merged (a merge would keep stale files).
    std::fs::remove_dir_all(out).map_err(|e| io_err("skillset.package.write_failed", out, e))
}

fn canonical_or_self(path: &Path) -> PathBuf {
    if let Ok(c) = path.canonicalize() {
        return c;
    }
    // Canonicalise the nearest existing ancestor so /tmp vs /private/tmp agree.
    let mut tail = Vec::new();
    let mut cursor = path;
    while let Some(parent) = cursor.parent() {
        if let Some(name) = cursor.file_name() {
            tail.push(name.to_os_string());
        }
        if let Ok(c) = parent.canonicalize() {
            let mut out = c;
            for part in tail.iter().rev() {
                out.push(part);
            }
            return out;
        }
        cursor = parent;
    }
    path.to_path_buf()
}

fn write_tree(out: &Path, files: &[RenderedFile]) -> Result<()> {
    std::fs::create_dir_all(out).map_err(|e| io_err("skillset.package.write_failed", out, e))?;
    for file in files {
        let path = out.join(&file.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| io_err("skillset.package.write_failed", parent, e))?;
        }
        match &file.content {
            RenderedContent::Bytes(bytes) => std::fs::write(&path, bytes)
                .map_err(|e| io_err("skillset.package.write_failed", &path, e))?,
            RenderedContent::Copy { source } => {
                std::fs::copy(source, &path)
                    .map_err(|e| io_err("skillset.package.write_failed", &path, e))?;
            }
        }
        #[cfg(unix)]
        if file.executable {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
        }
    }
    Ok(())
}

fn read_tree(dir: &Path) -> Result<FileMap> {
    if !dir.is_dir() {
        return Err(AikitError::new(
            "skillset.package.no_tree",
            format!("{} is not an exported package directory", dir.display()),
        ));
    }
    let mut files = FileMap::new();
    for entry in walkdir::WalkDir::new(dir).follow_links(false) {
        let entry =
            entry.map_err(|e| AikitError::new("skillset.package.unreadable", e.to_string()))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(dir)
            .unwrap_or(entry.path())
            .components()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("/");
        let bytes = std::fs::read(entry.path())
            .map_err(|e| io_err("skillset.package.unreadable", entry.path(), e))?;
        files.insert(rel, bytes);
    }
    Ok(files)
}

/// Content fingerprint of every file under the given source paths.
fn fingerprint(paths: &[PathBuf]) -> String {
    let mut lines = Vec::new();
    for root in paths {
        for entry in walkdir::WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .flatten()
        {
            if entry.file_type().is_file() {
                let sha = std::fs::read(entry.path())
                    .map(|b| sha256_hex(&b))
                    .unwrap_or_else(|e| format!("unreadable:{e}"));
                lines.push(format!("{}\0{sha}", entry.path().display()));
            }
        }
    }
    lines.sort();
    sha256_hex(lines.join("\n").as_bytes())
}

// ---------------------------------------------------------------------------
// Native validation and disposable discovery
// ---------------------------------------------------------------------------

fn on_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
}

fn unavailable(command: Option<String>, why: String) -> NativeValidation {
    NativeValidation {
        status: CheckStatus::Unavailable,
        command,
        exit: None,
        summary: why,
    }
}

fn exported_skill_names(pkg: &PortableSkillPackage, plan: &PackagePlan) -> Vec<String> {
    let carried = plan.carried_members();
    pkg.members
        .iter()
        .filter(|m| carried.contains(&m.id))
        .map(|m| m.name.clone())
        .collect()
}

fn native_check(
    target: &dyn PackageTarget,
    dir: &Path,
    pkg: &PortableSkillPackage,
    plan: &PackagePlan,
) -> (NativeValidation, Option<Discovery>) {
    match target.id() {
        TargetId::Claude => (claude_validate(target, dir), None),
        TargetId::Pi => pi_discover(target, dir, pkg, plan),
        TargetId::Openai | TargetId::Codex => codex_discover(dir, pkg, plan),
    }
}

fn claude_validate(target: &dyn PackageTarget, dir: &Path) -> NativeValidation {
    let Some(cmd) = target.native_validation(dir) else {
        return unavailable(None, "no native validator".into());
    };
    if !on_path(&cmd.program) {
        return unavailable(
            Some(cmd.display()),
            format!("`{}` is not on PATH on this machine", cmd.program),
        );
    }
    match Command::new(&cmd.program).args(&cmd.args).output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let report: Option<Value> = serde_json::from_str(stdout.trim()).ok();
            let success = report
                .as_ref()
                .and_then(|r| r["success"].as_bool())
                .unwrap_or(false);
            let errors = report
                .as_ref()
                .map(|r| r["manifest"]["errors"].clone())
                .unwrap_or(Value::Null);
            let warnings = report
                .as_ref()
                .map(|r| r["manifest"]["warnings"].clone())
                .unwrap_or(Value::Null);
            let passed = output.status.success() && success;
            NativeValidation {
                status: if passed {
                    CheckStatus::Passed
                } else {
                    CheckStatus::Failed
                },
                command: Some(cmd.display()),
                exit: output.status.code(),
                summary: format!(
                    "success={success} errors={errors} warnings={warnings}{}",
                    if report.is_none() {
                        format!(" stdout={}", stdout.trim())
                    } else {
                        String::new()
                    }
                ),
            }
        }
        Err(e) => unavailable(Some(cmd.display()), format!("could not run: {e}")),
    }
}

/// `pi --mode rpc … -e <dir>` with a disposable HOME, `get_commands`, then
/// check `skill:<name>` entries sourced from this package.
fn pi_discover(
    target: &dyn PackageTarget,
    dir: &Path,
    pkg: &PortableSkillPackage,
    plan: &PackagePlan,
) -> (NativeValidation, Option<Discovery>) {
    let Some(cmd) = target.native_validation(dir) else {
        return (unavailable(None, "no native validator".into()), None);
    };
    if !on_path(&cmd.program) {
        return (
            unavailable(
                Some(cmd.display()),
                format!("`{}` is not on PATH on this machine", cmd.program),
            ),
            None,
        );
    }
    let home = match tempfile::TempDir::new() {
        Ok(h) => h,
        Err(e) => return (unavailable(Some(cmd.display()), e.to_string()), None),
    };
    let display = format!("HOME=<disposable> {}", cmd.display());
    let mut child = match Command::new(&cmd.program)
        .args(&cmd.args)
        .env("HOME", home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            return (
                unavailable(Some(display), format!("could not run: {e}")),
                None,
            )
        }
    };
    let mut stdin = child.stdin.take().expect("piped stdin");
    let _ = stdin.write_all(cmd.stdin.clone().unwrap_or_default().as_bytes());
    let _ = stdin.flush();
    let stdout = child.stdout.take().expect("piped stdout");
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(|l| l.ok()) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let deadline = Instant::now() + Duration::from_secs(45);
    let mut response: Option<Value> = None;
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(line) => {
                if let Ok(v) = serde_json::from_str::<Value>(&line) {
                    if v["id"] == "1" && v["type"] == "response" {
                        response = Some(v);
                        break;
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    drop(stdin);
    let _ = child.kill();
    let status = child.wait().ok();
    let Some(response) = response else {
        return (
            NativeValidation {
                status: CheckStatus::Failed,
                command: Some(display),
                exit: status.and_then(|s| s.code()),
                summary: "pi returned no get_commands response within 45s".into(),
            },
            None,
        );
    };
    let root = canonical_or_self(dir);
    let commands = response["data"]["commands"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let from_package = |c: &Value| {
        c["sourceInfo"]["path"]
            .as_str()
            .is_some_and(|p| canonical_or_self(Path::new(p)).starts_with(&root))
    };
    let discovered: Vec<String> = commands
        .iter()
        .filter(|c| c["source"] == "skill" && from_package(c))
        .filter_map(|c| {
            c["name"]
                .as_str()?
                .strip_prefix("skill:")
                .map(str::to_string)
        })
        .collect();
    let expected = exported_skill_names(pkg, plan);
    let missing: Vec<String> = expected
        .iter()
        .filter(|n| !discovered.contains(n))
        .cloned()
        .collect();
    let extension_commands: Vec<String> = commands
        .iter()
        .filter(|c| c["source"] == "extension" && from_package(c))
        .filter_map(|c| c["name"].as_str().map(str::to_string))
        .collect();
    let missing_commands: Vec<String> = pkg
        .commands
        .iter()
        .map(|c| c.name.clone())
        .filter(|n| !extension_commands.contains(n))
        .collect();
    let passed = response["success"] == true && missing.is_empty() && missing_commands.is_empty();
    let status = if passed {
        CheckStatus::Passed
    } else {
        CheckStatus::Failed
    };
    (
        NativeValidation {
            status,
            command: Some(display),
            exit: Some(0),
            summary: format!(
                "get_commands success={}; package skills {:?}; package extension commands {:?}; missing skills {:?}; missing commands {:?}",
                response["success"], discovered, extension_commands, missing, missing_commands
            ),
        },
        Some(Discovery {
            status,
            method: "pi --mode rpc get_commands (disposable HOME)".into(),
            discovered_skills: discovered,
            missing_skills: missing,
            evidence: format!("{} commands reported", commands.len()),
        }),
    )
}

/// Disposable Codex marketplace load: a temp CODEX_HOME, a temp marketplace
/// wrapping a copy of the package, `marketplace add` → `plugin add` →
/// `plugin list`, then the installed cache is checked for every Skill.
/// `~/.codex/config.toml` is fingerprinted before and after.
fn codex_discover(
    dir: &Path,
    pkg: &PortableSkillPackage,
    plan: &PackagePlan,
) -> (NativeValidation, Option<Discovery>) {
    let method = "codex plugin marketplace add / plugin add / plugin list --json under a disposable CODEX_HOME";
    if !on_path("codex") {
        return (
            unavailable(
                Some(method.into()),
                "`codex` is not on PATH; Agent Plugins has no other native validator (structural check only)".into(),
            ),
            None,
        );
    }
    let user_config = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|h| h.join(".codex/config.toml"));
    let stamp = |p: &Option<PathBuf>| {
        p.as_ref()
            .and_then(|p| std::fs::metadata(p).ok())
            .map(|m| (m.len(), m.modified().ok()))
    };
    let before = stamp(&user_config);
    let result = (|| -> std::result::Result<(Value, Vec<String>, Vec<String>, String), String> {
        let temp = tempfile::TempDir::new().map_err(|e| e.to_string())?;
        let codex_home = temp.path().join("codex-home");
        let market = temp.path().join("marketplace");
        std::fs::create_dir_all(&codex_home).map_err(|e| e.to_string())?;
        let plugin_dir = market.join("plugins").join(&pkg.identity.name);
        copy_dir(dir, &plugin_dir).map_err(|e| e.to_string())?;
        let marketplace_name = "aikit-export-check";
        let manifest = json!({
            "name": marketplace_name,
            "plugins": [{
                "name": pkg.identity.name,
                "source": {"source": "local", "path": format!("./plugins/{}", pkg.identity.name)},
            }],
        });
        std::fs::create_dir_all(market.join(".agents/plugins")).map_err(|e| e.to_string())?;
        std::fs::write(
            market.join(".agents/plugins/marketplace.json"),
            serde_json::to_string_pretty(&manifest).unwrap_or_default(),
        )
        .map_err(|e| e.to_string())?;
        let run = |args: &[&str]| -> std::result::Result<String, String> {
            let output = Command::new("codex")
                .args(args)
                .env("CODEX_HOME", &codex_home)
                .output()
                .map_err(|e| e.to_string())?;
            if output.status.success() {
                Ok(String::from_utf8_lossy(&output.stdout).to_string())
            } else {
                Err(format!(
                    "codex {} exited {:?}: {}",
                    args.join(" "),
                    output.status.code(),
                    String::from_utf8_lossy(&output.stderr).trim()
                ))
            }
        };
        run(&[
            "plugin",
            "marketplace",
            "add",
            &market.display().to_string(),
        ])?;
        let plugin_id = format!("{}@{marketplace_name}", pkg.identity.name);
        let added = run(&["plugin", "add", &plugin_id, "--json"])?;
        let added: Value = serde_json::from_str(added.trim()).map_err(|e| e.to_string())?;
        let listed = run(&["plugin", "list", "--json"])?;
        let listed: Value = serde_json::from_str(listed.trim()).map_err(|e| e.to_string())?;
        let installed = listed["installed"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|p| p["pluginId"] == json!(plugin_id))
            .cloned()
            .ok_or_else(|| format!("{plugin_id} not listed as installed"))?;
        let installed_path = added["installedPath"].as_str().map(PathBuf::from);
        let mut discovered = Vec::new();
        let mut missing = Vec::new();
        for name in exported_skill_names(pkg, plan) {
            let found = installed_path
                .as_ref()
                .is_some_and(|p| p.join("skills").join(&name).join("SKILL.md").is_file());
            if found {
                discovered.push(name);
            } else {
                missing.push(name);
            }
        }
        let evidence = format!(
            "installed {} version {} enabled={}",
            installed["pluginId"], installed["version"], installed["enabled"]
        );
        Ok((installed, discovered, missing, evidence))
    })();
    let after = stamp(&user_config);
    let untouched = before == after;
    match result {
        Ok((installed, discovered, missing, evidence)) => {
            let passed =
                missing.is_empty() && untouched && installed["version"] == json!(pkg.version);
            let status = if passed {
                CheckStatus::Passed
            } else {
                CheckStatus::Failed
            };
            (
                NativeValidation {
                    status,
                    command: Some(method.into()),
                    exit: Some(0),
                    summary: format!(
                        "{evidence}; skills in installed cache {discovered:?}; missing {missing:?}; ~/.codex/config.toml untouched={untouched}"
                    ),
                },
                Some(Discovery {
                    status,
                    method: method.into(),
                    discovered_skills: discovered,
                    missing_skills: missing,
                    evidence,
                }),
            )
        }
        Err(why) => (
            NativeValidation {
                status: CheckStatus::Failed,
                command: Some(method.into()),
                exit: None,
                summary: format!("{why}; ~/.codex/config.toml untouched={untouched}"),
            },
            None,
        ),
    }
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    for entry in walkdir::WalkDir::new(from).follow_links(false) {
        let entry = entry.map_err(std::io::Error::other)?;
        let rel = entry.path().strip_prefix(from).unwrap_or(entry.path());
        let dest = to.join(rel);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&dest)?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(entry.path(), &dest)?;
        }
    }
    Ok(())
}
