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
    /// Resolve typed resources and operative expressions through the shared search field.
    #[command(visible_alias = "resolve")]
    Search(SearchArgs),
    /// Navigate provider-neutral project knowledge through the shared application faculty.
    Knowledge(KnowledgeCmd),
    /// Validate, write and repair `okf-wiki/v1` Agent Wiki files.
    Wiki(WikiCmd),
    /// Show the effective view for the current context.
    Status(StatusArgs),
    /// Explain why a capability or V2 Resource has its current effective evidence.
    Explain(ExplainArgs),
    /// Read cross-domain evidence-bearing History, optionally scoped to one Resource.
    History(HistoryArgs),
    /// Show what applying the current declarations would change.
    Diff(DiffArgs),
    /// Run the health checks.
    Doctor(DoctorArgs),
    /// Inspect, bind and explicitly import credentials.
    Credential(CredentialCmd),
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
    /// Compose the launch plan: Central profile + Actuation instantiation receipt → actor bootstrap.
    Compose(ComposeArgs),
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
    /// Discover Methods: skills whose description carries the METHOD: prefix.
    Method(MethodArgs),
    /// Record review decisions for catalogued capsule revisions.
    Trust(TrustCmd),
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
    /// Run, inspect and query the Agency Gateway service.
    Gateway(GatewayCmd),
}

/// `aikit gateway serve` — the persistent service carriers.
#[derive(Debug, Args)]
pub struct GatewayServeArgs {
    /// WebSocket bind address (`HOST:PORT`); requires a token.
    #[arg(long = "ws", value_name = "HOST:PORT")]
    pub websocket_bind: Option<String>,
    /// Bearer token for the WebSocket carrier, or `AIKIT_GATEWAY_TOKEN`.
    #[arg(long = "ws-token", value_name = "TOKEN")]
    pub websocket_token: Option<String>,
    /// Unix-domain socket path for the same-host carrier.
    #[arg(long = "unix", value_name = "PATH")]
    pub unix_socket: Option<std::path::PathBuf>,
    /// Persist semantic state across restarts to this file.
    #[arg(long = "state-file", value_name = "PATH")]
    pub state_file: Option<std::path::PathBuf>,
    /// Semantic gateway ref, or `AIKIT_GATEWAY_REF`.
    #[arg(long = "gateway-ref", value_name = "REF")]
    pub gateway_ref: Option<String>,
}

/// `aikit gateway <query>` — one command against a running gateway.
#[derive(Debug, Args)]
pub struct GatewayQueryArgs {
    /// Query the gateway at this Unix-domain socket.
    #[arg(long = "unix", value_name = "PATH")]
    pub unix_socket: Option<std::path::PathBuf>,
    /// Query the gateway WebSocket carrier at `HOST:PORT`.
    #[arg(long = "ws", value_name = "HOST:PORT")]
    pub websocket_bind: Option<String>,
    /// Request path of the WebSocket upgrade.
    #[arg(long = "ws-path", value_name = "PATH", default_value = "/")]
    pub websocket_path: String,
    /// Bearer token for the WebSocket carrier, or `AIKIT_GATEWAY_TOKEN`.
    #[arg(long = "ws-token", value_name = "TOKEN")]
    pub websocket_token: Option<String>,
}

#[derive(Debug, Args)]
pub struct GatewayCmd {
    #[command(subcommand)]
    pub command: GatewaySub,
}

#[derive(Debug, Subcommand)]
pub enum GatewaySub {
    /// Run the persistent gateway service until a `shutdown` command.
    Serve(GatewayServeArgs),
    /// Negotiate protocol versions with a running gateway.
    Protocol(GatewayQueryArgs),
    /// Discover the connectors and bindings of a running gateway.
    Discover(GatewayQueryArgs),
    /// Read the counters and connector health of a running gateway.
    Status(GatewayQueryArgs),
    /// Read the live agency/session/stream/surface ecology of a running gateway.
    Ecology(GatewayQueryArgs),
    /// Read the serialisable semantic snapshot of a running gateway.
    Snapshot(GatewayQueryArgs),
}

/// Arguments for `aikit compose` — the composition reads the authored ground;
/// nothing here selects a model or harness by hand.
#[derive(Debug, Args)]
pub struct ComposeArgs {}

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
    /// List registered projects exhaustively, optionally filtering by id or root.
    List(ProjectListArgs),
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
pub struct ProjectListArgs {
    #[arg(long)]
    pub filter: Option<String>,
}

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
    /// Promote the candidate snapshot; local directories need no extra trust flag.
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
    /// The directory is Control ground: read the sibling `central.skill/v1`
    /// contract (`skill.json`) beside each skill, so standing and provenance
    /// become capability metadata and a retired standing never projects.
    #[arg(long)]
    pub control_ground: bool,
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
    /// Stage into a published Central skill scope (normally Control/user/skills).
    #[arg(long, value_name = "PATH")]
    pub control_ground: Option<std::path::PathBuf>,
    /// Publish this native current-generation skill tree into ROOT. With
    /// --control-ground, replace the whole staged root; otherwise reconcile
    /// skill entries while preserving harness-owned files and undo material.
    #[arg(long, value_name = "PATH")]
    pub projection: Option<std::path::PathBuf>,
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
pub struct KnowledgeCmd {
    #[command(subcommand)]
    pub command: KnowledgeSub,
}

#[derive(Debug, Subcommand)]
pub enum KnowledgeSub {
    Search(KnowledgeSearchArgs),
    Read(KnowledgeAddressArgs),
    Relations(KnowledgeRelationsArgs),
    Route(KnowledgeRouteArgs),
    Frame(KnowledgeRouteArgs),
    Sources(KnowledgeAddressArgs),
    Explain(KnowledgeAddressArgs),
    History(KnowledgeHistoryArgs),
    Status(KnowledgeStatusArgs),
    Forget(KnowledgeForgetCmd),
}

#[derive(Debug, Args)]
pub struct KnowledgeSearchArgs {
    #[arg(value_name = "QUERY", default_value = "")]
    pub query: String,
    #[arg(long, default_value_t = 50)]
    pub limit: usize,
}

#[derive(Debug, Args)]
pub struct KnowledgeAddressArgs {
    /// Typed address JSON from `knowledge search`, or `wiki=REF`, `source=REF`, `project=REF`.
    #[arg(value_name = "ADDRESS")]
    pub address: String,
}

#[derive(Debug, Args)]
pub struct KnowledgeRelationsArgs {
    #[arg(value_name = "ADDRESS")]
    pub address: String,
    #[arg(long, default_value_t = 2)]
    pub depth: u8,
    #[arg(long, default_value_t = 256)]
    pub max_nodes: usize,
    #[arg(long, default_value_t = 512)]
    pub max_edges: usize,
}

#[derive(Debug, Args)]
pub struct KnowledgeRouteArgs {
    #[arg(long)]
    pub query: Option<String>,
    #[arg(value_name = "ADDRESS", required = true)]
    pub addresses: Vec<String>,
}

#[derive(Debug, Args)]
pub struct KnowledgeHistoryArgs {
    #[arg(value_name = "RESOURCE")]
    pub resource: Option<String>,
}

#[derive(Debug, Args)]
pub struct KnowledgeStatusArgs {}

#[derive(Debug, Args)]
pub struct KnowledgeForgetCmd {
    #[command(subcommand)]
    pub command: KnowledgeForgetSub,
}

#[derive(Debug, Subcommand)]
pub enum KnowledgeForgetSub {
    Destination(KnowledgeForgetResourceArgs),
    Route(KnowledgeForgetResourceArgs),
    Project(KnowledgeForgetResourceArgs),
    All(KnowledgeForgetAllArgs),
}

#[derive(Debug, Args)]
pub struct KnowledgeForgetResourceArgs {
    #[arg(value_name = "RESOURCE")]
    pub resource: String,
}

#[derive(Debug, Args)]
pub struct KnowledgeForgetAllArgs {}

/// `aikit wiki` — the write side of the Agent Wiki.
///
/// Wiki tooling is AVAILABLE, NOT ENFORCED: every command names the file it
/// touches, validates the post-mutation whole before persisting, writes through a
/// temp-file rename and advances the revision of whatever it changed. Nothing
/// here discovers a file to mutate or couples to a bootstrap process.
#[derive(Debug, Args)]
pub struct WikiCmd {
    #[command(subcommand)]
    pub command: WikiSub,
}

#[derive(Debug, Subcommand)]
pub enum WikiSub {
    /// Parse a Wiki file, rebuild the index over it and publish every finding.
    Validate(WikiValidateArgs),
    /// Write a WikiNode into a Wiki file.
    Node(WikiNodeCmd),
    /// Write a WikiEdge into a Wiki file.
    Edge(WikiEdgeCmd),
    /// Write a WikiSpace, or federate two of them.
    Space(WikiSpaceCmd),
    /// Doctor, prune and adopt the Central root Wiki.
    Root(WikiRootCmd),
    /// Stage a source file into the Wiki by its authored QL frontmatter.
    Stage(WikiStageArgs),
    /// Ingest an authored corpus directory (rooms, records, links, tags,
    /// register frontmatter) into a Wiki file.
    Ingest(WikiIngestArgs),
    /// Read a Wiki file's semantic index: search, neighbours, backlinks.
    Query(WikiQueryCmd),
}

#[derive(Debug, Args)]
pub struct WikiIngestArgs {
    /// The corpus directory to walk (e.g. an essay's canonical publication
    /// body, `submission-package/essay/`). Walked recursively; only files
    /// named by `--extension` are read as candidate records.
    #[arg(value_name = "CORPUS_DIR")]
    pub corpus: std::path::PathBuf,
    /// The wiki.json file to write the ingested objects into.
    #[arg(long, value_name = "PATH")]
    pub file: std::path::PathBuf,
    /// Write the ingestion. Without it, ingest only reports what it would
    /// do — the same dry-run-by-default convention as `wiki root prune`.
    #[arg(long)]
    pub apply: bool,
    /// Replace an object the file already holds (advancing its revision)
    /// instead of refusing on collision. Off by default: ingest never
    /// silently overwrites an authored or previously-ingested object.
    #[arg(long)]
    pub update: bool,
    /// Only files with this extension are read as candidate records.
    #[arg(long, value_name = "EXT", default_value = "md")]
    pub extension: String,
    /// How many leading path segments of a record's corpus-relative path
    /// name its room. A room is relative to the root ingest was pointed at:
    /// reading the Return of Zero corpus from its `symbolon/` body wants 1,
    /// reading it from the Obsidian vault root a directory above wants 2, or
    /// every record collapses into a single `symbolon` room. 0 compiles no
    /// room spaces at all.
    #[arg(long, value_name = "N", default_value_t = 1)]
    pub room_depth: usize,
    /// Where to write the SourcePool material the corpus compiles to — the
    /// bindings that carry its bibliography and its tags. Defaults to a
    /// `<wiki-file-stem>.sources/` directory beside `--file`. Written only
    /// with `--apply`, and only files this command itself owns
    /// (`corpus-*.json`) are replaced.
    #[arg(long, value_name = "DIR")]
    pub source_pool: Option<std::path::PathBuf>,
}

#[derive(Debug, Args)]
pub struct WikiQueryCmd {
    #[command(subcommand)]
    pub command: WikiQuerySub,
}

#[derive(Debug, Subcommand)]
pub enum WikiQuerySub {
    /// Full-text search over a Wiki file's nodes, edges, spaces and readings.
    Search(WikiQuerySearchArgs),
    /// Every object one ref links to (outgoing and incoming), by relation.
    Neighbours(WikiQueryRefArgs),
    /// Every object that links *to* one ref — the backlinks view.
    Backlinks(WikiQueryRefArgs),
}

#[derive(Debug, Args)]
pub struct WikiQuerySearchArgs {
    /// The search text.
    #[arg(value_name = "QUERY")]
    pub query: String,
    /// The wiki.json file to read.
    #[arg(long, value_name = "PATH")]
    pub file: std::path::PathBuf,
    #[arg(long, default_value_t = 20)]
    pub limit: usize,
}

#[derive(Debug, Args)]
pub struct WikiQueryRefArgs {
    /// The canonical ref to query from.
    #[arg(value_name = "REF")]
    pub resource_ref: String,
    /// The wiki.json file to read.
    #[arg(long, value_name = "PATH")]
    pub file: std::path::PathBuf,
    #[arg(long, default_value_t = 50)]
    pub limit: usize,
}

#[derive(Debug, Args)]
pub struct WikiStageArgs {
    /// The markdown source to stage. Its frontmatter carries the authored QL
    /// alignment; staging records it, it does not guess one.
    #[arg(value_name = "SOURCE_PATH")]
    pub source: std::path::PathBuf,
    /// The wiki.json file to write.
    #[arg(long, value_name = "PATH")]
    pub file: std::path::PathBuf,
    /// The staged node's ref. Defaults to `wiki:node:staged/<stem>`.
    #[arg(long, value_name = "REF")]
    pub node_ref: Option<String>,
    /// Space the staged node belongs to. Repeatable.
    #[arg(long, value_name = "SPACE_REF")]
    pub space: Vec<String>,
    /// Title override; defaults to the first `# ` heading or the file stem.
    #[arg(long, value_name = "TITLE")]
    pub title: Option<String>,
    /// Provenance source ref override; defaults to `staging/<stem>`.
    #[arg(long, value_name = "SOURCE_REF")]
    pub source_ref: Option<String>,
    /// Replace the node when the ref is already held (advancing its revision);
    /// the default refuses, exactly like `node create`.
    #[arg(long)]
    pub update: bool,
}

#[derive(Debug, Args)]
pub struct WikiValidateArgs {
    /// The wiki.json object collection to audit.
    #[arg(value_name = "PATH")]
    pub path: std::path::PathBuf,
}

#[derive(Debug, Args)]
pub struct WikiNodeCmd {
    #[command(subcommand)]
    pub command: WikiNodeSub,
}

#[derive(Debug, Subcommand)]
pub enum WikiNodeSub {
    /// Add a node; refuses a ref the file already holds.
    Create(WikiNodeArgs),
    /// Replace a node's body and advance its revision by one.
    Update(WikiNodeArgs),
}

#[derive(Debug, Args)]
pub struct WikiNodeArgs {
    /// The canonical node ref (`wiki:node:…`).
    #[arg(value_name = "REF")]
    pub node_ref: String,
    /// The wiki.json file to write.
    #[arg(long, value_name = "PATH")]
    pub file: std::path::PathBuf,
    /// Space the node belongs to. Repeatable; the node is recorded in the
    /// membership list of each named Space this file holds.
    #[arg(long, value_name = "SPACE_REF")]
    pub space: Vec<String>,
    /// The node type. The ontology is open: unknown types are preserved, never
    /// translated.
    #[arg(long = "type", value_name = "TYPE")]
    pub node_type: Option<String>,
    #[arg(long, value_name = "TITLE")]
    pub title: Option<String>,
    /// A source the node is grounded in. Repeatable; each becomes provenance.
    #[arg(long, value_name = "SOURCE_REF")]
    pub source: Vec<String>,
    /// Read the whole node JSON body from stdin instead of these flags. The body
    /// replaces the node wholesale; identity and revision stay with the file.
    #[arg(long, conflicts_with_all = ["space", "node_type", "title", "source"])]
    pub stdin: bool,
}

#[derive(Debug, Args)]
pub struct WikiEdgeCmd {
    #[command(subcommand)]
    pub command: WikiEdgeSub,
}

#[derive(Debug, Subcommand)]
pub enum WikiEdgeSub {
    /// Add a directed relation between two Wiki objects.
    Add(WikiEdgeArgs),
}

#[derive(Debug, Args)]
pub struct WikiEdgeArgs {
    /// The ref the relation starts from.
    #[arg(value_name = "FROM_REF")]
    pub from_ref: String,
    /// The relation. Open vocabulary, preserved verbatim.
    #[arg(long, value_name = "RELATION")]
    pub relation: String,
    /// The ref the relation points at.
    #[arg(long = "to", value_name = "TO_REF")]
    pub to_ref: String,
    /// Where the relation came from: authored, mechanical, compiled, inferred,
    /// learned, QL-derived or MEF-derived.
    #[arg(long, value_name = "ORIGIN")]
    pub origin: String,
    /// The run, source or proposal the relation is attributed to.
    #[arg(long, value_name = "REF")]
    pub origin_ref: Option<String>,
    /// The edge's own ref. Defaults to a deterministic name from its endpoints.
    #[arg(long, value_name = "REF")]
    pub edge_ref: Option<String>,
    /// The wiki.json file to write.
    #[arg(long, value_name = "PATH")]
    pub file: std::path::PathBuf,
    /// Permit endpoints that resolve in a peer Wiki file. They are reported as a
    /// warning either way; this flag only says the warning is expected.
    #[arg(long)]
    pub allow_dangling: bool,
}

#[derive(Debug, Args)]
pub struct WikiSpaceCmd {
    #[command(subcommand)]
    pub command: WikiSpaceSub,
}

#[derive(Debug, Subcommand)]
pub enum WikiSpaceSub {
    /// Add a WikiSpace to a Wiki file.
    Create(WikiSpaceCreateArgs),
    /// Federate a parent and a child Space, reciprocally and idempotently.
    Link(WikiSpaceLinkArgs),
}

#[derive(Debug, Args)]
pub struct WikiSpaceCreateArgs {
    /// The canonical Space ref (`wiki:space:…`).
    #[arg(value_name = "REF")]
    pub space_ref: String,
    /// The Space this one federates under. Omitted for a detached root Space.
    #[arg(long, value_name = "PARENT_REF")]
    pub parent: Option<String>,
    #[arg(long, value_name = "TITLE")]
    pub title: String,
    /// The wiki.json file to write.
    #[arg(long, value_name = "PATH")]
    pub file: std::path::PathBuf,
}

#[derive(Debug, Args)]
pub struct WikiSpaceLinkArgs {
    /// The federating Space.
    #[arg(value_name = "PARENT_REF")]
    pub parent_ref: String,
    /// The federated Space.
    #[arg(value_name = "CHILD_REF")]
    pub child_ref: String,
    /// The wiki.json file that holds the parent Space.
    #[arg(long, value_name = "PATH")]
    pub file: std::path::PathBuf,
    /// The wiki.json file that holds the child Space, when it is not `--file`.
    /// Both sides of the federation are written, each with its own revision.
    #[arg(long, value_name = "PATH")]
    pub child_file: Option<std::path::PathBuf>,
}

#[derive(Debug, Args)]
pub struct WikiRootCmd {
    #[command(subcommand)]
    pub command: WikiRootSub,
}

#[derive(Debug, Subcommand)]
pub enum WikiRootSub {
    /// Resolve every `child_space_refs` entry of the root Space against the
    /// filesystem and print the dangling set. Read-only.
    Doctor(WikiRootArgs),
    /// Retract one child ref from the root Space. Dry run unless `--apply`.
    Prune(WikiRootPruneArgs),
    /// Idempotently federate an existing project Wiki into the root Space.
    Adopt(WikiRootAdoptArgs),
    /// Ensure a Space is anchored on its root node: the Central root on the
    /// user identity node, or one project on its project root node.
    Anchor(WikiRootAnchorArgs),
}

#[derive(Debug, Args)]
pub struct WikiRootAnchorArgs {
    /// The Central directory (or its wiki.json). Defaults to discovery from
    /// the working directory; anchors the root Space on the identity node.
    #[arg(long, value_name = "PATH")]
    pub root: Option<std::path::PathBuf>,
    /// Anchor this project's Space instead of the Central root. The path is
    /// the project root holding a ProjectCentral manifest.
    #[arg(long, value_name = "PATH", conflicts_with = "root")]
    pub project: Option<std::path::PathBuf>,
}

#[derive(Debug, Args)]
pub struct WikiRootArgs {
    /// The Central root directory, or its wiki.json. Defaults to discovery from
    /// the working directory: the nearest ancestor holding
    /// `Control/agents/wiki/wiki.json`.
    #[arg(long, value_name = "PATH")]
    pub root: Option<std::path::PathBuf>,
}

#[derive(Debug, Args)]
pub struct WikiRootPruneArgs {
    /// The child Space ref to retract.
    #[arg(value_name = "CHILD_REF")]
    pub child_ref: String,
    /// Write the retraction. Without it the command only reports what it would do.
    #[arg(long)]
    pub apply: bool,
    /// The Central root directory, or its wiki.json.
    #[arg(long, value_name = "PATH")]
    pub root: Option<std::path::PathBuf>,
}

#[derive(Debug, Args)]
pub struct WikiRootAdoptArgs {
    /// The project directory whose ProjectCentral Wiki is already authored and
    /// only needs federating.
    #[arg(value_name = "PROJECT_PATH")]
    pub project_path: std::path::PathBuf,
    /// The Central root directory, or its wiki.json.
    #[arg(long, value_name = "PATH")]
    pub root: Option<std::path::PathBuf>,
}

#[derive(Debug, Args)]
pub struct StatusArgs {
    /// Include catalogued-but-inactive capabilities.
    #[arg(long)]
    pub all: bool,
}

#[derive(Debug, Args)]
pub struct ExplainArgs {
    /// Capability id or canonical V2 ResourceRef.
    #[arg(value_name = "RESOURCE", required_unless_present = "credential")]
    pub capability: Option<String>,
    /// Explain resolution for this semantic CredentialRef instead of a capability/Resource.
    #[arg(long, value_name = "CREDENTIAL", conflicts_with = "capability")]
    pub credential: Option<String>,
    /// Named shell/project environment source to make visible in the explanation.
    #[arg(long, value_name = "NAME", requires = "credential")]
    pub env_var: Option<String>,
    /// Named project .env file. It is inspected only as part of this explicit credential flow.
    #[arg(long, value_name = "FILE", requires = "env_var")]
    pub project_env: Option<std::path::PathBuf>,
    /// Explicitly permit the environment-import tier for this resolution.
    #[arg(long, requires = "env_var")]
    pub from_env: bool,
    /// Explain the headless/CI resolution path.
    #[arg(long, requires = "credential")]
    pub headless: bool,
}

#[derive(Debug, Args)]
pub struct HistoryArgs {
    /// Optional canonical ResourceRef to filter the common timeline.
    #[arg(value_name = "RESOURCE")]
    pub resource: Option<String>,
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
pub struct CredentialCmd {
    #[command(subcommand)]
    pub command: CredentialSub,
}

#[derive(Debug, Subcommand)]
pub enum CredentialSub {
    /// Resolve an existing binding or run the explicit initial-config flow.
    Setup(CredentialSetupArgs),
    /// Explain provider eligibility without materialising secret data.
    Explain(CredentialExplainArgs),
    /// List safe persisted binding metadata.
    List(CredentialListArgs),
}

#[derive(Debug, Args)]
pub struct CredentialSetupArgs {
    #[arg(value_name = "CREDENTIAL")]
    pub credential: String,
    #[arg(long, value_name = "CONSUMER", default_value = "operator:aikit")]
    pub consumer: String,
    #[arg(
        long,
        value_name = "PURPOSE",
        default_value = "provider authentication"
    )]
    pub purpose: String,
    #[arg(long, value_name = "NAME")]
    pub env_var: Option<String>,
    #[arg(long, value_name = "FILE", requires = "env_var")]
    pub project_env: Option<std::path::PathBuf>,
    /// Explicitly choose the environment-import path. Never implied by variable presence.
    #[arg(long, requires = "env_var")]
    pub from_env: bool,
    /// Never prompt. Existing binding or explicit --from-env must resolve.
    #[arg(long)]
    pub headless: bool,
}

#[derive(Debug, Args)]
pub struct CredentialExplainArgs {
    #[arg(value_name = "CREDENTIAL")]
    pub credential: String,
    #[arg(long, value_name = "CONSUMER", default_value = "operator:aikit")]
    pub consumer: String,
    #[arg(
        long,
        value_name = "PURPOSE",
        default_value = "credential resolution explanation"
    )]
    pub purpose: String,
    #[arg(long, value_name = "NAME")]
    pub env_var: Option<String>,
    #[arg(long, value_name = "FILE", requires = "env_var")]
    pub project_env: Option<std::path::PathBuf>,
    #[arg(long, requires = "env_var")]
    pub from_env: bool,
    #[arg(long)]
    pub headless: bool,
}

#[derive(Debug, Args)]
pub struct CredentialListArgs {}

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
pub struct MethodArgs {
    #[command(subcommand)]
    pub command: MethodCommand,
}

#[derive(Debug, Subcommand)]
pub enum MethodCommand {
    /// List detected methods with their effective state in this context.
    List {
        /// Only show methods whose name or payload contains this substring.
        filter: Option<String>,
    },
}

/// Record review decisions for catalogued capsule revisions. The only designed
/// path for a capsule that arrives through a home or project registry rather
/// than a managed skill source — those carry no `source promote --trust`.
#[derive(Debug, Args)]
pub struct TrustCmd {
    #[command(subcommand)]
    pub command: TrustSub,
}

#[derive(Debug, Subcommand)]
pub enum TrustSub {
    /// Record that a catalogued capsule revision was reviewed and is trusted.
    Record(TrustRecordArgs),
    /// Show the recorded trust state of a catalogued capsule's revisions.
    Show(TrustShowArgs),
}

#[derive(Debug, Args)]
pub struct TrustRecordArgs {
    #[arg(value_name = "CAPABILITY")]
    pub capability: String,
    /// Record against this registry source rather than the one that supplied
    /// the capsule.
    #[arg(long, value_name = "SOURCE")]
    pub source: Option<String>,
    /// A short review note recorded with the decision.
    #[arg(long, value_name = "TEXT")]
    pub note: Option<String>,
}

#[derive(Debug, Args)]
pub struct TrustShowArgs {
    #[arg(value_name = "CAPABILITY")]
    pub capability: String,
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
    /// A session name, session capsule, or portable session spec.
    #[arg(value_name = "SESSION_OR_SPEC")]
    pub spec: Option<String>,
}
#[derive(Debug, Args)]
pub struct SessionReconcileArgs {
    /// A session name, session capsule, or portable session spec.
    #[arg(value_name = "SESSION_OR_SPEC")]
    pub spec: Option<String>,
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
