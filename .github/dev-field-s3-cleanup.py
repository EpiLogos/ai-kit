from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]

NEW_FILES = {
    "crates/aikit-core/src/resource/development_field.rs",
    "crates/aikit-cli/build.rs",
    "crates/aikit-cli/src/app/development_field.rs",
    "crates/aikit-cli/tests/development_field.rs",
    "docs/DEVELOPMENT-FIELD-SUBSTRATE.md",
}


def run(*args: str) -> str:
    completed = subprocess.run(args, cwd=ROOT, text=True, capture_output=True, check=True)
    return completed.stdout


def read(path: str) -> str:
    return (ROOT / path).read_text()


def write(path: str, text: str) -> None:
    (ROOT / path).write_text(text)


def replace_once(path: str, old: str, new: str) -> None:
    text = read(path)
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one occurrence, found {count}: {old[:100]!r}")
    write(path, text.replace(old, new, 1))


# Remove the accidental whole-workspace rustfmt diff. The five genuinely new
# files are already formatted and remain; every existing file is restored to
# live main before the narrow integration edits are reapplied.
changed = [line for line in run("git", "diff", "--name-only", "origin/main...HEAD").splitlines() if line]
for path in changed:
    if path in NEW_FILES:
        continue
    exists_on_main = subprocess.run(
        ["git", "cat-file", "-e", f"origin/main:{path}"], cwd=ROOT
    ).returncode == 0
    if exists_on_main:
        subprocess.run(["git", "checkout", "origin/main", "--", path], cwd=ROOT, check=True)
    else:
        candidate = ROOT / path
        if candidate.exists():
            candidate.unlink()

# Core module map only. The typed contract is public through resource::*; no
# unrelated crate-root formatting is necessary.
replace_once(
    "crates/aikit-core/src/resource/mod.rs",
    "mod action_search;\nmod factory;\n",
    "mod action_search;\nmod development_field;\nmod factory;\n",
)
replace_once(
    "crates/aikit-core/src/resource/mod.rs",
    "pub use action_search::search_contextual_actions;\npub use factory::{FactoryInteropView, FactoryResourceImport};\n",
    "pub use action_search::search_contextual_actions;\npub use development_field::*;\npub use factory::{FactoryInteropView, FactoryResourceImport};\n",
)

# Native Git supplies exact current source/worktree evidence; semantic Project
# identity remains the caller's ProjectRef.
replace_once(
    "crates/aikit-adapters/src/native_git.rs",
    "    CreateWorktreeRequest, GitRepositoryRelation, GitWorkingState, GitWorktreeRelation,\n",
    "    CreateWorktreeRequest, DevelopmentFieldCurrentDiff, DevelopmentFieldGitBasis,\n    GitRepositoryRelation, GitWorkingState, GitWorktreeRelation,\n",
)
replace_once(
    "crates/aikit-adapters/src/native_git.rs",
    """    fn worktrees(&self, locator: &str) -> Result<Vec<GitWorktreeRelation>> {\n        let raw = self.checked(locator, [\"worktree\", \"list\", \"--porcelain\"])?;\n        parse_worktrees(&raw)\n    }\n}\n\nimpl VersionedWorldProvider for NativeGitProvider {\n""",
    """    fn worktrees(&self, locator: &str) -> Result<Vec<GitWorktreeRelation>> {\n        let raw = self.checked(locator, [\"worktree\", \"list\", \"--porcelain\"])?;\n        parse_worktrees(&raw)\n    }\n\n    /// Build the exact Git/VersionedWorld portion of a Development Field reading.\n    ///\n    /// The optional base is caller-owned Run/plan evidence. `git diff <base> --`\n    /// compares that base against the current index + working tree, while untracked\n    /// paths remain separately disclosed rather than having their contents silently\n    /// promoted into tracked source.\n    pub fn development_field_basis(\n        &self,\n        project: &ProjectRef,\n        locator: &str,\n        base_revision: Option<VersionRevision>,\n        max_bytes: usize,\n    ) -> Result<DevelopmentFieldGitBasis> {\n        let world = self.inspect(project, locator)?;\n        let current_diff_from_base = match base_revision.as_ref() {\n            None => None,\n            Some(base) => {\n                let args = vec![\n                    \"diff\".to_string(),\n                    \"--no-ext-diff\".to_string(),\n                    \"--binary\".to_string(),\n                    base.as_str().to_string(),\n                    \"--\".to_string(),\n                ];\n                let output = self.output(locator, args)?;\n                if !output.status.success() {\n                    return Err(git_failure(output));\n                }\n                let max = max_bytes.max(1);\n                let truncated = output.stdout.len() > max;\n                let bytes = &output.stdout[..output.stdout.len().min(max)];\n                Some(DevelopmentFieldCurrentDiff {\n                    base_revision: base.clone(),\n                    observed_head: world.repository.head.clone(),\n                    patch: String::from_utf8_lossy(bytes).to_string(),\n                    truncated,\n                    untracked_paths: world.working.untracked.clone(),\n                })\n            }\n        };\n        DevelopmentFieldGitBasis::new(world, base_revision, current_diff_from_base)\n    }\n}\n\nimpl VersionedWorldProvider for NativeGitProvider {\n""",
)
replace_once(
    "crates/aikit-adapters/src/native_git.rs",
    """    fn run<const N: usize>(cwd: &Path, args: [&str; N]) {\n""",
    r'''    #[test]
    fn development_field_basis_includes_current_tracked_difference_and_names_untracked_paths() {
        let provider = NativeGitProvider::new().unwrap();
        if !matches!(provider.descriptor().status, VersionedWorldProviderStatus::Available) {
            return;
        }
        let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!(
            "aikit-development-field-git-{}-{unique}", std::process::id()
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

    fn run<const N: usize>(cwd: &Path, args: [&str; N]) {
''',
)

# Shared application service exposes the one read; CLI is a thin front over it.
replace_once(
    "crates/aikit-cli/src/app/mod.rs",
    "mod flow_cognition;\nmod knowledge;\n\npub use flow_cognition::{\n",
    "mod development_field;\nmod flow_cognition;\nmod knowledge;\n\npub use development_field::DevelopmentFieldApplicationRequest;\npub use flow_cognition::{\n",
)
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
    "fn cmd_routine(command: RoutineCmd) -> Result<Reply> {\n",
    r'''fn cmd_development_field(cwd: &std::path::Path, args: DevelopmentFieldArgs) -> Result<Reply> {
    let service = Service::discover(cwd)?;
    let subjects = args
        .refs
        .into_iter()
        .map(aikit_core::resource::ResourceRef::parse)
        .collect::<Result<Vec<_>>>()?;
    let base_revision = parse_optional_development_field_revision(args.base, "--base")?;
    let expected_aikit_revision = parse_optional_development_field_revision(
        args.expect_aikit_revision,
        "--expect-aikit-revision",
    )?;
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

fn parse_optional_development_field_revision(
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

# Temporary cleanup machinery must not survive the verified commit.
for path in [
    ROOT / ".github/dev-field-s3-cleanup.py",
    ROOT / ".github/workflows/dev-field-s3-cleanup.yml",
]:
    if path.exists():
        path.unlink()
