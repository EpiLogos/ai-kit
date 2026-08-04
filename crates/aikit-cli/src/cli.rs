//! The clap tree.
//!
//! The whole command surface lives here as data, parsed once in `main` and never
//! re-parsed. Two rules from the architecture are encoded structurally rather
//! than left to prose:
//!
//! * `--json` is a **global** flag, so every substantive command accepts it and
//!   no command can forget to. The palette-only commands (`ui`, and the bare
//!   `aikit`) simply ignore it.
//! * `task spawn` defaults to a **shared** working tree. `--worktree` is the only
//!   flag that asks AIKit to cut a git worktree, and it conflicts with the other
//!   isolation flags so the choice is always unambiguous. See
//!   [`TaskSpawnArgs::isolation`].

use clap::{Args, Parser, Subcommand};

/// The three ways a task context can relate to the session's working tree.
/// Re-exported from core so the CLI and the resolver name the same thing.
pub use aikit_core::context::Isolation;

/// `aikit` — a context-scoped capability router for agentic terminal work.
#[derive(Debug, Parser)]
#[command(name = "aikit", version, about, disable_help_subcommand = true)]
pub struct Cli {
    /// Emit machine-readable JSON on stdout instead of human text.
    #[arg(long, global = true)]
    pub json: bool,

    /// Resolve as if in this directory rather than the current one.
    #[arg(long = "cwd", short = 'C', global = true, value_name = "DIR")]
    pub cwd: Option<std::path::PathBuf>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Manage pinned Git and machine-local Agent Skill sources.
    Source(SourceCmd),
    /// Author scoped, additive guidance for Agent Skills.
    Skill(SkillCmd),
    /// Bind directories and repositories to reusable project skill sets.
    Project(ProjectCmd),
    /// Discover the foreign skill roots already on this machine and show them.
    Init(InitArgs),
    /// Survey the skill trees on this machine: which version is running, where.
    Collate(CollateArgs),
    /// Move a foreign skill root into AIKit ownership through a reversible Procedure.
    Adopt(AdoptArgs),
    /// Inspect and undo recorded Procedures.
    Procedure(ProcedureCmd),
    /// Create and inspect project-specific profile lenses.
    Profile(ProfileCmd),
    /// Jump to what you meant: act if unambiguous, else offer the candidates.
    Z(ZArgs),
    /// Create, inspect and point harnesses at skill-sets.
    Set(SetCmd),
    /// The tree: organise sets, see the resolved hook chain, inspect registries.
    Tree(TreeArgs),
    /// Open the palette (the default when no subcommand is given).
    Ui(UiArgs),
    /// Search the catalogue for capabilities.
    Search(SearchArgs),
    /// Show the effective view for the current context.
    Status(StatusArgs),
    /// Explain why a capability is or is not active.
    Explain(ExplainArgs),
    /// Show what applying the current declarations would change.
    Diff(DiffArgs),
    /// Run the health checks.
    Doctor(DoctorArgs),
    /// Run an exported capability once.
    Run(RunArgs),
    /// Enable a capability in a scope.
    Enable(ToggleArgs),
    /// Disable a capability in a scope.
    Disable(ToggleArgs),
    /// Apply a profile to a scope.
    Use(UseArgs),
    /// Materialise the current declarations into a new generation.
    Apply(ApplyArgs),
    /// Return the previous generation.
    Rollback(RollbackArgs),
    /// Inspect and change context bindings.
    Context(ContextCmd),
    /// Bring up, attach to and reconcile session topologies.
    Session(SessionCmd),
    /// Spawn, list and close agent tasks.
    Task(TaskCmd),
    /// Show the capture inbox.
    Inbox(InboxArgs),
    /// Capture text or a command into the inbox.
    Capture(CaptureArgs),
    /// Promote a captured candidate into a capsule.
    Promote(PromoteArgs),
    /// Garbage-collect old generations.
    Prune(PruneArgs),
    /// Issue, list and revoke hook bypass tokens.
    Bypass(BypassCmd),
    /// Install, launch and inspect agent clients.
    Client(ClientCmd),
    /// Install multiplexer integration and detect the current stack.
    Mux(MuxCmd),
    /// The hook dispatcher entry point (invoked by clients, not usually by hand).
    Hook(HookCmd),
    /// List and read the capabilities exposed to a brokered client.
    Capabilities(CapabilitiesCmd),
    /// List tracked background jobs.
    Jobs(JobsArgs),
    /// List recently run invocations.
    Recent(RecentArgs),
    /// Show usage statistics.
    Stats(StatsArgs),
    /// Export the event log.
    Log(LogCmd),
    /// Print shell integration to be sourced from an rc file.
    Shell(ShellCmd),
    /// List catalogued-but-never-used capabilities.
    Unused(UnusedArgs),
    /// List recent hook and run failures.
    Failures(FailuresArgs),
    /// List bypasses issued and spent.
    Bypasses(BypassesArgs),
}

#[derive(Debug, Args)]
pub struct SkillCmd {
    #[command(subcommand)]
    pub command: SkillSub,
}

#[derive(Debug, Subcommand)]
pub enum SkillSub {
    /// Manage additive Skill Usage Overlays.
    Overlay(SkillOverlayCmd),
}

#[derive(Debug, Args)]
pub struct SkillOverlayCmd {
    #[command(subcommand)]
    pub command: SkillOverlaySub,
}

#[derive(Debug, Subcommand)]
pub enum SkillOverlaySub {
    /// Replace this scope's orienting augmentation for a skill.
    Set(SkillOverlaySetArgs),
    /// Show the effective ordered augmentations for a skill.
    Show(SkillOverlayShowArgs),
    /// Remove this scope's augmentation for a skill.
    Clear(SkillOverlayClearArgs),
}

#[derive(Debug, Args)]
pub struct SkillOverlaySetArgs {
    #[arg(value_name = "CAPABILITY")]
    pub capability: String,
    #[arg(long, value_name = "SCOPE")]
    pub scope: Option<String>,
    /// Additional routing text appended to the skill's description.
    #[arg(long, value_name = "TEXT")]
    pub description: Option<String>,
    /// User-authoritative contextual instructions appended to the skill body.
    #[arg(long, value_name = "TEXT", conflicts_with = "guidance_file")]
    pub guidance: Option<String>,
    /// Read contextual instructions from a UTF-8 Markdown file.
    #[arg(long, value_name = "FILE", conflicts_with = "guidance")]
    pub guidance_file: Option<std::path::PathBuf>,
    /// Start from the upstream skill rather than inheriting lower-scope overlays.
    #[arg(long)]
    pub no_inherit: bool,
    /// Source revision against which this augmentation was reviewed.
    #[arg(long, value_name = "REVISION")]
    pub reviewed_against: Option<String>,
}

#[derive(Debug, Args)]
pub struct SkillOverlayShowArgs {
    #[arg(value_name = "CAPABILITY")]
    pub capability: String,
}

#[derive(Debug, Args)]
pub struct SkillOverlayClearArgs {
    #[arg(value_name = "CAPABILITY")]
    pub capability: String,
    #[arg(long, value_name = "SCOPE")]
    pub scope: Option<String>,
}

#[derive(Debug, Args)]
pub struct ProjectCmd {
    #[command(subcommand)]
    pub command: ProjectSub,
}

#[derive(Debug, Subcommand)]
pub enum ProjectSub {
    /// Create or replace a reusable Project Specification.
    Bind(ProjectBindArgs),
    /// Show the Project Specification matching the current directory.
    Show(ProjectShowArgs),
    /// Configure the Skill Sets inherited by Project Specifications by default.
    Defaults(ProjectDefaultsArgs),
}

#[derive(Debug, Args)]
pub struct ProjectBindArgs {
    #[arg(value_name = "ID")]
    pub id: String,
    #[arg(long = "directory", value_name = "DIR")]
    pub directories: Vec<std::path::PathBuf>,
    #[arg(long = "repository", value_name = "IDENTITY")]
    pub repositories: Vec<String>,
    #[arg(long = "set", value_name = "SKILL_SET")]
    pub skill_sets: Vec<String>,
    #[arg(long)]
    pub no_default_skill_sets: bool,
}

#[derive(Debug, Args)]
pub struct ProjectShowArgs {}

#[derive(Debug, Args)]
pub struct ProjectDefaultsArgs {
    #[arg(long = "set", value_name = "SKILL_SET", required = true)]
    pub skill_sets: Vec<String>,
}

#[derive(Debug, Args)]
pub struct SourceCmd {
    #[command(subcommand)]
    pub command: SourceSub,
}

#[derive(Debug, Subcommand)]
pub enum SourceSub {
    /// Register a machine-local skill directory without making it active.
    AddDirectory(SourceAddDirectoryArgs),
    /// Register a Git repository and exact revision without fetching it yet.
    AddGit(SourceAddGitArgs),
    /// Move an existing Git source to a new exact revision without syncing it.
    SetRevision(SourceSetRevisionArgs),
    /// Copy the source into a new immutable candidate snapshot.
    Sync(SourceNameArgs),
    /// Inspect source, candidate, active and rollback state.
    Show(SourceNameArgs),
    /// Promote the candidate snapshot; trust requires an explicit flag.
    Promote(SourcePromoteArgs),
    /// Return to the previous promoted snapshot.
    Rollback(SourceNameArgs),
}

#[derive(Debug, Args)]
pub struct SourceAddDirectoryArgs {
    #[arg(value_name = "ID")]
    pub id: String,
    #[arg(value_name = "DIR")]
    pub directory: std::path::PathBuf,
}

#[derive(Debug, Args)]
pub struct SourceAddGitArgs {
    #[arg(value_name = "ID")]
    pub id: String,
    #[arg(value_name = "REPOSITORY")]
    pub repository: String,
    #[arg(long, value_name = "REVISION")]
    pub revision: String,
    #[arg(long, default_value = ".", value_name = "DIR")]
    pub root: std::path::PathBuf,
}

#[derive(Debug, Args)]
pub struct SourceSetRevisionArgs {
    #[arg(value_name = "ID")]
    pub id: String,
    #[arg(value_name = "REVISION")]
    pub revision: String,
}

#[derive(Debug, Args)]
pub struct SourceNameArgs {
    #[arg(value_name = "ID")]
    pub id: String,
}

#[derive(Debug, Args)]
pub struct SourcePromoteArgs {
    #[arg(value_name = "ID")]
    pub id: String,
    /// Record explicit per-revision trust for every skill in this snapshot.
    #[arg(long)]
    pub trust: bool,
    /// Record trust for one selected skill revision. Repeat for more skills.
    #[arg(long = "trust-skill", value_name = "CAPSULE", conflicts_with = "trust")]
    pub trust_skills: Vec<String>,
}

// ---------------------------------------------------------------------------
// Leaf command arguments
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct AdoptArgs {
    /// Foreign Agent Skills root to adopt.
    #[arg(value_name = "ROOT")]
    pub root: std::path::PathBuf,
    /// Capsule namespace under `skill/` (for example `claude`).
    #[arg(long, value_name = "NAME")]
    pub namespace: Option<String>,
    /// Apply the reviewed plan. Without this flag adoption only prints its diff.
    #[arg(long)]
    pub yes: bool,
    /// Digest printed by the preview. Binds confirmation to the exact surveyed
    /// source bytes, paths, links and modes.
    #[arg(long, value_name = "DIGEST", requires = "yes")]
    pub expect_digest: Option<String>,
}

#[derive(Debug, Args)]
pub struct ProcedureCmd {
    #[command(subcommand)]
    pub command: ProcedureSub,
}

#[derive(Debug, Subcommand)]
pub enum ProcedureSub {
    /// Create and persist a reviewable Procedure without applying it.
    Plan(ProcedurePlanArgs),
    /// Render the durable before/after diff for a planned Procedure.
    Diff(ProcedureDiffArgs),
    /// Apply one exact persisted Procedure after checking its digest.
    Run(ProcedureRunArgs),
    /// Undo a committed Procedure using its recorded inverse journal.
    Undo(ProcedureUndoArgs),
    /// List Procedures that have an undo record.
    List(ProcedureListArgs),
}

#[derive(Debug, Args)]
pub struct ProcedureUndoArgs {
    #[arg(value_name = "PROCEDURE")]
    pub procedure: String,
}

#[derive(Debug, Args)]
pub struct ProcedureListArgs {}

#[derive(Debug, Args)]
pub struct ProcedurePlanArgs {
    #[command(subcommand)]
    pub command: ProcedurePlanSub,
}

#[derive(Debug, Subcommand)]
pub enum ProcedurePlanSub {
    /// Plan adoption of a foreign Agent Skills root.
    Adopt(ProcedurePlanAdoptArgs),
    /// Plan a project-local profile fork.
    ProfileFork(ProcedurePlanProfileForkArgs),
}

#[derive(Debug, Args)]
pub struct ProcedurePlanAdoptArgs {
    #[arg(value_name = "ROOT")]
    pub root: std::path::PathBuf,
    #[arg(long, value_name = "NAME")]
    pub namespace: Option<String>,
}

#[derive(Debug, Args)]
pub struct ProcedurePlanProfileForkArgs {
    #[arg(value_name = "BASE")]
    pub base: String,
    #[arg(long, value_name = "PROFILE")]
    pub name: Option<String>,
    #[arg(long, default_value = "project", value_name = "SCOPE")]
    pub scope: String,
    #[arg(long = "param", value_name = "KEY=VALUE")]
    pub params: Vec<String>,
}

#[derive(Debug, Args)]
pub struct ProcedureDiffArgs {
    #[arg(value_name = "PROCEDURE")]
    pub procedure: String,
}

#[derive(Debug, Args)]
pub struct ProcedureRunArgs {
    #[arg(value_name = "PROCEDURE")]
    pub procedure: String,
    /// Digest printed by `procedure plan` or `procedure diff`.
    #[arg(long, value_name = "DIGEST")]
    pub expect_digest: String,
}

#[derive(Debug, Args)]
pub struct ProfileCmd {
    #[command(subcommand)]
    pub command: ProfileSub,
}

#[derive(Debug, Subcommand)]
pub enum ProfileSub {
    /// Create a project-local delta that extends a base profile.
    Fork(ProfileForkArgs),
    /// Show only what a project fork changes relative to its base.
    Diff(ProfileDiffArgs),
}

#[derive(Debug, Args)]
pub struct ProfileForkArgs {
    #[arg(value_name = "BASE")]
    pub base: String,
    /// Id for the project-local fork. Defaults to `profile/project/<base-name>`.
    #[arg(long, value_name = "PROFILE")]
    pub name: Option<String>,
    /// Forks are project lenses; `project` is currently the only writable scope.
    #[arg(long, default_value = "project", value_name = "SCOPE")]
    pub scope: String,
    /// Bind a parameter required by the base profile. Repeat for multiple values.
    #[arg(long = "param", value_name = "KEY=VALUE")]
    pub params: Vec<String>,
    /// Apply the reviewed plan. Without this flag only the diff is returned.
    #[arg(long)]
    pub yes: bool,
    /// Review digest printed by the preview. Required together with `--yes`.
    #[arg(long, value_name = "DIGEST", requires = "yes")]
    pub expect_digest: Option<String>,
}

#[derive(Debug, Args)]
pub struct ProfileDiffArgs {
    #[arg(value_name = "PROFILE")]
    pub profile: String,
}

#[derive(Debug, Args)]
pub struct TreeArgs {
    /// Expand these paths, e.g. `sets` or `hooks/PreToolUse`. Repeatable.
    #[arg(long = "expand", value_name = "PATH")]
    pub expand: Vec<String>,
    /// Expand every root one level.
    #[arg(long)]
    pub all: bool,
    /// Force the ASCII rendering, whatever the terminal claims.
    #[arg(long)]
    pub ascii: bool,
    /// Only rows matching this text, with their ancestors kept.
    #[arg(long, value_name = "TEXT")]
    pub filter: Option<String>,
}

#[derive(Debug, Args)]
pub struct SetCmd {
    #[command(subcommand)]
    pub command: SetSub,
}

#[derive(Debug, Subcommand)]
pub enum SetSub {
    /// List sets, their membership counts and where they project.
    List(SetListArgs),
    /// Show a set's members — and the members that would NOT project here, with
    /// the reason. A set is a request; this is the reply.
    Show(SetShowArgs),
    /// Create a set. `mkdir` is a legitimate alternative.
    Create(SetCreateArgs),
    /// Add capabilities to a set.
    Add(SetMemberArgs),
    /// Remove capabilities from a set. Never deletes the capability.
    Remove(SetMemberArgs),
    /// Rename a writable set through a reversible Procedure.
    Rename(SetRenameArgs),
    /// Move a writable set into Procedure-owned recovery storage.
    Delete(SetDeleteArgs),
}

#[derive(Debug, Args)]
pub struct SetListArgs {}

#[derive(Debug, Args)]
pub struct SetShowArgs {
    #[arg(value_name = "NAME")]
    pub name: String,
}

#[derive(Debug, Args)]
pub struct SetCreateArgs {
    #[arg(value_name = "NAME")]
    pub name: String,
    /// Capability ids to start with.
    #[arg(value_name = "IDS")]
    pub ids: Vec<String>,
    /// Globs to expand NOW into explicit ids. The pattern is retained as
    /// provenance; it never matches dynamically later.
    #[arg(long = "match", value_name = "GLOB")]
    pub globs: Vec<String>,
}

#[derive(Debug, Args)]
pub struct SetMemberArgs {
    #[arg(value_name = "NAME")]
    pub name: String,
    #[arg(value_name = "IDS", required = true)]
    pub ids: Vec<String>,
}

#[derive(Debug, Args)]
pub struct SetRenameArgs {
    #[arg(value_name = "FROM")]
    pub from: String,
    #[arg(value_name = "TO")]
    pub to: String,
}

#[derive(Debug, Args)]
pub struct SetDeleteArgs {
    #[arg(value_name = "NAME")]
    pub name: String,
}

#[derive(Debug, Args)]
pub struct ZArgs {
    /// The words you remember. Matched against ids and exported command names.
    #[arg(value_name = "WORDS", required = true)]
    pub words: Vec<String>,
    /// Report what would happen without doing it. Implied by `--json`.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Args)]
pub struct CollateArgs {
    /// An additional skill root to survey, beyond the well-known ones. Repeatable.
    #[arg(long = "root", value_name = "DIR")]
    pub roots: Vec<std::path::PathBuf>,
    /// Show every name, not only the ones needing a decision.
    #[arg(long)]
    pub all: bool,
}

#[derive(Debug, Args)]
pub struct InitArgs {
    /// An additional foreign skill root to index, beyond the well-known ones.
    /// Repeatable. Discovery is always read-only.
    #[arg(long = "root", value_name = "DIR")]
    pub roots: Vec<std::path::PathBuf>,
}

#[derive(Debug, Args)]
pub struct UiArgs {
    /// Force the fullscreen host even when a popup would fit.
    #[arg(long)]
    pub fullscreen: bool,
    /// Open the organising tree instead of the invocation palette.
    #[arg(long, conflicts_with = "query")]
    pub tree: bool,
    /// Seed the palette's search box.
    #[arg(value_name = "QUERY")]
    pub query: Option<String>,
}

#[derive(Debug, Args)]
pub struct SearchArgs {
    /// The query, in the palette's search grammar.
    #[arg(value_name = "QUERY", default_value = "")]
    pub query: String,
    /// Cap the number of rows returned.
    #[arg(long, default_value_t = 50)]
    pub limit: usize,
}

#[derive(Debug, Args)]
pub struct StatusArgs {
    /// Include catalogued-but-inactive capabilities.
    #[arg(long)]
    pub all: bool,
}

#[derive(Debug, Args)]
pub struct ExplainArgs {
    /// The capability id, e.g. `skill/rust/code-review`.
    #[arg(value_name = "CAPABILITY")]
    pub capability: String,
}

#[derive(Debug, Args)]
pub struct DiffArgs {}

#[derive(Debug, Args)]
pub struct DoctorArgs {
    /// Propose and, after confirmation, apply fixes.
    #[arg(long)]
    pub fix: bool,
    /// Answer the confirmation prompt yes in advance (non-interactive fix).
    #[arg(long)]
    pub yes: bool,
}

#[derive(Debug, Args)]
pub struct RunArgs {
    /// The exported command name or capability id to run.
    #[arg(value_name = "NAME")]
    pub name: String,
    /// Arguments passed through to the capability.
    #[arg(
        value_name = "ARGS",
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    pub args: Vec<String>,
    /// Override the execution mode.
    #[arg(long, value_name = "MODE")]
    pub mode: Option<String>,
    /// Confirm running an executable whose revision has not been reviewed.
    #[arg(long)]
    pub confirm: bool,
}

#[derive(Debug, Args)]
pub struct ToggleArgs {
    /// The capability id to enable or disable.
    #[arg(value_name = "CAPABILITY")]
    pub capability: String,
    /// Write to this scope rather than the context's default mutation scope.
    #[arg(long, value_name = "SCOPE")]
    pub scope: Option<String>,
    /// Apply immediately rather than only recording the declaration.
    #[arg(long)]
    pub apply: bool,
}

#[derive(Debug, Args)]
pub struct UseArgs {
    /// The profile id to apply.
    #[arg(value_name = "PROFILE")]
    pub profile: String,
    /// Write to this scope rather than the context's default mutation scope.
    #[arg(long, value_name = "SCOPE")]
    pub scope: Option<String>,
}

#[derive(Debug, Args)]
pub struct ApplyArgs {
    /// Attach a cosmetic label to the generation this apply produces (e.g.
    /// `known-good`). Labels are excluded from the generation's identity, so
    /// labelling an unchanged view updates the label in place rather than minting
    /// a new generation.
    #[arg(long, value_name = "TEXT")]
    pub label: Option<String>,
}

#[derive(Debug, Args)]
pub struct RollbackArgs {}

#[derive(Debug, Args)]
pub struct InboxArgs {
    /// Include quarantined and rejected candidates.
    #[arg(long)]
    pub all: bool,
}

#[derive(Debug, Args)]
pub struct CaptureArgs {
    /// A short title for the capture.
    #[arg(value_name = "TITLE")]
    pub title: String,
    /// The body; if omitted, read from stdin.
    #[arg(long, value_name = "TEXT")]
    pub body: Option<String>,
}

#[derive(Debug, Args)]
pub struct PromoteArgs {
    /// The candidate id to promote.
    #[arg(value_name = "CANDIDATE")]
    pub candidate: String,
    /// The capsule id to give the new capability.
    #[arg(long, value_name = "ID")]
    pub id: Option<String>,
}

#[derive(Debug, Args)]
pub struct PruneArgs {
    /// Number of generations to keep per context.
    #[arg(long, default_value_t = 5)]
    pub keep: usize,
}

#[derive(Debug, Args)]
pub struct JobsArgs {}

#[derive(Debug, Args)]
pub struct RecentArgs {
    #[arg(long, default_value_t = 20)]
    pub limit: u32,
}

#[derive(Debug, Args)]
pub struct StatsArgs {}

#[derive(Debug, Args)]
pub struct UnusedArgs {}

#[derive(Debug, Args)]
pub struct FailuresArgs {
    #[arg(long, default_value_t = 20)]
    pub limit: u32,
}

#[derive(Debug, Args)]
pub struct BypassesArgs {}

// ---------------------------------------------------------------------------
// context
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct ContextCmd {
    #[command(subcommand)]
    pub command: ContextSub,
}

#[derive(Debug, Subcommand)]
pub enum ContextSub {
    /// Show the current context and its binding.
    Current(ContextCurrentArgs),
    /// List known contexts.
    List(ContextListArgs),
    /// Bind the current context to a multiplexer location.
    Bind(ContextBindArgs),
    /// Forget the binding for the current context.
    Reset(ContextResetArgs),
    /// Print this context's environment as shell `export` lines.
    ///
    /// The shell integration evals this on directory change; it is what makes a
    /// per-context `BKMR_DB_URL` (and anything like it) real.
    Env(ContextEnvArgs),
}

#[derive(Debug, Args)]
pub struct ContextCurrentArgs {}
#[derive(Debug, Args)]
pub struct ContextListArgs {}
#[derive(Debug, Args)]
pub struct ContextBindArgs {
    #[arg(long, value_name = "SESSION")]
    pub session: Option<String>,
}
#[derive(Debug, Args)]
pub struct ContextResetArgs {}
#[derive(Debug, Args)]
pub struct ContextEnvArgs {
    /// The shell whose syntax to emit: bash, zsh, fish or sh.
    #[arg(long, default_value = "bash", value_name = "SHELL")]
    pub shell: String,
}

// ---------------------------------------------------------------------------
// session
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct SessionCmd {
    #[command(subcommand)]
    pub command: SessionSub,
}

#[derive(Debug, Subcommand)]
pub enum SessionSub {
    /// Bring up a session topology idempotently.
    Up(SessionUpArgs),
    /// Attach to a running session.
    Attach(SessionAttachArgs),
    /// List sessions.
    List(SessionListArgs),
    /// Diff a running session against its spec.
    Diff(SessionDiffArgs),
    /// Reconcile a running session towards its spec.
    Reconcile(SessionReconcileArgs),
    /// Tear down a session.
    Down(SessionDownArgs),
}

#[derive(Debug, Args)]
pub struct SessionUpArgs {
    /// The session capsule or spec to bring up.
    #[arg(value_name = "SPEC")]
    pub spec: Option<String>,
}
#[derive(Debug, Args)]
pub struct SessionAttachArgs {
    #[arg(value_name = "SESSION")]
    pub session: String,
}
#[derive(Debug, Args)]
pub struct SessionListArgs {}
#[derive(Debug, Args)]
pub struct SessionDiffArgs {
    #[arg(value_name = "SESSION")]
    pub session: Option<String>,
}
#[derive(Debug, Args)]
pub struct SessionReconcileArgs {
    #[arg(value_name = "SESSION")]
    pub session: Option<String>,
    /// Allow reconciliation to close panes that are not in the spec.
    #[arg(long)]
    pub destructive: bool,
}
#[derive(Debug, Args)]
pub struct SessionDownArgs {
    #[arg(value_name = "SESSION")]
    pub session: String,
}

// ---------------------------------------------------------------------------
// task
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct TaskCmd {
    #[command(subcommand)]
    pub command: TaskSub,
}

#[derive(Debug, Subcommand)]
pub enum TaskSub {
    /// Spawn an agent task in the current session.
    Spawn(TaskSpawnArgs),
    /// List tasks.
    List(TaskListArgs),
    /// Close a task, refusing to discard an unclean worktree without `--force`.
    Close(TaskCloseArgs),
}

#[derive(Debug, Args)]
pub struct TaskSpawnArgs {
    /// A short name for the task.
    #[arg(value_name = "NAME")]
    pub name: String,
    /// The agent client to launch.
    #[arg(long, default_value = "claude")]
    pub agent: String,
    /// Give the task its own git worktree and branch (opt-in).
    #[arg(long, conflicts_with_all = ["directory", "shared"])]
    pub worktree: bool,
    /// Give the task a dedicated directory that is not a git worktree.
    #[arg(long, conflicts_with_all = ["worktree", "shared"])]
    pub directory: bool,
    /// Use the session's working tree as-is (the default; explicit form).
    #[arg(long, conflicts_with_all = ["worktree", "directory"])]
    pub shared: bool,
}

impl TaskSpawnArgs {
    /// The isolation the flags select.
    ///
    /// Shared is the default and remains the default when nothing is asked for:
    /// a worktree is cut only when `--worktree` is given, precisely because that
    /// is the choice that costs a checkout, a branch and a teardown decision.
    pub fn isolation(&self) -> Isolation {
        if self.worktree {
            Isolation::Worktree
        } else if self.directory {
            Isolation::Directory
        } else {
            Isolation::Shared
        }
    }
}

#[derive(Debug, Args)]
pub struct TaskListArgs {}

#[derive(Debug, Args)]
pub struct TaskCloseArgs {
    /// The task name to close.
    #[arg(value_name = "NAME")]
    pub name: String,
    /// Discard the task even if its worktree is dirty or has unpushed work.
    #[arg(long)]
    pub force: bool,
}

// ---------------------------------------------------------------------------
// bypass
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct BypassCmd {
    #[command(subcommand)]
    pub command: BypassSub,
}

#[derive(Debug, Subcommand)]
pub enum BypassSub {
    /// Mint a short-lived scoped bypass token.
    Issue(BypassIssueArgs),
    /// List open bypass tokens.
    List(BypassListArgs),
    /// Revoke an open bypass token.
    Revoke(BypassRevokeArgs),
}

#[derive(Debug, Args)]
pub struct BypassIssueArgs {
    /// The bypass scope: `next-event` (default), `session`, or a duration.
    #[arg(long, default_value = "next-event", value_name = "SCOPE")]
    pub scope: String,
    /// Why the bypass is being issued. Recorded and shown in `status`.
    #[arg(long, value_name = "REASON")]
    pub reason: Option<String>,
    /// Restrict the bypass to a single capsule rather than the whole chain.
    #[arg(long, value_name = "CAPABILITY")]
    pub capability: Option<String>,
}

#[derive(Debug, Args)]
pub struct BypassListArgs {}

#[derive(Debug, Args)]
pub struct BypassRevokeArgs {
    #[arg(value_name = "ID")]
    pub id: String,
}

// ---------------------------------------------------------------------------
// client
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct ClientCmd {
    #[command(subcommand)]
    pub command: ClientSub,
}

#[derive(Debug, Subcommand)]
pub enum ClientSub {
    /// Install AIKit's integration for a client (hook dispatcher entries, etc.).
    Install(ClientInstallArgs),
    /// Launch a client with the current context's projection.
    Launch(ClientLaunchArgs),
    /// Report a client's installation and projection status.
    Status(ClientStatusArgs),
}

#[derive(Debug, Args)]
pub struct ClientInstallArgs {
    #[arg(value_name = "CLIENT")]
    pub client: String,
}
#[derive(Debug, Args)]
pub struct ClientLaunchArgs {
    #[arg(value_name = "CLIENT")]
    pub client: String,
}
#[derive(Debug, Args)]
pub struct ClientStatusArgs {
    #[arg(value_name = "CLIENT")]
    pub client: Option<String>,
}

// ---------------------------------------------------------------------------
// mux
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct MuxCmd {
    #[command(subcommand)]
    pub command: MuxSub,
}

#[derive(Debug, Subcommand)]
pub enum MuxSub {
    /// Install multiplexer integration (tmux options, cmux hooks).
    Install(MuxInstallArgs),
    /// Detect the current multiplexer stack.
    Detect(MuxDetectArgs),
}

#[derive(Debug, Args)]
pub struct MuxInstallArgs {
    #[arg(value_name = "MUX")]
    pub mux: Option<String>,
    /// Root-table key that opens the AIKit popup.
    #[arg(long, default_value = "M-a", value_name = "KEY")]
    pub key: String,
    /// Deliberately replace an effective binding already using this key.
    #[arg(long)]
    pub replace_key: bool,
}
#[derive(Debug, Args)]
pub struct MuxDetectArgs {}

// ---------------------------------------------------------------------------
// hook
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct HookCmd {
    #[command(subcommand)]
    pub command: HookSub,
}

#[derive(Debug, Subcommand)]
pub enum HookSub {
    /// Dispatch a client hook event through the immutable chain.
    Dispatch(HookDispatchArgs),
}

#[derive(Debug, Args)]
pub struct HookDispatchArgs {
    /// The client whose protocol the event is in, e.g. `claude`.
    #[arg(value_name = "CLIENT")]
    pub client: String,
    /// The event name, e.g. `PreToolUse`.
    #[arg(value_name = "EVENT")]
    pub event: String,
}

// ---------------------------------------------------------------------------
// capabilities
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct CapabilitiesCmd {
    #[command(subcommand)]
    pub command: CapabilitiesSub,
}

#[derive(Debug, Subcommand)]
pub enum CapabilitiesSub {
    /// List the capabilities the broker exposes for the current context.
    List(CapabilitiesListArgs),
    /// Read one capability's guidance/preview text.
    Read(CapabilitiesReadArgs),
}

#[derive(Debug, Args)]
pub struct CapabilitiesListArgs {}
#[derive(Debug, Args)]
pub struct CapabilitiesReadArgs {
    #[arg(value_name = "CAPABILITY")]
    pub capability: String,
}

// ---------------------------------------------------------------------------
// log
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct LogCmd {
    #[command(subcommand)]
    pub command: LogSub,
}

#[derive(Debug, Subcommand)]
pub enum LogSub {
    /// Export the event log as JSON lines.
    Export(LogExportArgs),
}

#[derive(Debug, Args)]
pub struct LogExportArgs {
    #[arg(long, default_value_t = 200)]
    pub limit: u32,
}

// ---------------------------------------------------------------------------
// shell
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct ShellCmd {
    #[command(subcommand)]
    pub command: ShellSub,
}

#[derive(Debug, Subcommand)]
pub enum ShellSub {
    /// Print the integration snippet for a shell.
    Init(ShellInitArgs),
}

#[derive(Debug, Args)]
pub struct ShellInitArgs {
    /// The shell: `bash`, `zsh`, or `fish`.
    #[arg(value_name = "SHELL")]
    pub shell: String,
}
