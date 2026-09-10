from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def read(path: str) -> str:
    return (ROOT / path).read_text()


def write(path: str, text: str) -> None:
    target = ROOT / path
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(text)


def replace_once(path: str, old: str, new: str) -> None:
    text = read(path)
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected exactly one occurrence, found {count}: {old[:120]!r}")
    write(path, text.replace(old, new, 1))


def insert_before_final_brace(path: str, addition: str) -> None:
    text = read(path)
    marker = "\n}\n"
    index = text.rfind(marker)
    if index < 0:
        raise SystemExit(f"{path}: final module brace not found")
    write(path, text[:index] + "\n" + addition.rstrip() + text[index:])


# ---------------------------------------------------------------------------
# Core carrier/read contract refinements
# ---------------------------------------------------------------------------

replace_once(
    "crates/aikit-core/src/resource/development_field.rs",
    """    #[serde(default, skip_serializing_if = \"Option::is_none\")]\n    pub source_revision: Option<VersionRevision>,\n}\n""",
    """    #[serde(default, skip_serializing_if = \"Option::is_none\")]\n    pub source_revision: Option<VersionRevision>,\n    /// True when this executable was built from a checkout whose tracked source\n    /// differed from the named revision. A dirty build may disclose its base\n    /// revision but may not masquerade as that exact revision.\n    #[serde(default)]\n    pub source_dirty: bool,\n}\n""",
)

replace_once(
    "crates/aikit-core/src/resource/development_field.rs",
    """    match development_field_binding(record) {\n        Ok(binding) => DevelopmentFieldSubjectReading {\n            subject,\n            availability: DevelopmentFieldAvailability::available(),\n""",
    """    match development_field_binding(record) {\n        Ok(binding) => DevelopmentFieldSubjectReading {\n            subject,\n            availability: if binding.is_some() {\n                DevelopmentFieldAvailability::available()\n            } else {\n                DevelopmentFieldAvailability::unknown(\n                    \"ResourceRef is present, but no Development Field carrier binding was supplied by its native owner\",\n                )\n            },\n""",
)

replace_once(
    "crates/aikit-core/src/resource/development_field.rs",
    """            modality: DevelopmentFieldExecutableModality::Developer,\n            source_revision: Some(VersionRevision::new(\"abc123\")),\n        }\n""",
    """            modality: DevelopmentFieldExecutableModality::Developer,\n            source_revision: Some(VersionRevision::new(\"abc123\")),\n            source_dirty: false,\n        }\n""",
)

insert_before_final_brace(
    "crates/aikit-core/src/resource/development_field.rs",
    r'''
    #[test]
    fn a_present_resource_without_an_owner_binding_is_unknown_not_a_fabricated_carrier() {
        let mut index = MemoryResourceIndex::default();
        index.insert(ResourceRecord::new(ResourceDescriptor::new(
            ResourceRef::parse("source:ordinary").unwrap(),
            ResourceKind::KnowledgeSource,
            "ordinary source",
            "present in the Resource field without a Development Field declaration",
        )));

        let reading = read_development_field(
            &index,
            &DevelopmentFieldReadRequest {
                subjects: vec![ResourceRef::parse("source:ordinary").unwrap()],
                limit: 8,
            },
            executable(),
            Ok(None),
        );
        assert_eq!(reading.subjects.len(), 1);
        assert_eq!(
            reading.subjects[0].availability.state,
            DevelopmentFieldAvailabilityState::Unknown
        );
        assert!(reading.subjects[0].carrier_kind.is_none());
    }

    #[test]
    fn every_development_field_carrier_kind_remains_addressable_through_the_existing_resolver() {
        let cases = [
            (DevelopmentFieldCarrierKind::SelfDescription, "central:self:probe"),
            (DevelopmentFieldCarrierKind::TierBinding, "central:tier:probe"),
            (DevelopmentFieldCarrierKind::UserExperience, "central:ux:probe"),
            (DevelopmentFieldCarrierKind::ExperienceMetadata, "central:ex:probe"),
            (DevelopmentFieldCarrierKind::Evidence, "evidence:probe"),
            (DevelopmentFieldCarrierKind::Capability, "capability:probe"),
            (DevelopmentFieldCarrierKind::Plan, "factory:plan:probe"),
        ];
        let mut index = MemoryResourceIndex::default();
        for (kind, reference) in cases {
            index.insert(carrier(reference, kind));
        }

        for (_, reference) in cases {
            let expression = crate::resource::ResolveExpression::ordinary_search(reference);
            let path = crate::resource::resolve_expression(&expression, &index, 8);
            assert!(
                path.candidates
                    .iter()
                    .any(|candidate| candidate.resource.as_str() == reference),
                "{reference} must stay in the one ResourceRef-native Search/Resolve field"
            );
        }
    }
''',
)

# Curated crate-root surface: consumers should not need to discover an internal module path.
replace_once(
    "crates/aikit-core/src/lib.rs",
    """pub use resource::{\n    Eligibility, OwnerRef, PreferenceIntent, ProviderOffer, ProviderRef, ProviderState,\n""",
    """pub use resource::{\n    attach_development_field_binding, development_field_binding, read_development_field,\n    DevelopmentFieldAvailability, DevelopmentFieldAvailabilityState, DevelopmentFieldBinding,\n    DevelopmentFieldCarrierKind, DevelopmentFieldCarrierProjection, DevelopmentFieldCurrentDiff,\n    DevelopmentFieldExecutableBasis, DevelopmentFieldExecutableModality, DevelopmentFieldGitBasis,\n    DevelopmentFieldReadRequest, DevelopmentFieldReading, DevelopmentFieldRelation,\n    DevelopmentFieldSelfDescriptionAperture, DevelopmentFieldSubjectReading,\n    QlShapeBindingCarrier, QlShapeMemberBinding, WorkcellMaterialRef,\n    DEVELOPMENT_FIELD_BINDING_ANNOTATION, DEVELOPMENT_FIELD_BINDING_VERSION,\n    DEVELOPMENT_FIELD_READING_VERSION, DEFAULT_DEVELOPMENT_FIELD_READ_LIMIT,\n    MAX_DEVELOPMENT_FIELD_READ_LIMIT,\n    Eligibility, OwnerRef, PreferenceIntent, ProviderOffer, ProviderRef, ProviderState,\n""",
)

# ---------------------------------------------------------------------------
# Exact Git/VersionedWorld basis, including current tracked working difference
# ---------------------------------------------------------------------------

replace_once(
    "crates/aikit-adapters/src/native_git.rs",
    """use aikit_core::resource::{\n    CreateWorktreeRequest, GitRepositoryRelation, GitWorkingState, GitWorktreeRelation,\n""",
    """use aikit_core::resource::{\n    CreateWorktreeRequest, DevelopmentFieldCurrentDiff, DevelopmentFieldGitBasis,\n    GitRepositoryRelation, GitWorkingState, GitWorktreeRelation,\n""",
)

replace_once(
    "crates/aikit-adapters/src/native_git.rs",
    """    fn worktrees(&self, locator: &str) -> Result<Vec<GitWorktreeRelation>> {\n        let raw = self.checked(locator, [\"worktree\", \"list\", \"--porcelain\"])?;\n        parse_worktrees(&raw)\n    }\n}\n\nimpl VersionedWorldProvider for NativeGitProvider {\n""",
    """    fn worktrees(&self, locator: &str) -> Result<Vec<GitWorktreeRelation>> {\n        let raw = self.checked(locator, [\"worktree\", \"list\", \"--porcelain\"])?;\n        parse_worktrees(&raw)\n    }\n\n    /// Build the exact Git/VersionedWorld portion of a Development Field reading.\n    ///\n    /// The optional base is caller-owned Run/plan evidence. `git diff <base> --`\n    /// compares that base against the current index + working tree, while untracked\n    /// paths remain separately disclosed rather than having their contents silently\n    /// promoted into tracked source.\n    pub fn development_field_basis(\n        &self,\n        project: &ProjectRef,\n        locator: &str,\n        base_revision: Option<VersionRevision>,\n        max_bytes: usize,\n    ) -> Result<DevelopmentFieldGitBasis> {\n        let world = self.inspect(project, locator)?;\n        let current_diff_from_base = match base_revision.as_ref() {\n            None => None,\n            Some(base) => {\n                let args = vec![\n                    \"diff\".to_string(),\n                    \"--no-ext-diff\".to_string(),\n                    \"--binary\".to_string(),\n                    base.as_str().to_string(),\n                    \"--\".to_string(),\n                ];\n                let output = self.output(locator, args)?;\n                if !output.status.success() {\n                    return Err(git_failure(output));\n                }\n                let max = max_bytes.max(1);\n                let truncated = output.stdout.len() > max;\n                let bytes = &output.stdout[..output.stdout.len().min(max)];\n                Some(DevelopmentFieldCurrentDiff {\n                    base_revision: base.clone(),\n                    observed_head: world.repository.head.clone(),\n                    patch: String::from_utf8_lossy(bytes).to_string(),\n                    truncated,\n                    untracked_paths: world.working.untracked.clone(),\n                })\n            }\n        };\n        DevelopmentFieldGitBasis::new(world, base_revision, current_diff_from_base)\n    }\n}\n\nimpl VersionedWorldProvider for NativeGitProvider {\n""",
)

insert_before_final_brace(
    "crates/aikit-adapters/src/native_git.rs",
    r'''
    #[test]
    fn development_field_basis_includes_current_tracked_difference_and_names_untracked_paths() {
        let provider = NativeGitProvider::new().unwrap();
        if !matches!(
            provider.descriptor().status,
            VersionedWorldProviderStatus::Available
        ) {
            return;
        }
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "aikit-development-field-git-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        run(&root, ["init", "-q"]);
        run(&root, ["config", "user.name", "AIKit Test"]);
        run(&root, ["config", "user.email", "aikit@example.invalid"]);
        fs::write(root.join("README.md"), "one\n").unwrap();
        run(&root, ["add", "README.md"]);
        run(&root, ["commit", "-qm", "initial"]);

        let project = ProjectRef::parse("project:development-field").unwrap();
        let locator = root.to_string_lossy().to_string();
        let base = provider.inspect(&project, &locator).unwrap().repository.head;
        fs::write(root.join("README.md"), "two\n").unwrap();
        fs::write(root.join("untracked.txt"), "not source until Git says so\n").unwrap();

        let basis = provider
            .development_field_basis(&project, &locator, Some(base.clone()), 64 * 1024)
            .unwrap();
        let diff = basis.current_diff_from_base.unwrap();
        assert_eq!(diff.base_revision, base);
        assert_eq!(diff.observed_head, basis.world.repository.head);
        assert!(diff.patch.contains("+two"), "{}", diff.patch);
        assert_eq!(diff.untracked_paths, vec!["untracked.txt"]);

        let _ = fs::remove_dir_all(&root);
    }
''',
)

# ---------------------------------------------------------------------------
# Shared application read + executable identity
# ---------------------------------------------------------------------------

write(
    "crates/aikit-cli/build.rs",
    r'''use std::env;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=AIKIT_BUILD_SOURCE_REVISION");
    println!("cargo:rerun-if-env-changed=AIKIT_BUILD_SOURCE_DIRTY");

    if let Ok(revision) = env::var("AIKIT_BUILD_SOURCE_REVISION") {
        if !revision.trim().is_empty() {
            println!("cargo:rustc-env=AIKIT_BUILD_SOURCE_REVISION={}", revision.trim());
            let dirty = env::var("AIKIT_BUILD_SOURCE_DIRTY").unwrap_or_else(|_| "0".into());
            println!("cargo:rustc-env=AIKIT_BUILD_SOURCE_DIRTY={dirty}");
            return;
        }
    }

    let manifest = env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default();
    let revision = Command::new("git")
        .arg("-C")
        .arg(&manifest)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|value| !value.is_empty());

    if let Some(revision) = revision {
        println!("cargo:rustc-env=AIKIT_BUILD_SOURCE_REVISION={revision}");
        let dirty = Command::new("git")
            .arg("-C")
            .arg(&manifest)
            .args(["status", "--porcelain", "--untracked-files=no"])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .is_some_and(|output| !output.stdout.is_empty());
        println!(
            "cargo:rustc-env=AIKIT_BUILD_SOURCE_DIRTY={}",
            if dirty { "1" } else { "0" }
        );
    }
}
''',
)

write(
    "crates/aikit-cli/src/app/development_field.rs",
    r'''//! Bounded Development Field application reading.
//!
//! This is an application composition over owner-native Resource records and the
//! existing VersionedWorld provider. It is not a second source store, QL engine,
//! Workcell lifecycle, or intelligence grammar.

use aikit_adapters::native_git::NativeGitProvider;
use aikit_core::resource::{
    read_development_field, DevelopmentFieldExecutableBasis, DevelopmentFieldExecutableModality,
    DevelopmentFieldGitBasis, DevelopmentFieldReadRequest, DevelopmentFieldReading, ResourceRef,
    VersionRevision,
};
use aikit_core::{AikitError, Result};
use aikit_tui::backend::PaletteBackend;

use super::Service;

#[derive(Debug, Clone)]
pub struct DevelopmentFieldApplicationRequest {
    pub subjects: Vec<ResourceRef>,
    pub limit: usize,
    pub base_revision: Option<VersionRevision>,
    pub max_diff_bytes: usize,
    pub expected_aikit_revision: Option<VersionRevision>,
}

impl Default for DevelopmentFieldApplicationRequest {
    fn default() -> Self {
        Self {
            subjects: Vec::new(),
            limit: aikit_core::resource::DEFAULT_DEVELOPMENT_FIELD_READ_LIMIT,
            base_revision: None,
            max_diff_bytes: 256 * 1024,
            expected_aikit_revision: None,
        }
    }
}

impl Service {
    /// Read the current bounded Development Field through the same application
    /// backend used by CLI/TUI composition. Owner records enter through the
    /// canonical Resource field; this operation only composes their declared
    /// relations with exact process and Git basis.
    pub fn development_field_read(
        &self,
        request: DevelopmentFieldApplicationRequest,
    ) -> Result<DevelopmentFieldReading> {
        let records = <Self as PaletteBackend>::context_resource_records(self)?;
        let resources = aikit_tui::project_world_service::resource_index_with_records(self, records)?;
        let executable_basis = current_executable_basis();
        verify_expected_revision(&executable_basis, request.expected_aikit_revision.as_ref())?;
        let git_basis = self.development_field_git_basis(
            request.base_revision.clone(),
            request.max_diff_bytes,
        );
        Ok(read_development_field(
            &resources,
            &DevelopmentFieldReadRequest {
                subjects: request.subjects,
                limit: request.limit,
            },
            executable_basis,
            git_basis,
        ))
    }

    fn development_field_git_basis(
        &self,
        base_revision: Option<VersionRevision>,
        max_diff_bytes: usize,
    ) -> Result<Option<DevelopmentFieldGitBasis>> {
        let Some(root) = self.descriptor.project_root.as_deref() else {
            return Ok(None);
        };
        let Some(binding) = <Self as PaletteBackend>::project_binding(self)? else {
            return Ok(None);
        };
        let provider = NativeGitProvider::new()?;
        match provider.development_field_basis(
            &binding.project,
            &root.to_string_lossy(),
            base_revision,
            max_diff_bytes,
        ) {
            Ok(basis) => Ok(Some(basis)),
            Err(error) if error.code() == "versioned_world.git_failed" => Ok(None),
            Err(error) => Err(error),
        }
    }
}

fn current_executable_basis() -> DevelopmentFieldExecutableBasis {
    let source_revision = option_env!("AIKIT_BUILD_SOURCE_REVISION")
        .filter(|value| !value.trim().is_empty())
        .map(VersionRevision::new);
    let source_dirty = option_env!("AIKIT_BUILD_SOURCE_DIRTY") == Some("1");
    let modality = match source_revision {
        Some(_) if cfg!(debug_assertions) => DevelopmentFieldExecutableModality::Developer,
        Some(_) => DevelopmentFieldExecutableModality::Source,
        None => DevelopmentFieldExecutableModality::Installed,
    };
    DevelopmentFieldExecutableBasis {
        executable: std::env::current_exe()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "<unavailable>".into()),
        package_version: env!("CARGO_PKG_VERSION").into(),
        modality,
        source_revision,
        source_dirty,
    }
}

fn verify_expected_revision(
    basis: &DevelopmentFieldExecutableBasis,
    expected: Option<&VersionRevision>,
) -> Result<()> {
    let Some(expected) = expected else {
        return Ok(());
    };
    let exact = basis
        .source_revision
        .as_ref()
        .is_some_and(|actual| actual == expected)
        && !basis.source_dirty;
    if exact {
        return Ok(());
    }
    Err(AikitError::new(
        "resource.development_field_executable_revision_mismatch",
        "the active AIKit executable does not exactly represent the requested source revision",
    )
    .with("expected_revision", expected.as_str())
    .with(
        "actual_revision",
        basis
            .source_revision
            .as_ref()
            .map(VersionRevision::as_str)
            .unwrap_or("unavailable"),
    )
    .with("source_dirty", basis.source_dirty.to_string())
    .with("executable", basis.executable.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expected_revision_rejects_both_a_different_build_and_a_dirty_matching_build() {
        let expected = VersionRevision::new("abc");
        let mut basis = DevelopmentFieldExecutableBasis {
            executable: "/tmp/aikit".into(),
            package_version: "0.0.0".into(),
            modality: DevelopmentFieldExecutableModality::Source,
            source_revision: Some(VersionRevision::new("def")),
            source_dirty: false,
        };
        assert_eq!(
            verify_expected_revision(&basis, Some(&expected))
                .unwrap_err()
                .code(),
            "resource.development_field_executable_revision_mismatch"
        );
        basis.source_revision = Some(expected.clone());
        basis.source_dirty = true;
        assert!(verify_expected_revision(&basis, Some(&expected)).is_err());
        basis.source_dirty = false;
        assert!(verify_expected_revision(&basis, Some(&expected)).is_ok());
    }
}
''',
)

replace_once(
    "crates/aikit-cli/src/app/mod.rs",
    """mod flow_cognition;\nmod knowledge;\n\npub use flow_cognition::{\n""",
    """mod development_field;\nmod flow_cognition;\nmod knowledge;\n\npub use development_field::DevelopmentFieldApplicationRequest;\npub use flow_cognition::{\n""",
)

# ---------------------------------------------------------------------------
# Native CLI parity
# ---------------------------------------------------------------------------

replace_once(
    "crates/aikit-cli/src/cli.rs",
    """    #[command(visible_alias = \"resolve\")]\n    Search(SearchArgs),\n    /// Navigate provider-neutral project knowledge through the shared application faculty.\n""",
    """    #[command(visible_alias = \"resolve\")]\n    Search(SearchArgs),\n    /// Read the bounded Development Field carrier/provenance/Git substrate.\n    DevelopmentField(DevelopmentFieldArgs),\n    /// Navigate provider-neutral project knowledge through the shared application faculty.\n""",
)

replace_once(
    "crates/aikit-cli/src/cli.rs",
    """/// `aikit gateway serve` — the persistent service carriers.\n#[derive(Debug, Args)]\npub struct GatewayServeArgs {\n""",
    """/// `aikit development-field` — bounded owner-native carrier reading.\n#[derive(Debug, Args)]\npub struct DevelopmentFieldArgs {\n    /// Stable ResourceRefs to read. Omit to read the bounded carrier set already present.\n    #[arg(long = \"ref\", value_name = \"RESOURCE_REF\")]\n    pub refs: Vec<String>,\n    /// Maximum subjects returned (hard-capped by the core contract).\n    #[arg(long, default_value_t = 16)]\n    pub limit: usize,\n    /// Exact caller-supplied Run/plan Git base revision for current-difference disclosure.\n    #[arg(long, value_name = \"REVISION\")]\n    pub base: Option<String>,\n    /// Maximum tracked diff bytes returned when --base is supplied.\n    #[arg(long = \"max-diff-bytes\", default_value_t = 262144)]\n    pub max_diff_bytes: usize,\n    /// Refuse this executable unless it exactly represents this clean source revision.\n    #[arg(long = \"expect-aikit-revision\", value_name = \"REVISION\")]\n    pub expect_aikit_revision: Option<String>,\n}\n\n/// `aikit gateway serve` — the persistent service carriers.\n#[derive(Debug, Args)]\npub struct GatewayServeArgs {\n""",
)

replace_once(
    "crates/aikit-cli/src/main.rs",
    """use aikit_cli::app::{\n    AikitApplication, ApplyRequest, FlowContemplateBasis, PromoteRequest, RunRequest, Service,\n    SessionRequest,\n};\n""",
    """use aikit_cli::app::{\n    AikitApplication, ApplyRequest, DevelopmentFieldApplicationRequest, FlowContemplateBasis,\n    PromoteRequest, RunRequest, Service, SessionRequest,\n};\n""",
)

replace_once(
    "crates/aikit-cli/src/main.rs",
    """        Some(Command::Search(a)) => cmd_search(cwd, a),\n        Some(Command::Knowledge(c)) => cmd_knowledge(cwd, c),\n""",
    """        Some(Command::Search(a)) => cmd_search(cwd, a),\n        Some(Command::DevelopmentField(a)) => cmd_development_field(cwd, a),\n        Some(Command::Knowledge(c)) => cmd_knowledge(cwd, c),\n""",
)

replace_once(
    "crates/aikit-cli/src/main.rs",
    """fn cmd_routine(command: RoutineCmd) -> Result<Reply> {\n""",
    r'''fn cmd_development_field(cwd: &std::path::Path, args: DevelopmentFieldArgs) -> Result<Reply> {
    let service = Service::discover(cwd)?;
    let subjects = args
        .refs
        .into_iter()
        .map(aikit_core::resource::ResourceRef::parse)
        .collect::<Result<Vec<_>>>()?;
    let base_revision = parse_optional_revision(args.base, "--base")?;
    let expected_aikit_revision =
        parse_optional_revision(args.expect_aikit_revision, "--expect-aikit-revision")?;
    let reading = service.development_field_read(DevelopmentFieldApplicationRequest {
        subjects,
        limit: args.limit,
        base_revision,
        max_diff_bytes: args.max_diff_bytes,
        expected_aikit_revision,
    })?;
    let data = serde_json::to_value(reading).map_err(|error| {
        AikitError::new(
            "cli.development_field_encode_failed",
            format!("could not encode Development Field reading: {error}"),
        )
    })?;
    Ok(reply(&service, data, diagnostic_warnings(&service)))
}

fn parse_optional_revision(
    raw: Option<String>,
    argument: &str,
) -> Result<Option<aikit_core::resource::VersionRevision>> {
    raw.map(|value| {
        if value.trim().is_empty() {
            Err(AikitError::new(
                "cli.development_field_revision_empty",
                format!("{argument} requires a non-empty revision"),
            ))
        } else {
            Ok(aikit_core::resource::VersionRevision::new(value))
        }
    })
    .transpose()
}

fn cmd_routine(command: RoutineCmd) -> Result<Reply> {
''',
)

# ---------------------------------------------------------------------------
# Deterministic CLI/application acceptance
# ---------------------------------------------------------------------------

write(
    "crates/aikit-cli/tests/development_field.rs",
    r'''//! Development Field S3 acceptance through the native application/CLI boundary.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, Output};

use aikit_cli::app::{DevelopmentFieldApplicationRequest, Service};
use aikit_core::resource::{
    DevelopmentFieldAvailabilityState, ResourceRef, VersionRevision,
    DEVELOPMENT_FIELD_READING_VERSION,
};
use aikit_store::AikitHome;

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git is available in the test environment");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn project(root: &Path) -> String {
    std::fs::create_dir_all(root.join(".aikit")).unwrap();
    std::fs::write(root.join(".aikit/profile.toml"), "schema = 1\n").unwrap();
    std::fs::create_dir_all(root.join("ProjectCentral")).unwrap();
    std::fs::write(
        root.join("ProjectCentral/project.json"),
        r#"{"schema":"central.project/v1","project_id":"project:probe","human_source":"ProjectCentral/user","wiki":{"profile":"okf-wiki/v1","source":"ProjectCentral/agents/wiki/wiki.json"}}"#,
    )
    .unwrap();
    git(root, &["init", "--initial-branch=trunk"]);
    git(root, &["config", "user.email", "probe@example.invalid"]);
    git(root, &["config", "user.name", "probe"]);
    std::fs::write(root.join("README.md"), "one\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "first"]);
    git(root, &["rev-parse", "HEAD"])
}

fn service(home: &Path, root: &Path) -> Service {
    let mut env = BTreeMap::new();
    env.insert(
        "AIKIT_CONTEXT_ID".to_owned(),
        aikit_core::ContextId::generate().to_string(),
    );
    Service::open(AikitHome::at(home), root, |key| env.get(key).cloned()).unwrap()
}

fn aikit(root: &Path, home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_aikit"))
        .current_dir(root)
        .env("AIKIT_HOME", home)
        .args(args)
        .output()
        .expect("run the native aikit binary")
}

#[test]
fn application_read_composes_existing_resource_field_and_exact_worktree_basis() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("probe");
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&root).unwrap();
    let base = project(&root);
    std::fs::write(root.join("README.md"), "two\n").unwrap();
    std::fs::write(root.join("untracked.txt"), "not yet Git source\n").unwrap();

    let service = service(&home, &root);
    let reading = service
        .development_field_read(DevelopmentFieldApplicationRequest {
            subjects: vec![ResourceRef::parse("source:missing").unwrap()],
            base_revision: Some(VersionRevision::new(base)),
            ..DevelopmentFieldApplicationRequest::default()
        })
        .unwrap();

    assert_eq!(reading.version, DEVELOPMENT_FIELD_READING_VERSION);
    assert_eq!(
        reading.subjects[0].availability.state,
        DevelopmentFieldAvailabilityState::Unknown
    );
    assert_eq!(
        reading.central_self_description.availability.state,
        DevelopmentFieldAvailabilityState::Unknown,
        "Central S1 has not supplied a public self carrier here; AIKit must not infer a path"
    );
    let git = reading.git.expect("a real worktree supplies exact Git basis");
    let diff = git
        .current_diff_from_base
        .expect("an explicit base requests current difference");
    assert!(diff.patch.contains("+two"), "{}", diff.patch);
    assert_eq!(diff.untracked_paths, vec!["untracked.txt"]);
}

#[test]
fn native_cli_returns_the_same_bounded_packet_and_rejects_a_stale_expected_binary() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("probe");
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&root).unwrap();
    let base = project(&root);
    std::fs::write(root.join("README.md"), "two\n").unwrap();
    std::fs::write(root.join("untracked.txt"), "not yet Git source\n").unwrap();

    let output = aikit(
        &root,
        &home,
        &[
            "--json",
            "development-field",
            "--ref",
            "source:missing",
            "--base",
            &base,
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        envelope.pointer("/data/version").and_then(serde_json::Value::as_str),
        Some(DEVELOPMENT_FIELD_READING_VERSION)
    );
    assert_eq!(
        envelope
            .pointer("/data/git_basis/state")
            .and_then(serde_json::Value::as_str),
        Some("available")
    );
    assert!(envelope
        .pointer("/data/git/current_diff_from_base/patch")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|patch| patch.contains("+two")));
    assert!(envelope
        .pointer("/data/executable_basis/package_version")
        .and_then(serde_json::Value::as_str)
        .is_some());

    let stale = aikit(
        &root,
        &home,
        &[
            "--json",
            "development-field",
            "--expect-aikit-revision",
            "definitely-not-this-build",
        ],
    );
    assert!(!stale.status.success());
    assert!(
        String::from_utf8_lossy(&stale.stdout)
            .contains("resource.development_field_executable_revision_mismatch"),
        "{}",
        String::from_utf8_lossy(&stale.stdout)
    );
}
''',
)

# ---------------------------------------------------------------------------
# Documentation: what exists, what remains owner-deferred
# ---------------------------------------------------------------------------

write(
    "docs/DEVELOPMENT-FIELD-SUBSTRATE.md",
    r'''# Development Field operative substrate

AIKit's Development Field surface is a bounded **read/composition contract** over the resources that their native owners have already supplied. It does not make AIKit the owner of Central source identity, QL form meaning, Factory developmental meaning, Workcell lifecycle, or Actuation actuality.

The core contract is `aikit.development-field-reading/v1`. A carrier remains an ordinary `ResourceRef` and therefore stays in the existing Search/Resolve field. Its optional `aikit.development-field-binding/v1` annotation adds only owner-declared carrier kind, explicit stable-ref relations, an attributable QL `ShapeBinding` carrier when supplied, and Workcell material references. A QL shape address is structural addressability, not a semantic Wiki edge, and partial/developed shapes are valid inputs.

## Native read

```sh
aikit --json development-field \
  --ref central:self:project \
  --ref factory:plan:42 \
  --base <exact-run-or-plan-git-revision>
```

The same application operation is `Service::development_field_read(DevelopmentFieldApplicationRequest)`. It returns owner/source/revision provenance, explicit linked refs, optional QL binding and Workcell material refs, plus the current `VersionedWorld` observation. When `--base` is supplied, the packet includes the bounded tracked difference from that revision to the current index/worktree and names untracked paths separately.

Unknown and unavailable states are first-class. In particular, until Central publishes the S1 self-description/tier binding contract into AIKit's Resource field, the `central_self_description` aperture reports `unknown`; AIKit does **not** interpret `ProjectCentral/self/**` paths as a semantic API.

## Active executable identity

Every packet reports the executable path, package version, source/developer/installed modality, and the build's source revision when that evidence existed. A build from modified tracked source sets `source_dirty: true`, so its base commit cannot be mistaken for the exact executable source.

O:I suite dispatch can pin the expected AIKit source revision:

```sh
aikit --json development-field --expect-aikit-revision <sha>
```

The command refuses an absent, different, or dirty source basis with `resource.development_field_executable_revision_mismatch`. This is the parity check that prevents a stale registered binary from silently representing current AIKit behaviour.

## Deliberate boundary

This substrate preserves the already-landed `@# - + x / =` and `@0..@5` operative Search/Resolve syntax unchanged. It does not implement the QL-MEF #123 full Vāk/C′/Ta-Onta reconciliation, Context-Frame intelligence propagation, a Development Field REPL, generated QL contemplation, familiarity routes, or a new Wiki graph. Those later layers can attach to these stable `ResourceRef`, provenance, shape-binding and Git seams without another ownership migration.
''',
)

# The temporary patch harness is not product state.
for temporary in [
    ROOT / ".github/dev-field-s3-patch.py",
    ROOT / ".github/workflows/dev-field-s3-patch.yml",
]:
    if temporary.exists():
        temporary.unlink()
