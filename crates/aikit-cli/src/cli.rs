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
#[command(name = "aikit", version = version_line(), about, disable_help_subcommand = true)]
pub struct Cli {
    /// Emit machine-readable JSON on stdout.
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
    /// Validate an externally authored harness-profile document against the
    /// `aikit.harness-profile/v1` schema and admission grammar — the outside
    /// author's intake check. Validation only; nothing is registered, applied
    /// or projected.
    HarnessProfile(HarnessProfileCmd),
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
    /// Read the bounded Development Field carrier/provenance/Git substrate.
    DevelopmentField(DevelopmentFieldArgs),
    /// Project repository checkouts onto their canonical target (origin/main):
    /// report drift, and with --apply fast-forward only the clean, behind ones.
    Worktree(WorktreeCmd),
    /// Navigate provider-neutral project knowledge through the shared application faculty.
    Knowledge(KnowledgeCmd),
    /// Owner-side Flow cognition: explicit Contemplate(FlowRef) with
    /// preflight and Explain disclosure, and the changed-since-thought read.
    Flow(FlowCmd),
    /// Validate, write and repair `okf-wiki/v1` Agent Wiki files.
    Wiki(WikiCmd),
    /// Declare, validate and compress QL-shaped `WikiConstellation`s against
    /// the pinned QL shape contract (CASE 18: the shape system's own product
    /// surface, separate from `wiki` so this case never has to touch that
    /// command's dispatch).
    WikiShape(WikiShapeCmd),
    /// Construct revisioned native Wiki wholes and contextual participations.
    WikiConstruct(crate::wiki_construct::ConstructArgs),
    /// Show the effective view for the current context.
    Status(StatusArgs),
    /// Emit the owner settings-disclosure descriptor for the O:I System surface.
    System(SystemArgs),
    /// Resolve the shipped six-product Guardian family against this machine's registered sources.
    Family(FamilyArgs),
    /// Emit the owner configuration contribution for the O:I configuration plane.
    ConfigContribution(ConfigContributionArgs),
    /// Owner-native configuration verbs for the O:I configuration plane.
    Config(ConfigCmd),
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
    /// Inspect the continuity engine's star commands and verify a close-out.
    Continuity(ContinuityCmd),
    /// Bring up, attach to and reconcile session topologies.
    Session(SessionCmd),
    /// Operate durable SessionSpace semantics (folded companion surface; O-I #376).
    ///
    /// A pure pass-through: everything after `session-space` (verbs, flags,
    /// `--help`) is forwarded verbatim to the one folded companion surface. The
    /// help flag is disabled here so `--help`/`-h` reach that surface instead of
    /// this wrapper; run `aikit session-space --help` for the verb list.
    #[command(name = "session-space", disable_help_flag = true)]
    SessionSpace {
        /// Everything after `session-space`, forwarded verbatim.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<std::ffi::OsString>,
    },
    /// Compose the launch plan: Central profile + Actuation instantiation receipt → actor bootstrap.
    Compose(ComposeArgs),
    /// Resolve one Model through the current roster without actualising it.
    ModelResolve(ModelResolveArgs),
    /// Read a Provider Source into the canonical Model catalogue, and read the catalogue back.
    ModelCatalogue(ModelCatalogueCmd),
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
    /// Run a harness against a declared model route (ADR 0005 Stage 2).
    Harness(HarnessCmd),
    /// List, check and install user-owned alias families (ADR 0005 Stage 1).
    Alias(AliasCmd),
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
    /// Read praxis: list Skills by form (Skill / Method / Methodology) and
    /// disclose an Agent's carried, selected and operative praxis.
    Praxis(PraxisCmd),
    /// A2A interoperability projections (the published Agent Card).
    A2a(A2aCmd),
    /// Authorise and read versioned Routine invocation evidence.
    Routine(RoutineCmd),
    /// Invoke or validate the general typed Jev decision capability.
    Jev(JevCmd),
    /// Prepare, inspect and mutate Redis-backed participant NOW context.
    #[command(name = "now-context")]
    NowContext(NowContextCmd),
    /// Start developmental work through Factory's native Commission boundary.
    Factory(FactoryCmd),
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
    /// Who and where am I: the joined World inhabitation reading
    /// (`aikit.inhabitation-reading/v1`) over Central, Actuation, Factory and
    /// AIKit's own SessionSpace/Redis projections.
    Whoami(WhoamiArgs),
    /// Trace the current operation back to ProjectCentral ground
    /// (`aikit.refocus-reading/v1`): work, Position, NOW, body, nearby work,
    /// changed sources and the Return target.
    Refocus(RefocusArgs),
    /// Occupy a World Position and launch a body into it: claim the tenure
    /// through Actuation, then exec the harness with `OI_POSITION_REF` and
    /// `OI_OCCUPANT_GENERATION` stamped. Leaving is explicit (`--release`).
    Inhabit(InhabitArgs),
}

/// `aikit inhabit`.
#[derive(Debug, Args)]
pub struct InhabitArgs {
    /// The Position: a `central:position:…` ref or an `@handle`.
    #[arg(long, value_name = "REF|@HANDLE")]
    pub position: String,
    /// The Agent to occupy it (defaults to the Position's single eligible Agent).
    #[arg(long, value_name = "AGENT_REF")]
    pub agent: Option<String>,
    /// The Agency (defaults to the Agent's single admitted AIKit agency).
    #[arg(long, value_name = "AGENCY_REF")]
    pub agency: Option<String>,
    /// Take over from the current occupant (continuity handover).
    #[arg(long, conflicts_with_all = ["fresh", "release", "attach"])]
    pub handover: bool,
    /// Replace any current occupant with a fresh one.
    #[arg(long, conflicts_with_all = ["release", "attach"])]
    pub fresh: bool,
    /// Why this tenure opens (or ends, with --release).
    #[arg(long)]
    pub reason: Option<String>,
    /// End the tenure this body holds instead of claiming one.
    #[arg(long, conflicts_with_all = ["agent", "agency", "attach"])]
    pub release: bool,
    /// Continue the tenure this occupant already holds (e.g. resuming its
    /// harness session): verify the generation is still current, then exec the
    /// harness stamped with it. Nothing is claimed; a superseded generation is
    /// refused.
    #[arg(long, conflicts_with_all = ["agent", "agency"])]
    pub attach: bool,
    /// With --release or --attach: the generation held (defaults to
    /// `OI_OCCUPANT_GENERATION`).
    #[arg(long, value_name = "GENERATION_REF")]
    pub generation: Option<String>,
    #[arg(long = "agent-session", value_name = "REF")]
    pub agent_session: Option<String>,
    #[arg(long = "session-space", value_name = "REF")]
    pub session_space: Option<String>,
    #[arg(long = "harness-composition", value_name = "REF")]
    pub harness_composition: Option<String>,
    #[arg(long, value_name = "REF")]
    pub model: Option<String>,
    /// The harness argv to exec after the claim (after `--`). Without one the
    /// claim is printed with the variables to export.
    #[arg(last = true, value_name = "HARNESS_ARGV")]
    pub command: Vec<String>,
}

/// `aikit whoami`.
#[derive(Debug, Args)]
pub struct WhoamiArgs {
    /// Read this Position instead of resolving one (`OI_POSITION_REF`, then an
    /// occupancy naming the current AgentSession).
    #[arg(long, value_name = "POSITION_REF")]
    pub position: Option<String>,
    /// The current AgentSession, for resolving the occupancy that names it
    /// (defaults to `AIKIT_SESSION_ID`).
    #[arg(long = "agent-session", value_name = "REF")]
    pub agent_session: Option<String>,
    /// Every facet with the owner's full answer, the ActorBootstrap
    /// composition and the owner calls that were made.
    #[arg(long)]
    pub full: bool,
    /// Read the Redis World projection first (reporting its age and basis),
    /// falling back to live owner joins.
    #[arg(long, conflicts_with_all = ["publish", "rebuild"])]
    pub hot: bool,
    /// Publish the live reading's refs/revisions to the Redis World projection
    /// (compare-and-swap on its version).
    #[arg(long)]
    pub publish: bool,
    /// Recompute the reading from its owners and republish the projection.
    #[arg(long)]
    pub rebuild: bool,
    /// `aikit.redis-now-config/v1` document (defaults to `AIKIT_WORLD_REDIS_CONFIG`).
    #[arg(long = "redis-config", value_name = "PATH")]
    pub redis_config: Option<std::path::PathBuf>,
}

/// `aikit refocus`.
#[derive(Debug, Args)]
pub struct RefocusArgs {
    /// Why this Refocus is read.
    #[arg(long, default_value = "explicit", value_parser = ["explicit", "fresh", "compaction", "transition", "sustained"])]
    pub trigger: String,
    /// Refocus as this Position instead of resolving one.
    #[arg(long, value_name = "POSITION_REF")]
    pub position: Option<String>,
    /// The current AgentSession; also names which hook delivery state to
    /// compare changed sources against (read only — never recorded).
    #[arg(long = "agent-session", value_name = "REF")]
    pub agent_session: Option<String>,
}

/// `aikit worktree` — project repository checkouts onto their canonical target.
#[derive(Debug, Args)]
pub struct WorktreeCmd {
    #[command(subcommand)]
    pub command: WorktreeSub,
}

#[derive(Debug, Subcommand)]
pub enum WorktreeSub {
    /// Report each checkout's drift from the target and, with --apply,
    /// fast-forward the clean, behind ones. Dirty, ahead, and diverged
    /// checkouts are surfaced untouched — projection never discards work.
    Project(WorktreeProjectArgs),
}

/// `aikit worktree project` — whole-suite (or given-set) projection to a target.
#[derive(Debug, Args)]
pub struct WorktreeProjectArgs {
    /// A checkout to project, as `KEY=PATH` (repeatable). `KEY` is the report
    /// key (e.g. the dev-world project key); `PATH` is the checkout root. A bare
    /// `PATH` uses the directory name as the key.
    #[arg(long = "repo", value_name = "KEY=PATH", required = true)]
    pub repos: Vec<String>,
    /// The canonical target: `origin/main` (the default), or a bare ref that
    /// resolves against `origin`.
    #[arg(long, default_value = "origin/main")]
    pub target: String,
    /// Perform the safe fast-forwards. Without it, observe and report only.
    #[arg(long)]
    pub apply: bool,
    /// Do not fetch the remote first; compare against the last-fetched target.
    #[arg(long = "no-fetch")]
    pub no_fetch: bool,
}

/// `aikit development-field` — bounded owner-native carrier reading.
#[derive(Debug, Args)]
pub struct DevelopmentFieldArgs {
    /// Stable ResourceRefs to read. Omit to read the bounded carrier set already present.
    #[arg(long = "ref", value_name = "RESOURCE_REF")]
    pub refs: Vec<String>,
    /// Maximum subjects returned (hard-capped by the core contract).
    #[arg(long, default_value_t = 16)]
    pub limit: usize,
    /// Exact caller-supplied Run/plan Git base revision for current-difference disclosure.
    #[arg(long, value_name = "REVISION")]
    pub base: Option<String>,
    /// Maximum tracked diff bytes returned when --base is supplied.
    #[arg(long = "max-diff-bytes", default_value_t = 262144)]
    pub max_diff_bytes: usize,
    /// Refuse this executable unless it exactly represents this clean source revision.
    #[arg(long = "expect-aikit-revision", value_name = "REVISION")]
    pub expect_aikit_revision: Option<String>,
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
    /// Where the WebSocket bearer token lives: `file:/abs/path` (owner-only,
    /// chmod 600) or a keychain/pass/op/varlock ref. Read once at start.
    #[arg(
        long = "ws-token-location",
        value_name = "LOCATION",
        conflicts_with = "websocket_token"
    )]
    pub websocket_token_location: Option<String>,
    /// Serve the same-host Unix-domain carrier: at PATH, or with no value at
    /// this home's well-known socket. Name it beside --ws so local inbox,
    /// send and turn delivery keep reaching the service.
    #[arg(long = "unix", value_name = "PATH", num_args = 0..=1)]
    pub unix_socket: Option<Option<std::path::PathBuf>>,
    /// Persist semantic state across restarts to this file.
    #[arg(long = "state-file", value_name = "PATH")]
    pub state_file: Option<std::path::PathBuf>,
    /// Semantic gateway ref, or `AIKIT_GATEWAY_REF`.
    #[arg(long = "gateway-ref", value_name = "REF")]
    pub gateway_ref: Option<String>,
}

/// `aikit gateway install-service` — what the kept-alive service serves.
#[derive(Debug, Args)]
pub struct GatewayInstallArgs {
    /// Also serve the authenticated WebSocket carrier at `HOST:PORT`, for
    /// other Workcells to relay through. Requires --ws-token-location.
    #[arg(long = "ws", value_name = "HOST:PORT", requires = "token_location")]
    pub websocket_bind: Option<String>,
    /// Where the WebSocket bearer token lives (`file:/abs/path`, owner-only).
    #[arg(
        long = "ws-token-location",
        value_name = "LOCATION",
        requires = "websocket_bind"
    )]
    pub token_location: Option<String>,
    /// `AIKIT_GATEWAY_REF` for the service (e.g. agency-gateway/omarchy).
    #[arg(long = "gateway-ref", value_name = "REF")]
    pub gateway_ref: Option<String>,
    /// `AIKIT_WORKCELL_REF` for the service (e.g. workcell:omarchy).
    #[arg(long = "workcell-ref", value_name = "REF")]
    pub workcell_ref: Option<String>,
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

#[derive(Debug, Args)]
pub struct JevCmd {
    #[command(subcommand)]
    pub command: JevSub,
}

#[derive(Debug, Subcommand)]
pub enum JevSub {
    /// Validate a captured provider answer against the exact typed request.
    Validate(JevValidateArgs),
    /// Invoke the official Jev provider with explicit native credential and spend bounds.
    Invoke(JevInvokeArgs),
}

#[derive(Debug, Args)]
pub struct JevValidateArgs {
    #[arg(long = "request-file", value_name = "PATH")]
    pub request_file: std::path::PathBuf,
    #[arg(long = "response-file", value_name = "PATH")]
    pub response_file: std::path::PathBuf,
}

#[derive(Debug, Args)]
pub struct JevInvokeArgs {
    #[arg(long = "request-file", value_name = "PATH")]
    pub request_file: std::path::PathBuf,
    #[arg(long = "limits-file", value_name = "PATH")]
    pub limits_file: std::path::PathBuf,
    /// Native secret reference (varlock://, pass://, keychain://, op://; env:// requires explicit opt-in).
    #[arg(long = "credential-ref", value_name = "SECRET_REF")]
    pub credential_ref: String,
    #[arg(long = "invocation-ref", value_name = "RESOURCE_REF")]
    pub invocation_ref: Option<String>,
    #[arg(long = "curl", value_name = "PATH")]
    pub curl: Option<std::path::PathBuf>,
    /// Deterministic protocol proof only: explicit loopback endpoint. Omit for the official provider.
    #[arg(long = "controlled-endpoint", value_name = "HOST:PORT")]
    pub controlled_endpoint: Option<std::net::SocketAddr>,
    /// Permit a deliberately supplied env:// secret reference for this invocation only.
    #[arg(long = "allow-env-import")]
    pub allow_env_import: bool,
}

#[derive(Debug, Args)]
pub struct NowContextCmd {
    #[command(subcommand)]
    pub command: NowContextSub,
}

#[derive(Debug, Subcommand)]
pub enum NowContextSub {
    /// Check the selected Redis material service and report its actual version.
    Status(NowStatusArgs),
    /// Read one Project's bounded Factory sensing projection from Redis.
    FactorySensing(NowFactorySensingArgs),
    /// Resolve Central/BKMR + Wiki + Factory relations and atomically prepare one participant view.
    Prepare(NowPrepareArgs),
    /// Inspect one participant's current prepared version, delivery and cursor state.
    Inspect(NowInspectArgs),
    /// Publish an already owner-resolved prepared view through the same CAS boundary.
    Publish(NowPublishArgs),
    /// Append one replayable semantic source/dependency/Return change for a participant.
    AppendChange(NowAppendChangeArgs),
    /// Revoke one participant's cached material at the disclosure boundary.
    Revoke(NowRevokeArgs),
    /// Assemble the native `aikit.contemplation-field/v1`: telos anchor, UX
    /// spine (stories/practices/coverage), practice→Skill bindings, capability
    /// matrix (with code/test refs and grid relations), changed subject,
    /// GitNexus code lens and deterministic cross-lens joins — the native
    /// continuation of `scripts/jev-redis/assemble_contemplation.py`.
    Field(Box<NowFieldArgs>),
    /// Build the typed `aikit.contemplation-questions/v1` set over an
    /// assembled field and invoke Jev, returning `aikit.contemplation-
    /// decision/v1`.
    Contemplate(NowContemplateArgs),
    /// Prepare `aikit.test-selection/v1` (JSON + Markdown) from a field and
    /// an optional prior contemplation decision. A pure read/compose view —
    /// no new store, no acceptance database.
    TestSelection(NowTestSelectionArgs),
    /// Publish the field, an optional decision and an optional test-selection
    /// into the participant's prepared view through the existing CAS publish
    /// path, and append one replayable change per published item.
    PublishIntelligence(Box<NowPublishIntelligenceArgs>),
}

#[derive(Debug, Args)]
pub struct NowContemplateArgs {
    /// The assembled `aikit.contemplation-field/v1` document.
    #[arg(long, value_name = "PATH")]
    pub field: std::path::PathBuf,
    /// Forward (planning/development) or returning (review/analysis) pass.
    #[arg(
        long,
        value_name = "prospective|retrospective",
        default_value = "prospective"
    )]
    pub pass: String,
    #[arg(long = "limits-file", value_name = "PATH")]
    pub limits_file: std::path::PathBuf,
    /// Native secret reference (varlock://, pass://, keychain://, op://; env:// requires explicit opt-in).
    #[arg(long = "credential-ref", value_name = "SECRET_REF")]
    pub credential_ref: String,
    /// Deterministic protocol proof only: explicit loopback endpoint. Omit for the official provider.
    #[arg(long = "controlled-endpoint", value_name = "HOST:PORT")]
    pub controlled_endpoint: Option<std::net::SocketAddr>,
    #[arg(long = "curl", value_name = "PATH")]
    pub curl: Option<std::path::PathBuf>,
    /// Permit a deliberately supplied env:// secret reference for this invocation only.
    #[arg(long = "allow-env-import")]
    pub allow_env_import: bool,
    /// Minimum Noul score for a candidate to be treated as selected.
    #[arg(long = "relevance-threshold", default_value_t = 0.5)]
    pub relevance_threshold: f64,
    #[arg(long = "invocation-ref", value_name = "RESOURCE_REF")]
    pub invocation_ref: Option<String>,
}

#[derive(Debug, Args)]
pub struct NowTestSelectionArgs {
    /// The assembled `aikit.contemplation-field/v1` document.
    #[arg(long, value_name = "PATH")]
    pub field: std::path::PathBuf,
    /// An `aikit.contemplation-decision/v1` from `now-context contemplate`.
    #[arg(long, value_name = "PATH")]
    pub decision: Option<std::path::PathBuf>,
}

#[derive(Debug, Args)]
pub struct NowPublishIntelligenceArgs {
    #[arg(long = "config-file", value_name = "PATH")]
    pub config_file: std::path::PathBuf,
    #[arg(long = "participant-ref", value_name = "RESOURCE_REF")]
    pub participant_ref: String,
    #[arg(long = "project-ref", value_name = "RESOURCE_REF")]
    pub project_ref: String,
    #[arg(long = "now-ref", value_name = "RESOURCE_REF")]
    pub now_ref: String,
    #[arg(long = "agent-session", value_name = "RESOURCE_REF")]
    pub agent_session: String,
    #[arg(long, value_name = "TEXT")]
    pub concern: String,
    #[arg(long = "disclosure-revision", value_name = "REVISION")]
    pub disclosure_revision: String,
    #[arg(long = "expected-version", value_name = "N")]
    pub expected_version: u64,
    /// The assembled `aikit.contemplation-field/v1` document.
    #[arg(long = "field-file", value_name = "PATH")]
    pub field_file: std::path::PathBuf,
    /// An `aikit.contemplation-decision/v1` from `now-context contemplate`.
    #[arg(long = "decision-file", value_name = "PATH")]
    pub decision_file: Option<std::path::PathBuf>,
    /// An `aikit.test-selection/v1` from `now-context test-selection`.
    #[arg(long = "test-selection-file", value_name = "PATH")]
    pub test_selection_file: Option<std::path::PathBuf>,
    /// Central root to read `central.day.read` from, folding the Day ref/
    /// revision into the prepared basis. Omit to leave the Day basis absent.
    #[arg(long = "central-root", value_name = "DIR")]
    pub central_root: Option<std::path::PathBuf>,
    #[arg(long = "ctrl-bin", value_name = "PATH")]
    pub ctrl_bin: Option<std::path::PathBuf>,
    /// A `REF=REVISION` pair (repeatable) folded into the prepared basis —
    /// e.g. a Workcell root/child NOW ref, supplied by the caller rather
    /// than fetched.
    #[arg(long = "now-basis-ref", value_name = "REF=REVISION")]
    pub now_basis_refs: Vec<String>,
    #[arg(long = "allow-env-import")]
    pub allow_env_import: bool,
}

#[derive(Debug, Args)]
pub struct NowFieldArgs {
    /// A typed `aikit.contemplation-field-request/v1` JSON file. When given,
    /// every other flag below is ignored (the request carries the same
    /// fields under their schema names).
    #[arg(long = "request-file", value_name = "PATH")]
    pub request_file: Option<std::path::PathBuf>,
    /// A project's ProjectCentral dir; matrix carriers are discovered telos-
    /// first (`user/telos/`, then `user/`, then `telos/`), exactly as
    /// `assemble_contemplation.py` does.
    #[arg(long, value_name = "DIR")]
    pub projectcentral: Option<std::path::PathBuf>,
    #[arg(long = "matrix-manifest", value_name = "PATH")]
    pub matrix_manifest: Option<std::path::PathBuf>,
    #[arg(long = "matrix-csv", value_name = "PATH")]
    pub matrix_csv: Option<std::path::PathBuf>,
    /// `ql.ux-spine-trace/1` JSON.
    #[arg(long = "spine-trace", value_name = "PATH")]
    pub spine_trace: Option<std::path::PathBuf>,
    /// The Git repository the spine trace is checked out in; `canonical_skill`
    /// resolves relative to its root. Defaults to the spine trace file's own
    /// containing Git repository.
    #[arg(long = "spine-repo-root", value_name = "DIR")]
    pub spine_repo_root: Option<std::path::PathBuf>,
    /// The ai-kit repository root that `native_skill_ref` values prefixed
    /// `ai-kit:` resolve against. Defaults to this invocation's own repo root.
    #[arg(long = "ai-kit-repo-root", value_name = "DIR")]
    pub ai_kit_repo_root: Option<std::path::PathBuf>,
    /// A telos goal folder (`goal.md` + `tracks/`); anchors the field in the
    /// long horizon.
    #[arg(long = "telos-goal-dir", value_name = "DIR")]
    pub telos_goal_dir: Option<std::path::PathBuf>,
    #[arg(long = "serving-track", value_name = "TRACK")]
    pub serving_track: Option<String>,
    #[arg(long = "now-ref", value_name = "RESOURCE_REF")]
    pub now_ref: Option<String>,
    #[arg(long = "central-root", value_name = "DIR")]
    pub central_root: Option<std::path::PathBuf>,
    #[arg(long = "ctrl-bin", value_name = "PATH")]
    pub ctrl_bin: Option<std::path::PathBuf>,
    #[arg(long = "redis-config", value_name = "PATH")]
    pub redis_config: Option<std::path::PathBuf>,
    #[arg(long = "redis-participant-ref", value_name = "RESOURCE_REF")]
    pub redis_participant_ref: Option<String>,
    #[arg(long = "allow-env-import")]
    pub allow_env_import: bool,
    #[arg(long = "wiki-query", value_name = "QUERY")]
    pub wiki_queries: Vec<String>,
    /// Forward (planning/development) or returning (review/analysis) pass.
    #[arg(
        long,
        value_name = "prospective|retrospective",
        default_value = "prospective"
    )]
    pub pass: String,
    /// A Return or evidence document for the retrospective pass.
    #[arg(long = "return", value_name = "PATH")]
    pub return_file: Option<std::path::PathBuf>,
    /// The changed subject's repository.
    #[arg(long, value_name = "DIR")]
    pub repo: Option<std::path::PathBuf>,
    /// A stable GitNexus registry alias for the repo. Defaults to the
    /// directory's own name, which collides when a repo is checked out at
    /// more than one path under the same basename (e.g. a lane worktree
    /// beside the primary checkout) — name it explicitly in that case.
    #[arg(long = "repo-name", value_name = "NAME")]
    pub repo_name: Option<String>,
    #[arg(long, value_name = "REVISION")]
    pub base: Option<String>,
    #[arg(long, default_value = "HEAD", value_name = "REVISION")]
    pub head: String,
    /// Bound on how many changed-file symbols get a GitNexus context/impact reading.
    #[arg(long = "max-code-symbols", default_value_t = 8)]
    pub max_code_symbols: usize,
    /// `gitnexus` binary override (tests point this at a scripted double).
    #[arg(long = "gitnexus-binary", value_name = "PATH")]
    pub gitnexus_binary: Option<String>,
    /// An `oi.experience.coverage-reading/v1` document (from
    /// `python3 scripts/experience_map.py --output-dir` in O-I). Without it,
    /// capability→story/practice relations are not fabricated as candidates —
    /// only Jev may propose those, and this field assembler runs before Jev.
    #[arg(long = "experience-reading", value_name = "PATH")]
    pub experience_reading: Option<std::path::PathBuf>,
}

#[derive(Debug, Args)]
pub struct NowStatusArgs {
    #[arg(long = "config-file", value_name = "PATH")]
    pub config_file: std::path::PathBuf,
    #[arg(long = "allow-env-import")]
    pub allow_env_import: bool,
}

#[derive(Debug, Args)]
pub struct NowFactorySensingArgs {
    #[arg(long = "config-file", value_name = "PATH")]
    pub config_file: std::path::PathBuf,
    #[arg(long = "project-world-ref", value_name = "PROJECT_REF")]
    pub project_world_ref: String,
    #[arg(long = "allow-env-import")]
    pub allow_env_import: bool,
}

#[derive(Debug, Args)]
pub struct NowPrepareArgs {
    #[arg(long = "request-file", value_name = "PATH")]
    pub request_file: std::path::PathBuf,
}

#[derive(Debug, Args)]
pub struct NowInspectArgs {
    #[arg(long = "config-file", value_name = "PATH")]
    pub config_file: std::path::PathBuf,
    #[arg(long = "participant-ref", value_name = "RESOURCE_REF")]
    pub participant_ref: String,
    /// Validate the cached payload for external-provider disclosure before returning it.
    #[arg(long = "external-provider")]
    pub external_provider: bool,
    #[arg(long = "allow-env-import")]
    pub allow_env_import: bool,
}

#[derive(Debug, Args)]
pub struct NowPublishArgs {
    #[arg(long = "config-file", value_name = "PATH")]
    pub config_file: std::path::PathBuf,
    #[arg(long = "view-file", value_name = "PATH")]
    pub view_file: std::path::PathBuf,
    #[arg(long = "expected-version", value_name = "N")]
    pub expected_version: u64,
    #[arg(long = "allow-env-import")]
    pub allow_env_import: bool,
}

#[derive(Debug, Args)]
pub struct NowAppendChangeArgs {
    #[arg(long = "config-file", value_name = "PATH")]
    pub config_file: std::path::PathBuf,
    #[arg(long = "participant-ref", value_name = "RESOURCE_REF")]
    pub participant_ref: String,
    #[arg(long = "change-file", value_name = "PATH")]
    pub change_file: std::path::PathBuf,
    #[arg(long = "allow-env-import")]
    pub allow_env_import: bool,
}

#[derive(Debug, Args)]
pub struct NowRevokeArgs {
    #[arg(long = "config-file", value_name = "PATH")]
    pub config_file: std::path::PathBuf,
    #[arg(long = "participant-ref", value_name = "RESOURCE_REF")]
    pub participant_ref: String,
    #[arg(long = "disclosure-revision", value_name = "REVISION")]
    pub disclosure_revision: String,
    #[arg(long = "allow-env-import")]
    pub allow_env_import: bool,
}

#[derive(Debug, Args)]
pub struct RoutineCmd {
    #[command(subcommand)]
    pub command: RoutineSub,
}

#[derive(Debug, Args)]
pub struct FactoryCmd {
    #[command(subcommand)]
    pub command: FactorySub,
}

#[derive(Debug, Subcommand)]
pub enum FactorySub {
    /// Commission one developmental difference through the Factory owner CLI.
    StartWork {
        /// Factory-owned developmental provider state to create or reopen.
        #[arg(long, value_name = "PATH")]
        state: std::path::PathBuf,
        /// Exact factory.commission-request/v1 JSON file.
        #[arg(long = "request-file", value_name = "PATH")]
        request_file: std::path::PathBuf,
        /// Native Factory executable; defaults to AIKIT_FACTORY_BIN or `factory`.
        #[arg(long = "factory-bin", value_name = "PATH")]
        factory_bin: Option<std::path::PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
pub enum RoutineSub {
    /// Validate and idempotently admit one authorised Routine occurrence.
    AuthoriseInvocation {
        /// Structured request JSON. Prefix a path with @ to read a file.
        #[arg(long = "request-json", value_name = "JSON|@FILE")]
        request_json: String,
    },
    /// Read one previously admitted invocation envelope.
    Invocation {
        #[arg(value_name = "INVOCATION_REF")]
        invocation_ref: String,
    },
    /// List all admitted invocation envelopes in stable identity order.
    Invocations,
    /// List stored Routines (and, read-only, the foreign harness timers that
    /// no Routine claims).
    List {
        /// Only show Routines in this state: draft | enabled | disabled | stale-proof.
        #[arg(long = "state", value_name = "STATE")]
        state: Option<String>,
    },
    /// Show one stored Routine in full: proof, trigger, authority, binding.
    Show {
        #[arg(value_name = "ROUTINE_REF")]
        routine_ref: String,
    },
    /// Create a Routine from a proven basis. The Routine sits in Draft until
    /// explicitly enabled.
    Create {
        #[arg(long, value_name = "NAME")]
        name: String,
        #[arg(default_value = "", long, value_name = "TEXT")]
        description: Option<String>,
        #[arg(long, value_name = "REF")]
        method: String,
        /// ProvenMethodBasis JSON from `aikit method prove`. Prefix with @ for a file.
        #[arg(long = "proof-json", value_name = "JSON|@FILE")]
        proof_json: String,
        /// Trigger JSON: an aikit.time-schedule/v1 record, or
        /// {"kind":"manual"|"event"|"external", ...}. Prefix with @ for a file.
        #[arg(long = "trigger-json", value_name = "JSON|@FILE")]
        trigger_json: String,
        /// RoutineAuthority JSON: authority_ref, revision, action_refs, granted,
        /// unattended. Prefix with @ for a file.
        #[arg(long = "authority-json", value_name = "JSON|@FILE")]
        authority_json: String,
        /// Opaque Central AgentProfile source relation.
        #[arg(long = "agent-profile", value_name = "REF")]
        agent_profile: Option<String>,
        /// Context scope refs the run resolves inside.
        #[arg(long = "context-scope", value_name = "REF")]
        context_scope: Vec<String>,
    },
    /// Enable a stored Routine with a fresh authority receipt at the current
    /// revision. Schedule and event triggers additionally require unattended
    /// authority.
    Enable {
        #[arg(value_name = "ROUTINE_REF")]
        routine_ref: String,
        #[arg(long = "authority-json", value_name = "JSON|@FILE")]
        authority_json: String,
    },
    /// Disable a stored Routine. Disabled Routines observe nothing.
    Disable {
        #[arg(value_name = "ROUTINE_REF")]
        routine_ref: String,
    },
    /// Run a stored Routine now through the same authorisation gate.
    RunNow {
        #[arg(value_name = "ROUTINE_REF")]
        routine_ref: String,
    },
    /// Replace a Routine's proof after a Method change. The Routine returns to
    /// Disabled and must be explicitly enabled again.
    Reprove {
        #[arg(value_name = "ROUTINE_REF")]
        routine_ref: String,
        #[arg(long = "proof-json", value_name = "JSON|@FILE")]
        proof_json: String,
    },
    /// Delete a stored Routine. Refuses while the Routine is Enabled.
    Delete {
        #[arg(value_name = "ROUTINE_REF")]
        routine_ref: String,
    },
    /// Bind where a native Routine run finds an owner credential its Method
    /// declares (a location, never the value), or clear the binding.
    Credential {
        #[arg(value_name = "ROUTINE_REF")]
        routine_ref: String,
        /// The credential variable the Method's native body declares.
        #[arg(long, value_name = "ENV")]
        env: String,
        /// `file:/abs/path` (owner-only) or a keychain:// / pass:// / op:// /
        /// varlock:// ref.
        #[arg(long, value_name = "LOCATION", required_unless_present = "clear")]
        location: Option<String>,
        #[arg(long, conflicts_with = "location")]
        clear: bool,
    },
    /// Reconcile one foreign harness cron job (read-only over the harness
    /// store) into a Routine. `--report` only reads and reports.
    ImportForeign {
        /// Foreign provider: openclaw-cron | hermes-cron.
        #[arg(long, value_name = "PROVIDER")]
        provider: String,
        /// The job's id in the harness store.
        #[arg(long = "job-id", value_name = "ID")]
        job_id: String,
        /// The Method this job's payload runs. Inferred when omitted.
        #[arg(long, value_name = "REF")]
        method: Option<String>,
        /// ProvenMethodBasis JSON from `aikit method prove`. Prefix with @ for a file.
        #[arg(long = "proof-json", value_name = "JSON|@FILE")]
        proof_json: Option<String>,
        /// Declare the Routine's intent to take over this timer; the harness
        /// timer itself is retired by you, in the harness, after the Routine's
        /// first admitted scheduled run.
        #[arg(long = "adopt", conflicts_with = "report")]
        adopt: bool,
        /// Read-only reconciliation report; nothing is created.
        #[arg(long = "report", conflicts_with = "adopt")]
        report: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum GatewaySub {
    /// Run the persistent gateway service until a `shutdown` command. The
    /// Routine dispatcher ticks every 30 seconds while the service runs.
    Serve(GatewayServeArgs),
    /// Run exactly one dispatcher pass: resolve occurrences, admit due items,
    /// dispatch, record outcomes, exit. No gateway required.
    Tick,
    /// Install the user service that keeps the gateway (the dispatcher tick
    /// and the relay pass) alive across restart, sleep and reboot: a macOS
    /// LaunchAgent or a Linux systemd user unit.
    InstallService(GatewayInstallArgs),
    /// Remove the installed gateway service.
    UninstallService,
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
    /// Who is here: every Position of the Project World with its occupancy,
    /// current work and undelivered Communiques (`aikit.population-reading/v1`).
    Who(GatewayWhoArgs),
    /// Address a Communique to a Position. Never blocks: a vacant Position
    /// holds it for its next occupant; an occupant on another Workcell gets it
    /// relayed through that Workcell's gateway.
    Send(GatewaySendArgs),
    /// The Communiques waiting for an occupant; `--ack` marks them delivered
    /// to this body's verified occupant generation.
    Inbox(GatewayInboxArgs),
    /// Both directions between this Position and another, from the journal.
    Conversation(GatewayConversationArgs),
    /// Cross a Communique into obligation-bearing work: Factory assigns custody
    /// to its recipient Position and the Communique is marked escalated.
    Delegate(GatewayDelegateArgs),
    /// Relay every Communique whose recipient now stands on a declared remote
    /// Workcell (the gateway service also runs this every tick).
    Forward(GatewayQueryArgs),
    /// Declare, list or remove the gateway endpoints of other Workcells.
    Remote(GatewayRemoteCmd),
}

/// `aikit gateway who`.
#[derive(Debug, Args)]
pub struct GatewayWhoArgs {
    /// Project World to read (`project:O-I` or `O-I`); defaults to the World
    /// Central says this directory stands in.
    #[arg(long = "project-world", value_name = "WORLD")]
    pub project_world: Option<String>,
    #[command(flatten)]
    pub carrier: GatewayQueryArgs,
}

/// `aikit gateway send`.
#[derive(Debug, Args)]
pub struct GatewaySendArgs {
    /// Recipient Position: `central:position:<world>:<slug>` or `@handle`.
    #[arg(long, value_name = "POSITION|@HANDLE")]
    pub to: String,
    /// The words to send.
    #[arg(long, value_name = "TEXT", conflicts_with = "body_file")]
    pub body: Option<String>,
    /// Read the words from a file (`-` for stdin).
    #[arg(long = "body-file", value_name = "PATH")]
    pub body_file: Option<std::path::PathBuf>,
    /// The Communique this one answers.
    #[arg(long = "reply-to", value_name = "COMMUNIQUE")]
    pub reply_to: Option<String>,
    /// Speak as this Position when the body carries no OI_POSITION_REF.
    /// Attribution is still verified from occupancy, never taken on trust.
    #[arg(long = "from-position", value_name = "POSITION")]
    pub from_position: Option<String>,
    /// Project World an @handle is looked up in.
    #[arg(long = "project-world", value_name = "WORLD")]
    pub project_world: Option<String>,
    #[command(flatten)]
    pub carrier: GatewayQueryArgs,
}

/// `aikit gateway inbox`.
#[derive(Debug, Args)]
pub struct GatewayInboxArgs {
    /// The Position to read; defaults to this body's OI_POSITION_REF.
    #[arg(long, value_name = "POSITION")]
    pub position: Option<String>,
    /// Mark every listed Communique delivered to this body's verified
    /// occupant generation.
    #[arg(long)]
    pub ack: bool,
    #[command(flatten)]
    pub carrier: GatewayQueryArgs,
}

/// `aikit gateway conversation`.
#[derive(Debug, Args)]
pub struct GatewayConversationArgs {
    /// The other Position (`central:position:…` or `@handle`).
    #[arg(long, value_name = "POSITION|@HANDLE")]
    pub with: String,
    /// This side of the conversation; defaults to OI_POSITION_REF.
    #[arg(long, value_name = "POSITION")]
    pub position: Option<String>,
    /// Project World an @handle is looked up in.
    #[arg(long = "project-world", value_name = "WORLD")]
    pub project_world: Option<String>,
    #[command(flatten)]
    pub carrier: GatewayQueryArgs,
}

/// `aikit gateway delegate`.
#[derive(Debug, Args)]
pub struct GatewayDelegateArgs {
    /// The Communique to escalate.
    #[arg(long, value_name = "COMMUNIQUE")]
    pub communique: String,
    /// The developmental work the recipient Position takes custody of.
    #[arg(long, value_name = "WORK_REF")]
    pub work: String,
    #[arg(long, value_name = "RUN_REF")]
    pub run: Option<String>,
    #[arg(long, value_name = "JOURNEY_REF")]
    pub journey: Option<String>,
    #[arg(long = "workflow-unit", value_name = "UNIT_REF")]
    pub workflow_unit: Option<String>,
    /// Why this crossing into obligation-bearing work is made.
    #[arg(long, value_name = "TEXT")]
    pub reason: String,
    #[command(flatten)]
    pub carrier: GatewayQueryArgs,
}

#[derive(Debug, Args)]
pub struct GatewayRemoteCmd {
    #[command(subcommand)]
    pub command: GatewayRemoteSub,
}

#[derive(Debug, Subcommand)]
pub enum GatewayRemoteSub {
    /// Declare (or replace) the gateway endpoint of a remote Workcell.
    Add {
        /// The remote Workcell, e.g. `workcell:omarchy`.
        #[arg(long, value_name = "WORKCELL_REF")]
        workcell: String,
        /// Its gateway WebSocket carrier, `HOST:PORT`.
        #[arg(long = "ws", value_name = "HOST:PORT")]
        websocket_bind: String,
        #[arg(long = "ws-path", value_name = "PATH", default_value = "/")]
        websocket_path: String,
        /// Where its bearer token lives: `file:/abs/path` (owner-only) or a
        /// keychain:// / pass:// / op:// / varlock:// ref. Never the token.
        #[arg(long = "token-location", value_name = "LOCATION")]
        token_location: String,
    },
    /// List the declared endpoints (token locations only).
    List,
    /// Remove a declared endpoint.
    Remove {
        #[arg(long, value_name = "WORKCELL_REF")]
        workcell: String,
    },
}

/// Arguments for `aikit compose` — the composition reads the authored ground;
/// nothing here selects a model or harness by hand.
#[derive(Debug, Args)]
pub struct ComposeArgs {
    /// Exact source-basis JSON for an Actuation agency actualisation request.
    /// No AgentProfile is required. Source material and native authority are rechecked.
    #[arg(long, requires_all = ["agent", "world"])]
    pub agency_source: Option<std::path::PathBuf>,
    /// Stable AgentRef to enact, not a profile, model, session or display label.
    #[arg(long, requires = "agency_source")]
    pub agent: Option<String>,
    /// Explicit WorldRef of the supplied native WorldBinding.
    #[arg(long, requires = "agency_source")]
    pub world: Option<String>,
    /// Actualise the selected model through Actuation instead of only
    /// disclosing the plan. With an explicit --model that pin selects; without
    /// one, the model roster resolves the model and the ranking explanation
    /// rides the realisation.
    #[arg(long)]
    pub realise: bool,
    /// JSON file naming the existing SessionSpace, AgentSession and native owner socket.
    #[arg(long, requires = "realise")]
    pub resident_target: Option<std::path::PathBuf>,
    /// The Model to select, as a canonical `model:<stable-id>` ref.
    #[arg(long)]
    pub model: Option<String>,
    /// Pin the provider to use. A pin constrains which route is taken; it
    /// never changes which Model was selected.
    #[arg(long)]
    pub provider: Option<String>,
    /// The kind of work the model is for, when the roster resolves the model
    /// (no --model). Named in the ranking explanation.
    #[arg(long, default_value = "compose")]
    pub use_type: String,
    /// The ranking policy the roster resolves under, when no --model is
    /// given. One of: CHEAPEST_ELIGIBLE, TASK_FIT, ROLE_FIT, PROFILE_FIT,
    /// QUALITY_UNDER_BUDGET, BALANCED (default), INDEPENDENT_REVIEWER,
    /// LOCAL_INSPECTABILITY.
    #[arg(long)]
    pub ranking_policy: Option<String>,
}

#[derive(Debug, Args)]
pub struct ModelResolveArgs {
    /// The kind of work this child/model is for.
    #[arg(long, default_value = "agent-child")]
    pub use_type: String,
    /// The roster policy to apply. Prime-QL descendants normally request
    /// CHEAPEST_ELIGIBLE; other callers may explicitly choose another policy.
    #[arg(long, default_value = "CHEAPEST_ELIGIBLE")]
    pub ranking_policy: String,
}

#[derive(Debug, Args)]
pub struct ModelCatalogueCmd {
    #[command(subcommand)]
    pub command: ModelCatalogueSub,
}

#[derive(Debug, Subcommand)]
pub enum ModelCatalogueSub {
    /// Read a Provider Source's published model list into the local catalogue.
    Refresh(ModelCatalogueRefreshArgs),
    /// Show the resolved catalogue: first-party seed, Provider Sources, owner entries.
    Show(ModelCatalogueShowArgs),
}

#[derive(Debug, Args)]
pub struct ModelCatalogueRefreshArgs {
    /// Which Provider Source to read. Only `openrouter` is implemented; its
    /// model list is public and no credential is used.
    #[arg(long, default_value = "openrouter")]
    pub provider: String,
}

#[derive(Debug, Args)]
pub struct ModelCatalogueShowArgs {
    /// Only show entries whose ModelRef or name contains this text.
    #[arg(long)]
    pub filter: Option<String>,
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
    /// List registered projects exhaustively, optionally filtering by id or root.
    List(ProjectListArgs),
    /// Configure the Skill Sets inherited by Project Specifications by default.
    Defaults(ProjectDefaultsArgs),
    /// Remove a Project Specification and the AIKit-owned link it placed in
    /// each bound directory. The projects' own files are untouched.
    Unbind(ProjectIdArgs),
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
pub struct ProjectIdArgs {
    #[arg(value_name = "ID")]
    pub id: String,
}

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
    /// Bind an existing or new skill source to Central's stable directory ref.
    BindCentral(SourceBindCentralArgs),
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
    /// Remove the registration and every snapshot it owns. Refuses while the
    /// source still has an active snapshot unless `--force` names the loss;
    /// recorded trust stays as review evidence.
    Remove(SourceRemoveArgs),
}

#[derive(Debug, Args)]
pub struct SourceBindCentralArgs {
    pub id: String,
    pub source_ref: String,
    #[arg(long)]
    pub root: std::path::PathBuf,
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
pub struct SourceRemoveArgs {
    #[arg(value_name = "ID")]
    pub id: String,
    /// Remove even while the source still has an active snapshot. The reply
    /// names what was removed; projected generations keep their bytes until
    /// the next apply rebuilds without the source.
    #[arg(long)]
    pub force: bool,
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
pub struct HarnessProfileCmd {
    #[command(subcommand)]
    pub command: HarnessProfileSub,
}

#[derive(Debug, Subcommand)]
pub enum HarnessProfileSub {
    /// Validate a harness-profile TOML document against the exact schema and
    /// admission grammar the registry applies. The reply carries the decision
    /// (`admit` or `refuse`) and named diagnostics; refusal exits non-zero.
    Validate(HarnessProfileValidateArgs),
}

#[derive(Debug, Args)]
pub struct HarnessProfileValidateArgs {
    /// Path to the `aikit.harness-profile/v1` TOML document to check.
    #[arg(value_name = "FILE")]
    pub file: std::path::PathBuf,
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
    /// Export the set as a native agent package (openai | codex | claude | pi).
    /// The set stays the source; the package is a target projection of it.
    Package(crate::skillset_package_cli::SetPackageCmd),
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
    #[arg(value_name = "IDS", required_unless_present = "children")]
    pub ids: Vec<String>,
    /// Carry another set by reference (a home set name or a registry semantic
    /// ref such as `central:documentation`). Repeatable. The referenced set is
    /// shared, never copied into this set's members. `set add` only.
    #[arg(long = "child", value_name = "SET_REF")]
    pub children: Vec<String>,
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

#[derive(Debug, Args)]
pub struct FlowCmd {
    #[command(subcommand)]
    pub command: FlowSub,
}

#[derive(Debug, Subcommand)]
pub enum FlowSub {
    /// Disclose the deterministic Contemplate preflight — exactly what will
    /// be read and touched — before anything crosses the Agent/model seam.
    /// Inert: records nothing.
    Preflight(FlowContemplateArgs),
    /// One explicit Contemplate(FlowRef): preflight → Explain disclosure →
    /// record-gated execution. Contemplate is never auto-invoked: without the
    /// owner seams (or a host executor) the typed reading is `unavailable`.
    Contemplate(FlowContemplateArgs),
    /// W1.5 owner read: what changed relative to one recorded thought —
    /// changed sources, affected knowledge, unresolved — each with provenance.
    ChangedSince(FlowChangedSinceArgs),
}

#[derive(Debug, Args)]
pub struct FlowContemplateArgs {
    /// ResourceRef of the Flow node, as listed by `knowledge resolve`.
    /// Required unless `--now-ref` names the subject instead.
    #[arg(value_name = "FLOW_REF")]
    pub flow_ref: Option<String>,
    /// Address a NOW clearing's raw contemplative stream instead of a Flow:
    /// the contemplate subject is the NOW's T/T' system (Central #175).
    /// Requires `--fixtures`.
    #[arg(long, value_name = "NOW_REF")]
    pub now_ref: Option<String>,
    /// Path to the NOW's `central.thoughts-reading/v1` stream JSON, carried
    /// verbatim from Central's `central.now.thoughts.read`. The caller
    /// supplies the seam; this surface fabricates none.
    #[arg(long, value_name = "FILE")]
    pub fixtures: Option<std::path::PathBuf>,
    /// Path to a provider-neutral `KnowledgeChangeHorizon` JSON owner seam
    /// (e.g. Central's `central.source-change-horizon/v1`, adapted).
    #[arg(long, value_name = "FILE")]
    pub horizon: Option<std::path::PathBuf>,
    /// Path to a host `ModelRuntimeReadModel` JSON owner seam identifying
    /// model, Agent, Agency and AgentSession for attribution.
    #[arg(long, value_name = "FILE")]
    pub runtime: Option<std::path::PathBuf>,
}

#[derive(Debug, Args)]
pub struct FlowChangedSinceArgs {
    /// Path to a recorded `aikit.flow-cognition/v1` thought JSON (as emitted
    /// by a successful `flow contemplate`).
    #[arg(long, value_name = "FILE")]
    pub thought: std::path::PathBuf,
    /// Path to a provider-neutral `KnowledgeChangeHorizon` JSON owner seam.
    /// Without it the reading is explicitly `unavailable`, never guessed.
    #[arg(long, value_name = "FILE")]
    pub horizon: Option<std::path::PathBuf>,
}

#[derive(Debug, Subcommand)]
pub enum KnowledgeSub {
    Search(KnowledgeSearchArgs),
    /// Owner-side resolution rows: every row is a ref carrying owner,
    /// provenance and its available canonical Actions.
    Resolve(KnowledgeSearchArgs),
    /// Explicit open: resolve and read the ref, recording exactly one
    /// successful-use familiarity observation.
    Open(KnowledgeOpenArgs),
    Read(KnowledgeAddressArgs),
    Relations(KnowledgeRelationsArgs),
    /// Metadata and exact native relations, with explicit completeness limits.
    Graph(KnowledgeGraphArgs),
    Route(KnowledgeRouteArgs),
    Frame(KnowledgeRouteArgs),
    Sources(KnowledgeAddressArgs),
    Explain(KnowledgeAddressArgs),
    History(KnowledgeHistoryArgs),
    Status(KnowledgeStatusArgs),
    Forget(KnowledgeForgetCmd),
    /// Call the GitNexus-backed code lens directly, with the same provenance
    /// envelope (provider, version, tested version, drift, SourceRef, source
    /// revision, CodeReference, operation, basis) the contemplation field uses.
    Code(KnowledgeCodeCmd),
}

#[derive(Debug, Args)]
pub struct KnowledgeCodeCmd {
    #[command(subcommand)]
    pub command: KnowledgeCodeSub,
}

/// Shared repo/binding flags every `knowledge code` verb needs.
#[derive(Debug, Args)]
pub struct KnowledgeCodeRepoArgs {
    /// The Git repository this code reference/index lives in.
    #[arg(long, value_name = "DIR")]
    pub repo: std::path::PathBuf,
    /// A stable name for the index (defaults to the repo directory's name).
    #[arg(long = "repo-name", value_name = "NAME")]
    pub repo_name: Option<String>,
    /// `gitnexus` binary override.
    #[arg(long = "gitnexus-binary", value_name = "PATH")]
    pub gitnexus_binary: Option<String>,
    /// Exact source revision this reading is bound to (defaults to `git rev-parse HEAD`).
    #[arg(long, value_name = "REVISION")]
    pub revision: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum KnowledgeCodeSub {
    /// Detection status: provider, installed/tested version, drift, indexed, capabilities.
    Status(KnowledgeCodeRepoArgs),
    /// Index (or re-index) the repository.
    Index(KnowledgeCodeIndexArgs),
    /// Symbol/path search.
    Search(KnowledgeCodeSearchArgs),
    /// Symbol context.
    Context(KnowledgeCodeSymbolArgs),
    /// Upstream/downstream impact of a symbol.
    Impact(KnowledgeCodeImpactArgs),
    /// Trace a path between two symbols.
    Trace(KnowledgeCodeTraceArgs),
    /// Detect changed symbols/affected processes.
    Changes(KnowledgeCodeChangesArgs),
    /// Structural check (e.g. import cycles).
    Check(KnowledgeCodeRepoArgs),
}

#[derive(Debug, Args)]
pub struct KnowledgeCodeIndexArgs {
    #[command(flatten)]
    pub repo: KnowledgeCodeRepoArgs,
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct KnowledgeCodeSearchArgs {
    #[command(flatten)]
    pub repo: KnowledgeCodeRepoArgs,
    #[arg(value_name = "QUERY")]
    pub query: String,
    #[arg(long, default_value_t = 20)]
    pub limit: usize,
}

#[derive(Debug, Args)]
pub struct KnowledgeCodeSymbolArgs {
    #[command(flatten)]
    pub repo: KnowledgeCodeRepoArgs,
    #[arg(long, value_name = "SYMBOL")]
    pub symbol: String,
    #[arg(long, value_name = "PATH")]
    pub file: String,
    #[arg(long, value_name = "KIND")]
    pub kind: Option<String>,
}

#[derive(Debug, Args)]
pub struct KnowledgeCodeImpactArgs {
    #[command(flatten)]
    pub symbol: KnowledgeCodeSymbolArgs,
    #[arg(long, default_value = "upstream", value_name = "upstream|downstream")]
    pub direction: String,
}

#[derive(Debug, Args)]
pub struct KnowledgeCodeTraceArgs {
    #[command(flatten)]
    pub repo: KnowledgeCodeRepoArgs,
    #[arg(long = "from-symbol", value_name = "SYMBOL")]
    pub from_symbol: String,
    #[arg(long = "from-file", value_name = "PATH")]
    pub from_file: String,
    #[arg(long = "to-symbol", value_name = "SYMBOL")]
    pub to_symbol: String,
    #[arg(long = "to-file", value_name = "PATH")]
    pub to_file: String,
}

#[derive(Debug, Args)]
pub struct KnowledgeCodeChangesArgs {
    #[command(flatten)]
    pub repo: KnowledgeCodeRepoArgs,
    #[arg(
        long,
        default_value = "unstaged",
        value_name = "unstaged|staged|all|compare"
    )]
    pub scope: String,
    #[arg(long = "base-ref", value_name = "REVISION")]
    pub base_ref: Option<String>,
}

#[derive(Debug, Args)]
pub struct KnowledgeSearchArgs {
    #[arg(value_name = "QUERY", default_value = "")]
    pub query: String,
    #[arg(long, default_value_t = 50)]
    pub limit: usize,
}

#[derive(Debug, Args)]
pub struct KnowledgeOpenArgs {
    /// ResourceRef to open, as listed by `knowledge resolve`.
    #[arg(value_name = "RESOURCE")]
    pub resource: String,
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
pub struct KnowledgeGraphArgs {
    #[arg(value_name = "QUERY", default_value = "")]
    pub query: String,
    #[arg(long, default_value_t = 4096)]
    pub max_nodes: usize,
    #[arg(long, default_value_t = 16384)]
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
    /// Read or revise a live Markdown operational projection in the Agent Wiki.
    Projection(crate::wiki_projection::ProjectionCmd),
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
    /// Apply reviewed upserts to the project's canonical Agent Wiki through
    /// the maintenance contract (plan, compare-and-swap persist, readback).
    Maintenance(WikiMaintenanceArgs),
}

#[derive(Debug, Args)]
pub struct WikiMaintenanceArgs {
    /// Maintenance request JSON: `{"upserts": [WikiObject...],
    /// "human_source_proposals": [...], "observed_source_revisions":
    /// {"<source-ref>": "<revision>"}}` — the same object form the Wiki file
    /// itself uses. `-` reads the request from stdin.
    #[arg(long, value_name = "REQUEST_JSON")]
    pub request: std::path::PathBuf,
}

/// `aikit wiki-shape` — CASE 18's product surface over the QL shape
/// contract: declaring, validating and compressing `WikiConstellation`s.
/// A sibling of `wiki`, not a subcommand of it, so this case's work never
/// touches `wiki`'s own dispatch.
#[derive(Debug, Args)]
pub struct WikiShapeCmd {
    #[command(subcommand)]
    pub command: WikiShapeSub,
}

#[derive(Debug, Subcommand)]
pub enum WikiShapeSub {
    /// Write a WikiFrame carrying one QL-shaped WikiConstellation into a Wiki
    /// file. The whole Frame JSON body is read from stdin (the same
    /// convention as `wiki node create --stdin`); the structural floor
    /// (conjugate-requires-direct, shape declaration, whole-anchor law) is
    /// enforced before the write lands, not only when `validate` is run later.
    Declare(WikiShapeDeclareArgs),
    /// Validate every constellation a Wiki file holds against the pinned QL
    /// shape contract: conjugate-requires-direct, the declared shape_ref
    /// against the contract's own field, the declared grain, the return
    /// canon's ground_kind, and each member/anchor's declared node stance.
    Validate(WikiShapeValidateArgs),
    /// Compress one constellation's direct/conjugate sixfold plus its six
    /// declared generated relations through the 0 // 1 trinity (the 6+6′
    /// compression).
    Compress(WikiShapeCompressArgs),
}

#[derive(Debug, Args)]
pub struct WikiShapeDeclareArgs {
    /// The Frame ref being declared. Must match the `ref` the stdin body
    /// carries; a write never rewrites identity.
    #[arg(value_name = "FRAME_REF")]
    pub frame_ref: String,
    /// The wiki.json file to write.
    #[arg(long, value_name = "PATH")]
    pub file: std::path::PathBuf,
}

#[derive(Debug, Args)]
pub struct WikiShapeValidateArgs {
    /// The wiki.json file to read.
    #[arg(long, value_name = "PATH")]
    pub file: std::path::PathBuf,
}

#[derive(Debug, Args)]
pub struct WikiShapeCompressArgs {
    /// The constellation's whole-anchor ref.
    #[arg(value_name = "ANCHOR_REF")]
    pub anchor_ref: String,
    /// The wiki.json file to read.
    #[arg(long, value_name = "PATH")]
    pub file: std::path::PathBuf,
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
pub struct SystemArgs {}

/// The Guardian family reading takes no arguments: it discloses the whole
/// shipped family against every registered source.
#[derive(Debug, Args)]
pub struct FamilyArgs {}

/// The owner configuration contribution: a bare `oi.configuration-contribution/v1`
/// document on stdout, exactly like `system --json` (no envelope, never wrapped).
#[derive(Debug, Args)]
pub struct ConfigContributionArgs {}

#[derive(Debug, Args)]
pub struct ConfigCmd {
    #[command(subcommand)]
    pub command: ConfigSub,
}

#[derive(Debug, Subcommand)]
pub enum ConfigSub {
    /// Validate one requested value at one scope, owner-natively.
    Validate(ConfigValidateArgs),
    /// Plan one requested change; mints the idempotency anchor (plan_digest).
    Plan(ConfigPlanArgs),
    /// Apply a minted plan owner-natively; returns the receipt.
    Apply(ConfigApplyArgs),
    /// Reset a setting at a scope to the owner baseline.
    Reset(ConfigResetArgs),
}

#[derive(Debug, Args)]
pub struct ConfigValidateArgs {
    /// The setting ref (ai-kit:<section>:<key>).
    #[arg(long, value_name = "REF")]
    pub setting: String,
    /// The compact scope address (machine, project:<id>, agent-session:<id>).
    #[arg(long, value_name = "SCOPE")]
    pub scope: Option<String>,
    /// The requested value as inline JSON.
    #[arg(long, value_name = "JSON", conflicts_with = "value_file")]
    pub value: Option<String>,
    /// Read the requested value from a file, or `-` for stdin.
    #[arg(long, value_name = "PATH")]
    pub value_file: Option<String>,
}

#[derive(Debug, Args)]
pub struct ConfigPlanArgs {
    /// The setting ref (ai-kit:<section>:<key>).
    #[arg(long, value_name = "REF")]
    pub setting: String,
    /// The compact scope address (machine, project:<id>, agent-session:<id>).
    #[arg(long, value_name = "SCOPE")]
    pub scope: Option<String>,
    /// The requested value as inline JSON.
    #[arg(long, value_name = "JSON", conflicts_with = "value_file")]
    pub value: Option<String>,
    /// Read the requested value from a file, or `-` for stdin.
    #[arg(long, value_name = "PATH")]
    pub value_file: Option<String>,
}

#[derive(Debug, Args)]
pub struct ConfigApplyArgs {
    /// The plan document to apply: a path, or `-` for stdin.
    #[arg(long, value_name = "PATH")]
    pub plan_file: String,
    /// The client-minted ChangeSet id that anchors idempotent replay.
    #[arg(long, value_name = "ID")]
    pub changeset: Option<String>,
}

#[derive(Debug, Args)]
pub struct ConfigResetArgs {
    /// The setting ref (ai-kit:<section>:<key>).
    #[arg(long, value_name = "REF")]
    pub setting: String,
    /// The compact scope address (machine, project:<id>, agent-session:<id>).
    #[arg(long, value_name = "SCOPE")]
    pub scope: Option<String>,
    /// The client-minted ChangeSet id that anchors idempotent replay.
    #[arg(long, value_name = "ID")]
    pub changeset: Option<String>,
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
    /// Replace a credential's material or location; the ref stays stable.
    Rotate(CredentialRotateArgs),
    /// Mark a binding revoked so resolution and dispatch refuse it.
    Revoke(CredentialRevokeArgs),
    /// Surface candidate keys already on this machine (presence only).
    Discover(CredentialDiscoverArgs),
    /// Run one operator-invoked live check against a bound provider key.
    Verify(CredentialVerifyArgs),
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
    /// Declare where the material already lives (op://, varlock://, pass://,
    /// keychain://) instead of binding material. No secret is read or stored.
    #[arg(long = "ref", value_name = "SECRET_REF", conflicts_with_all = ["stdin", "from_env"])]
    pub declared_ref: Option<String>,
    /// Read the key from standard input (one line) and bind it into the OS
    /// secure store — for a caller that hands material over a pipe (the
    /// desktop's write-only key field). Refused when stdin is a terminal;
    /// the material never enters argv, the environment or any output.
    #[arg(long, conflicts_with = "from_env")]
    pub stdin: bool,
}

#[derive(Debug, Args)]
pub struct CredentialRotateArgs {
    #[arg(value_name = "CREDENTIAL")]
    pub credential: String,
    #[arg(long, value_name = "CONSUMER", default_value = "operator:aikit")]
    pub consumer: String,
    #[arg(long, value_name = "PURPOSE", default_value = "credential rotation")]
    pub purpose: String,
    #[arg(long, value_name = "NAME")]
    pub env_var: Option<String>,
    #[arg(long, value_name = "FILE", requires = "env_var")]
    pub project_env: Option<std::path::PathBuf>,
    /// Import fresh material from the named variable into the OS secure store.
    #[arg(long, requires = "env_var")]
    pub from_env: bool,
    /// Declare a new external location for the material (op://, varlock://…).
    #[arg(long = "ref", value_name = "SECRET_REF", conflicts_with_all = ["stdin", "from_env"])]
    pub declared_ref: Option<String>,
    /// Read fresh material from standard input (one line) into the OS secure
    /// store. Refused when stdin is a terminal.
    #[arg(long, conflicts_with = "from_env")]
    pub stdin: bool,
}

#[derive(Debug, Args)]
pub struct CredentialRevokeArgs {
    #[arg(value_name = "CREDENTIAL")]
    pub credential: String,
}

#[derive(Debug, Args)]
pub struct CredentialDiscoverArgs {
    /// An additional dotenv-shaped file to scan by name.
    #[arg(long, value_name = "FILE")]
    pub env_file: Option<std::path::PathBuf>,
}

#[derive(Debug, Args)]
pub struct CredentialVerifyArgs {
    #[arg(value_name = "CREDENTIAL")]
    pub credential: String,
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
    /// Promote one proven Method run into a ProvenMethodBasis for Routine use.
    ///
    /// The Method must be catalogue-resolved at an exact revision; the proof
    /// JSON carries the run's Activity/Return/Evidence/verification refs and
    /// the invocation_succeeded/verification_passed facts. No proof, no
    /// Routine — this is the gate, unchanged.
    Prove {
        /// The Method ref (`aikit method list` shows the ids).
        #[arg(long, value_name = "REF")]
        method: String,
        /// MethodProofInput JSON. Prefix a path with @ to read a file.
        #[arg(long = "proof-json", value_name = "JSON|@FILE")]
        proof_json: String,
    },
}

#[derive(Debug, Args)]
pub struct PraxisCmd {
    #[command(subcommand)]
    pub command: PraxisSub,
}

#[derive(Debug, Subcommand)]
pub enum PraxisSub {
    /// List catalogued Skills with their praxis form and effective state.
    List {
        /// Only this form: skill, method or methodology.
        #[arg(long, value_name = "FORM")]
        form: Option<String>,
        /// Only Skills whose name or payload contains this substring.
        filter: Option<String>,
    },
    /// Disclose an Agent's praxis (`aikit.agent-praxis-disclosure/v1`) from its
    /// Central AgentProfile, resolved SkillSets and optional activity evidence.
    Disclose {
        /// `central.agent-profile/v1` JSON (as `agent-profile.read` returns it,
        /// or wrapped in its action envelope). Prefix a path with @ or pass a path.
        #[arg(long = "profile-json", value_name = "JSON|@FILE")]
        profile_json: String,
        /// `aikit.praxis-activity/v1` evidence of what actually happened.
        #[arg(long = "activity-json", value_name = "JSON|@FILE")]
        activity_json: Option<String>,
        /// Skills selected for the current act. Repeatable.
        #[arg(long = "select", value_name = "SKILL")]
        select: Vec<String>,
    },
}

#[derive(Debug, Args)]
pub struct A2aCmd {
    #[command(subcommand)]
    pub command: A2aSub,
}

#[derive(Debug, Subcommand)]
pub enum A2aSub {
    /// Project an A2A v1.0.1 Agent Card from an `oi.agent-world-participation/v1`
    /// reading. Only publicly disclosed capabilities become card skills.
    Card {
        #[arg(long = "participation-json", value_name = "JSON|@FILE")]
        participation_json: String,
        /// The A2A interface endpoint the Agent actually serves.
        #[arg(long = "interface-url", value_name = "URL")]
        interface_url: String,
        /// Write the card here (e.g. `<site>/.well-known/agent-card.json`).
        #[arg(long, value_name = "FILE")]
        out: Option<std::path::PathBuf>,
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
// continuity
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct ContinuityCmd {
    #[command(subcommand)]
    pub command: ContinuitySub,
}

#[derive(Debug, Subcommand)]
pub enum ContinuitySub {
    /// List the star prompt-commands the active composition arms.
    Commands(ContinuityCommandsArgs),
    /// Show the context-pressure brackets in effect and this session's reading.
    Pressure(ContinuityPressureArgs),
    /// Verify that a close-out left the objects it claims to have left.
    Closeout(ContinuityCloseoutArgs),
}

#[derive(Debug, Args)]
pub struct ContinuityCommandsArgs {}

#[derive(Debug, Args)]
pub struct ContinuityPressureArgs {
    /// Read the pressure for this session id rather than for the current
    /// working directory's scope.
    #[arg(long, value_name = "SESSION")]
    pub session: Option<String>,
}

#[derive(Debug, Args)]
pub struct ContinuityCloseoutArgs {
    #[command(subcommand)]
    pub command: ContinuityCloseoutSub,
}

#[derive(Debug, Subcommand)]
pub enum ContinuityCloseoutSub {
    /// Read the carriers back and report each close-out clause.
    Verify(ContinuityCloseoutVerifyArgs),
}

#[derive(Debug, Args)]
pub struct ContinuityCloseoutVerifyArgs {
    /// The project whose NOW field to read. Defaults to the project this
    /// working directory stands in.
    #[arg(long, value_name = "PROJECT")]
    pub project: Option<String>,
    /// Only count material recorded at or after this unix timestamp, so a
    /// clause cannot be satisfied by a record from a previous session.
    #[arg(long, value_name = "UNIX_SECONDS")]
    pub since: Option<i64>,
    /// The Factory development-ledger root to check deferred work against.
    /// Defaults to the composition's `factory_ledger_root` tuning.
    #[arg(long, value_name = "PATH")]
    pub ledger_root: Option<String>,
    /// The Factory Run whose ledger to read. Defaults to the composition's
    /// `factory_run_ref` tuning.
    #[arg(long, value_name = "RUN_REF")]
    pub run: Option<String>,
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
    /// Durable lifecycle history of sessions: start/end, thinking,
    /// cancellation and permission events with stable identities.
    Lifecycle(SessionLifecycleCmd),
}

#[derive(Debug, Args)]
pub struct SessionLifecycleCmd {
    #[command(subcommand)]
    pub command: SessionLifecycleSub,
}

#[derive(Debug, Subcommand)]
pub enum SessionLifecycleSub {
    /// List sessions that have a durable lifecycle history.
    List(SessionLifecycleListArgs),
    /// The durable event history of one session.
    History(SessionLifecycleSessionArgs),
    /// The typed, schema-stamped read model of one session.
    Show(SessionLifecycleSessionArgs),
    /// Record a session-started event.
    Start(SessionLifecycleRecordArgs),
    /// Record a session-ended event.
    End(SessionLifecycleRecordArgs),
    /// Record an in-flight thinking state.
    Thinking(SessionLifecycleThinkingArgs),
    /// Record a cancellation with its reason.
    Cancel(SessionLifecycleCancelArgs),
    /// Permission events: request issued / granted / refused.
    Permission(SessionLifecyclePermissionCmd),
}

#[derive(Debug, Args)]
pub struct SessionLifecycleListArgs {}

#[derive(Debug, Args)]
pub struct SessionLifecycleSessionArgs {
    /// The session identity.
    #[arg(value_name = "SESSION")]
    pub session: aikit_core::SessionId,
}

#[derive(Debug, Args)]
pub struct SessionLifecycleRecordArgs {
    /// The session identity.
    #[arg(value_name = "SESSION")]
    pub session: aikit_core::SessionId,
    /// Stable activity identity to record; minted when omitted.
    #[arg(long, value_name = "ACTIVITY")]
    pub activity: Option<aikit_core::SessionActivityId>,
    /// Who or what originated the event.
    #[arg(long, default_value = "operator")]
    pub origin: String,
}

#[derive(Debug, Args)]
pub struct SessionLifecycleThinkingArgs {
    /// The session identity.
    #[arg(value_name = "SESSION")]
    pub session: aikit_core::SessionId,
    /// The in-flight reasoning state label.
    #[arg(long, value_name = "STATE")]
    pub state: String,
    /// Stable activity identity to record; minted when omitted.
    #[arg(long, value_name = "ACTIVITY")]
    pub activity: Option<aikit_core::SessionActivityId>,
    /// Who or what originated the event.
    #[arg(long, default_value = "operator")]
    pub origin: String,
}

#[derive(Debug, Args)]
pub struct SessionLifecycleCancelArgs {
    /// The session identity.
    #[arg(value_name = "SESSION")]
    pub session: aikit_core::SessionId,
    /// Why the session was cancelled.
    #[arg(long, value_name = "REASON")]
    pub reason: String,
    /// Stable activity identity to record; minted when omitted.
    #[arg(long, value_name = "ACTIVITY")]
    pub activity: Option<aikit_core::SessionActivityId>,
    /// Who or what originated the event.
    #[arg(long, default_value = "operator")]
    pub origin: String,
}

#[derive(Debug, Args)]
pub struct SessionLifecyclePermissionCmd {
    #[command(subcommand)]
    pub command: SessionLifecyclePermissionSub,
}

#[derive(Debug, Subcommand)]
pub enum SessionLifecyclePermissionSub {
    /// Issue a tool permission request. Prints the stable request and
    /// activity identities a correlator quotes verbatim.
    Request(SessionLifecyclePermissionRequestArgs),
    /// Grant a previously issued request.
    Grant(SessionLifecyclePermissionAnswerArgs),
    /// Refuse a previously issued request.
    Refuse(SessionLifecyclePermissionRefuseArgs),
}

#[derive(Debug, Args)]
pub struct SessionLifecyclePermissionRequestArgs {
    /// The session identity.
    #[arg(value_name = "SESSION")]
    pub session: aikit_core::SessionId,
    /// The tool the request names.
    #[arg(long, value_name = "TOOL")]
    pub tool: String,
    /// Stable activity identity the request belongs to; minted when omitted.
    #[arg(long, value_name = "ACTIVITY")]
    pub activity: Option<aikit_core::SessionActivityId>,
    /// Stable request identity to record; minted when omitted.
    #[arg(long, value_name = "REQUEST")]
    pub request: Option<aikit_core::PermissionRequestId>,
    /// Who or what originated the event.
    #[arg(long, default_value = "agent")]
    pub origin: String,
}

#[derive(Debug, Args)]
pub struct SessionLifecyclePermissionAnswerArgs {
    /// The session identity.
    #[arg(value_name = "SESSION")]
    pub session: aikit_core::SessionId,
    /// The issued request identity to answer.
    #[arg(long, value_name = "REQUEST")]
    pub request: aikit_core::PermissionRequestId,
    /// Who or what originated the event.
    #[arg(long, default_value = "operator")]
    pub origin: String,
}

#[derive(Debug, Args)]
pub struct SessionLifecyclePermissionRefuseArgs {
    /// The session identity.
    #[arg(value_name = "SESSION")]
    pub session: aikit_core::SessionId,
    /// The issued request identity to answer.
    #[arg(long, value_name = "REQUEST")]
    pub request: aikit_core::PermissionRequestId,
    /// Why the request was refused.
    #[arg(long, value_name = "REASON")]
    pub reason: String,
    /// Who or what originated the event.
    #[arg(long, default_value = "operator")]
    pub origin: String,
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
// harness / alias (ADR 0005 — the route portal)
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct HarnessCmd {
    #[command(subcommand)]
    pub command: HarnessSub,
}

#[derive(Debug, Subcommand)]
pub enum HarnessSub {
    /// Run a harness in the foreground against a declared model route.
    Run(HarnessRunArgs),
    /// Show a harness's declared auth options (`--json`), or run its declared
    /// one-shot login interactively in this terminal.
    Auth(HarnessAuthArgs),
}

/// `aikit harness auth <harness>`
#[derive(Debug, Args)]
pub struct HarnessAuthArgs {
    /// The harness to authenticate: a registry name, registered alias,
    /// catalog slug or embedded profile slug.
    #[arg(value_name = "HARNESS")]
    pub harness: String,
}

#[derive(Debug, Args)]
pub struct HarnessRunArgs {
    /// The harness to run: a catalog slug, registry name or registered alias.
    #[arg(long, value_name = "HARNESS")]
    pub harness: String,
    /// The canonical `model:<stable-id>` to route to the harness.
    #[arg(long, value_name = "MODEL_REF")]
    pub model: String,
    /// Pin the route's provider. A pin constrains the route; it never changes
    /// which Model was selected.
    #[arg(long, value_name = "PROVIDER_REF")]
    pub provider: Option<String>,
    /// Print the composed launch (argv, delivered variable names) without
    /// spawning or materialising anything.
    #[arg(long)]
    pub dry_run: bool,
    /// Arguments passed through to the harness after `--`.
    #[arg(
        value_name = "ARGS",
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    pub passthrough: Vec<String>,
}

#[derive(Debug, Args)]
pub struct AliasCmd {
    #[command(subcommand)]
    pub command: AliasSub,
}

#[derive(Debug, Subcommand)]
pub enum AliasSub {
    /// List every alias family with what each entry would run, honestly.
    List,
    /// Validate the families against the registry, profiles and catalogue.
    Check,
    /// Emit a family's launcher scripts as generated data under the AIKit home.
    Install(AliasInstallArgs),
}

#[derive(Debug, Args)]
pub struct AliasInstallArgs {
    /// The family to install (its manifest file name without .toml).
    #[arg(value_name = "FAMILY")]
    pub family: String,
    /// Write the launchers here instead of the AIKit home default.
    #[arg(long, value_name = "DIR")]
    pub out: Option<std::path::PathBuf>,
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
    /// Speak claude-code's `hookSpecificOutput.permissionDecision` JSON on
    /// stdout instead of exit codes alone. The default exit-code flavor is the
    /// common denominator of claude-code and zcode and keeps stdout empty,
    /// which zcode's strict hook-output schema requires; use this flag only
    /// where the calling harness consumes the JSON protocol.
    #[arg(long = "decision-json")]
    pub decision_json: bool,
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

/// The suite-wide version contract: the package version plus, when the build
/// stamped one, the exact source revision the binary was compiled from — so
/// any installed aikit answers what it is without its build tree.
pub fn version_line() -> &'static str {
    static LINE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    LINE.get_or_init(|| match option_env!("SUITE_BUILD_REVISION") {
        Some(revision) if !revision.is_empty() => {
            format!("{} ({revision})", env!("CARGO_PKG_VERSION"))
        }
        _ => env!("CARGO_PKG_VERSION").to_owned(),
    })
}

/// `aikit session-space` — durable SessionSpace semantics (list, show, stage,
/// apply, working surfaces, …) and the encounter owner protocol, folded into
/// the main binary from the former `aikit-session-space` companion (O-I #376:
/// `oi install` places one executable per product). Every verb keeps the
/// companion's name and JSON shape; only the invocation changed.
#[derive(Debug, Subcommand)]
pub enum SessionSpaceCommand {
    /// Internal scoped Model launch; raw credential material never enters JSON.
    EncounterModelExec {
        #[arg(long)]
        agent_session: String,
        #[arg(long)]
        provider: String,
        #[arg(long)]
        expected_model_basis: String,
    },
    /// Prepare a Central task and an exact Workcell boundary for this session.
    EncounterTaskConfigure {
        #[arg(long)]
        agent_session: String,
        #[arg(long)]
        request_json: String,
        #[arg(long)]
        expected_revision: Option<String>,
    },
    /// Read task preparation, including pending effects and native source bases.
    EncounterTaskRead {
        #[arg(long)]
        agent_session: String,
    },
    /// Internal native protocol launch; emits no wrapper bytes to stdout.
    EncounterTaskExec {
        #[arg(long)]
        agent_session: String,
        #[arg(long)]
        expected_revision: String,
    },
    /// Start the native resident owner once, independently of this CLI client.
    #[cfg(unix)]
    EncounterStart,
    /// Run the resident generic ACP owner. Client exit never stops providers.
    #[cfg(unix)]
    EncounterServe {
        #[arg(long)]
        socket: Option<std::path::PathBuf>,
    },
    /// Configure a native ACP provider. This operation is not exposed over IPC.
    EncounterConfigure {
        #[arg(long)]
        provider_json: String,
    },
    /// Provision or withdraw a native Agency binding under an exact revision.
    /// This is an owner-only operation, not gateway/IPC input.
    EncounterAgencyConfigure {
        #[arg(long)]
        agent_session: String,
        #[arg(long)]
        binding_json: String,
        #[arg(long)]
        expected_revision: Option<String>,
    },
    /// Correlate operator-reviewed native evidence for a stuck delivery; never replay it.
    EncounterDeliveryReconcile {
        #[arg(long)]
        agent_session: String,
        #[arg(long)]
        delivery_ref: String,
        #[arg(long)]
        evidence_ref: String,
        #[arg(long)]
        expected_phase: String,
    },
    /// Apply a canonical encounter action to the resident owner.
    #[cfg(unix)]
    Encounter {
        #[arg(long)]
        request_json: String,
        #[arg(long)]
        socket: Option<std::path::PathBuf>,
    },
    /// Read the current canonical Project + ContextResolution binding for typed stage intent.
    ProjectContext,
    /// List persisted SessionSpaces.
    List,
    /// Show one canonical SessionSpace semantic state.
    Show { space: String },
    /// Open persisted semantic state without claiming provider-native recovery.
    Open { space: String },
    /// Read, open, or focus one exact persisted provider working Surface.
    WorkingSurface {
        #[command(subcommand)]
        command: SessionSpaceWorkingSurfaceCommand,
    },
    /// Discover SessionSpaces, optionally by exact ProjectRef.
    Discover {
        #[arg(long)]
        project: Option<String>,
    },
    /// Stage a new SessionSpace. This is write-free and returns a preview.
    Create {
        id: String,
        #[arg(long)]
        label: Option<String>,
    },
    /// Stage any typed SessionSpace mutation from JSON. Prefix with @ to read a file.
    ///
    /// `--print-schema` prints documented JSON templates instead of staging:
    /// every mutation operation, each with the `intent` value to pass here and
    /// field notes beside it — including the complete SessionPlan template a
    /// bind-working-surface binding requires. Pass `--operation` for one.
    Stage {
        #[arg(long)]
        space: Option<String>,
        /// Print documented intent templates instead of staging.
        #[arg(long = "print-schema", default_value_t = false)]
        print_schema: bool,
        /// With --print-schema: restrict the output to one operation
        /// (kebab-case, e.g. bind-working-surface).
        #[arg(long = "operation", value_name = "OPERATION")]
        operation: Option<String>,
        #[arg(long = "intent-json", value_name = "JSON|@FILE")]
        intent_json: Option<String>,
    },
    /// Apply exactly a previously reviewed preview. Prefix with @ to read a file.
    Apply {
        #[arg(long = "preview-json", value_name = "JSON|@FILE")]
        preview_json: String,
    },
    /// Show immutable SessionSpace application receipts.
    History { space: String },
    /// Compare two receipt-backed semantic states.
    Compare {
        space: String,
        from_sequence: u64,
        to_sequence: u64,
    },
    /// Stage restoration from a prior receipt through current authority.
    RestorePreview { space: String, sequence: u64 },
    /// Reconstruct using persisted semantic state only; absent live evidence stays unavailable.
    Reconstruct { space: String },
    /// Reconcile as a read of canonical-vs-observed state; with no supplied observations this is non-mutating.
    Reconcile { space: String },
    /// Explain persisted SessionSpace state and the receipt that last changed it.
    Explain { space: String },
}

#[derive(Debug, Subcommand)]
pub enum SessionSpaceWorkingSurfaceCommand {
    /// Read the persisted binding and its current provider observation.
    Observe { space: String, binding: String },
    /// Explicitly create-or-attach the persisted provider plan for this Surface.
    Open { space: String, binding: String },
    /// Focus only the currently live persisted Surface; this never recreates it.
    Focus { space: String, binding: String },
    /// Replace this terminal client with attachment to the exact live provider Surface.
    Attach { space: String, binding: String },
}
