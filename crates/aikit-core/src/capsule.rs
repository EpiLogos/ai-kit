//! The capsule: AIKit's single packaging abstraction.
//!
//! One envelope, typed payload sections. The lifecycle (catalog, trust, scope
//! selection, explanation) is uniform; the *runtime semantics* are not — a hook
//! that gates tool calls and an alias that exports shell syntax must not pretend
//! to be interchangeable beyond search and lifecycle.
//!
//! Note two deliberate refusals encoded here:
//!
//! * A manifest may not declare its own trust. Trust lives in AIKit's database,
//!   keyed on `(registry source, capsule id, revision)`.
//! * A manifest's `kind` must agree with its id prefix, so that `kind:` search
//!   filters and registry paths can never disagree with the payload.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::arg::ArgSpec;
use crate::duration::HumanDuration;
use crate::effects::Effects;
use crate::error::{err, AikitError, Result};
use crate::id::{CapsuleId, RegistrySource, Revision};
use crate::platform::{Platform, TargetId};
use crate::profile::ConfigMerge;

pub const SUPPORTED_SCHEMA: u32 = 1;

/// What a capsule packages. `active` means something different for each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    Skill,
    Script,
    Hook,
    Guidance,
    Alias,
    Session,
    Tool,
    Template,
}

impl Kind {
    pub const ALL: [Kind; 8] = [
        Kind::Skill,
        Kind::Script,
        Kind::Hook,
        Kind::Guidance,
        Kind::Alias,
        Kind::Session,
        Kind::Tool,
        Kind::Template,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Skill => "skill",
            Kind::Script => "script",
            Kind::Hook => "hook",
            Kind::Guidance => "guidance",
            Kind::Alias => "alias",
            Kind::Session => "session",
            Kind::Tool => "tool",
            Kind::Template => "template",
        }
    }

    /// What "active" means for this kind, shown in the palette preview.
    pub fn activation_meaning(self) -> &'static str {
        match self {
            Kind::Skill => "projected into the relevant agent skill surface",
            Kind::Script => "exported into the contextual command path and boosted in search",
            Kind::Hook => "included in an event dispatcher chain",
            Kind::Guidance => "eligible for context composition and injection",
            Kind::Alias => "exported through shell integration or a generated shim",
            Kind::Session => "available to create or reconcile a session space",
            Kind::Tool => "available as a checked external dependency or wrapper",
            Kind::Template => "available to materialize into a project or task",
        }
    }

    /// Kinds whose activation changes agent behaviour, and therefore demand a
    /// reviewed revision before they may become active.
    pub fn requires_trust_to_activate(self) -> bool {
        matches!(self, Kind::Hook | Kind::Skill | Kind::Guidance)
    }

    /// Kinds that carry an executable payload.
    pub fn is_executable(self) -> bool {
        matches!(self, Kind::Script | Kind::Hook | Kind::Alias)
    }

    /// A script can be run explicitly while inactive; activation only decides
    /// ambient exposure. Hooks and skills have no such escape hatch.
    pub fn runnable_while_inactive(self) -> bool {
        matches!(self, Kind::Script | Kind::Tool | Kind::Template)
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Kind {
    type Err = AikitError;
    fn from_str(s: &str) -> Result<Self> {
        Ok(match s {
            "skill" => Kind::Skill,
            "script" => Kind::Script,
            "hook" => Kind::Hook,
            "guidance" => Kind::Guidance,
            "alias" => Kind::Alias,
            "session" => Kind::Session,
            "tool" => Kind::Tool,
            "template" => Kind::Template,
            other => {
                return err(
                    "capsule.unknown_kind",
                    format!("`{other}` is not a capsule kind"),
                )
            }
        })
    }
}

/// Maturity is a review decision. Usage may *suggest* a change; it never makes one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[derive(Default)]
pub enum Maturity {
    #[default]
    Draft,
    Candidate,
    Stable,
    Deprecated,
    Blocked,
}

impl Maturity {
    pub fn as_str(self) -> &'static str {
        match self {
            Maturity::Draft => "draft",
            Maturity::Candidate => "candidate",
            Maturity::Stable => "stable",
            Maturity::Deprecated => "deprecated",
            Maturity::Blocked => "blocked",
        }
    }

    /// A blocked capsule may never enter an effective view, whatever selects it.
    pub fn is_selectable(self) -> bool {
        !matches!(self, Maturity::Blocked)
    }
}

// ---------------------------------------------------------------------------
// Facets: `[metadata.aikit]`
// ---------------------------------------------------------------------------

/// Who consumes a capability's deliverable.
///
/// The classification is by **consumer**, not by whether the user is spoken to:
/// a grilling interview talks to the user constantly but sharpens the agent's
/// plan, so it is internal; a chart is for the user to look at, so it is external.
/// That distinction is worth naming because it is a real mechanical difference —
/// internal output flows back into the agent's context, external output has to
/// reach a screen (see [`Surface`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Facing {
    /// The deliverable feeds the agent's own continued work. The default, because
    /// most capabilities are of this shape and a manifest should not have to say so.
    #[default]
    Internal,
    /// The deliverable is for the user to look at.
    External,
    /// Faces both ways. Shared language is the case this exists for: it aligns the
    /// agent *and* teaches the user.
    Both,
}

impl Facing {
    pub fn as_str(self) -> &'static str {
        match self {
            Facing::Internal => "internal",
            Facing::External => "external",
            Facing::Both => "both",
        }
    }

    /// Whether output from this capability has to reach the user's screen.
    pub fn shows_the_user(self) -> bool {
        matches!(self, Facing::External | Facing::Both)
    }
}

/// Where an external-facing capability's output lands.
///
/// Declared rather than guessed because it genuinely varies, and because the
/// headless case must report honestly instead of pretending it showed something
/// (STANDARDS §1: no silent degradation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Surface {
    /// Renders in a marimo/Jupyter session — the reactive, native home.
    Notebook,
    /// Opens a browser tab.
    Browser,
    /// Writes a self-contained artifact and reports its path.
    ArtifactPath,
}

impl Surface {
    pub fn as_str(self) -> &'static str {
        match self {
            Surface::Notebook => "notebook",
            Surface::Browser => "browser",
            Surface::ArtifactPath => "artifact-path",
        }
    }
}

/// What kind of shared language a `guidance` capsule carries.
///
/// Shared language is the alignment contract between the agent's work and the
/// user's understanding, so it is worth being able to search and dedup by which
/// scale it operates at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LanguageFacet {
    /// Portable across codebases: a glossary, a vocabulary.
    Vocabulary,
    /// Per-module, project-scoped: what a caller must know to use this module.
    ModuleInterface,
    /// "How I do neo4j" — a portable approach to a subject.
    Approach,
}

impl LanguageFacet {
    pub fn as_str(self) -> &'static str {
        match self {
            LanguageFacet::Vocabulary => "vocabulary",
            LanguageFacet::ModuleInterface => "module-interface",
            LanguageFacet::Approach => "approach",
        }
    }
}

/// The `[metadata.aikit]` facets.
///
/// **Facets describe; they never select.** They drive search, presentation,
/// dedup discipline and surfacing. Part I rule 6 stands unchanged: nothing becomes
/// active because it carries a facet, exactly as nothing becomes active because it
/// matches a tag.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Facets {
    #[serde(default)]
    pub facing: Facing,
    #[serde(default)]
    pub surface: Option<Surface>,
    #[serde(default)]
    pub language: Option<LanguageFacet>,
}

impl Facets {
    /// Reject the combinations that are contradictions rather than configurations.
    fn validate(&self, id: &CapsuleId) -> Result<()> {
        if self.surface.is_some() && !self.facing.shows_the_user() {
            return Err(AikitError::new(
                "manifest.invalid_facet",
                format!(
                    "`{id}` declares `surface` but is `facing = \"{}\"`: a surface says where \
                     output reaches the user, which is only meaningful for a capability whose \
                     deliverable the user looks at",
                    self.facing.as_str()
                ),
            )
            .with("id", id.to_string()));
        }
        Ok(())
    }
}

/// A declared dependency on another capsule.
///
/// Deliberately an exact capsule id rather than a virtual provider: a
/// package-manager-style solver would be a large complexity purchase before real
/// usage has shown it is needed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Requirement {
    pub id: CapsuleId,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    /// An optional requirement is expanded when available and skipped otherwise.
    #[serde(default)]
    pub optional: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Conflict {
    pub id: CapsuleId,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProvenanceSource {
    Authored,
    Harvested,
    Imported,
    Generated,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Provenance {
    #[serde(default)]
    pub source: Option<ProvenanceSource>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub source_event: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub upstream: Option<String>,
}

// ---------------------------------------------------------------------------
// Kind-specific sections
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[derive(Default)]
pub enum ExecMode {
    /// Hand the terminal to the child, then optionally show a compact summary.
    #[default]
    Foreground,
    /// Keep the palette, stream stdout/stderr into a result panel.
    Capture,
    /// Tracked child process, progress via jobs/status surfaces.
    Background,
    /// Ask the mux adapter for a new pane.
    NewPane,
    /// Ask the mux adapter for a new workspace/window.
    NewView,
    /// exec-style handoff; AIKit does not return.
    Replace,
}

impl ExecMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ExecMode::Foreground => "foreground",
            ExecMode::Capture => "capture",
            ExecMode::Background => "background",
            ExecMode::NewPane => "new-pane",
            ExecMode::NewView => "new-view",
            ExecMode::Replace => "replace",
        }
    }

    /// Modes that require the palette to release the terminal first.
    pub fn releases_terminal(self) -> bool {
        matches!(self, ExecMode::Foreground | ExecMode::Replace)
    }

    pub fn needs_mux(self) -> bool {
        matches!(self, ExecMode::NewPane | ExecMode::NewView)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[derive(Default)]
pub enum WorkingDir {
    /// The resolved project root for the context.
    #[default]
    Project,
    /// Wherever the invocation happened.
    Cwd,
    /// The capsule's own payload directory.
    Capsule,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptSection {
    pub entry: String,
    #[serde(default)]
    pub interpreter: Vec<String>,
    #[serde(default)]
    pub cwd: WorkingDir,
    #[serde(default)]
    pub mode: ExecMode,
    #[serde(default)]
    pub interactive: bool,
    #[serde(default)]
    pub timeout: Option<HumanDuration>,
    /// Command names exported into the context's `bin/` directory.
    #[serde(default)]
    pub exports: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

impl ScriptSection {
    /// Exports default to the capsule's leaf name when none are declared.
    pub fn effective_exports(&self, id: &CapsuleId) -> Vec<String> {
        if self.exports.is_empty() {
            vec![id.leaf().to_string()]
        } else {
            self.exports.clone()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[derive(Default)]
pub enum SkillFormat {
    /// An existing `SKILL.md` tree is the source of truth.
    #[default]
    AgentSkill,
    /// AIKit metadata plus `instructions.md`, compiled per target.
    Aikit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[derive(Default)]
pub enum SkillActivation {
    ModelOnly,
    UserOnly,
    #[default]
    ModelOrUser,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillSection {
    #[serde(default)]
    pub format: SkillFormat,
    #[serde(default = "default_payload_root")]
    pub root: String,
    #[serde(default)]
    pub export_name: String,
    #[serde(default)]
    pub activation: SkillActivation,
    /// Tools the skill declares it will use, surfaced during review.
    #[serde(default)]
    pub tools: Vec<String>,
}

fn default_payload_root() -> String {
    "payload".to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[derive(Default)]
pub enum HookPhase {
    #[default]
    Gate,
    Transform,
    Verify,
    Inject,
    Observe,
    Capture,
}

impl HookPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            HookPhase::Gate => "gate",
            HookPhase::Transform => "transform",
            HookPhase::Verify => "verify",
            HookPhase::Inject => "inject",
            HookPhase::Observe => "observe",
            HookPhase::Capture => "capture",
        }
    }

    /// Phases whose denial stops the chain.
    pub fn can_deny(self) -> bool {
        matches!(self, HookPhase::Gate | HookPhase::Verify)
    }

    /// Observers run in a finally stage: their failure must not deny the event.
    pub fn is_terminal_stage(self) -> bool {
        matches!(self, HookPhase::Observe | HookPhase::Capture)
    }
}

/// What happens when a hook fails for a *system* reason (crash, timeout).
///
/// Kept distinct from a policy denial everywhere: conflating "the gate said no"
/// with "the gate fell over" is how a security control quietly stops working.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[derive(Default)]
pub enum FailurePolicy {
    /// Treat failure as denial.
    #[default]
    Closed,
    /// Treat failure as allow.
    Open,
    /// Allow, but record and surface a warning.
    Warn,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BypassPolicy {
    #[serde(default)]
    pub allowed: bool,
    #[serde(default)]
    pub reason_required: bool,
}

impl Default for BypassPolicy {
    fn default() -> Self {
        Self {
            allowed: false,
            reason_required: true,
        }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookSection {
    pub entry: String,
    pub events: Vec<String>,
    #[serde(default)]
    pub matcher: Option<String>,
    #[serde(default)]
    pub phase: HookPhase,
    #[serde(default)]
    pub order: i32,
    #[serde(default)]
    pub timeout: Option<HumanDuration>,
    #[serde(default)]
    pub failure: FailurePolicy,
    /// Serial by default: a capsule must opt in to parallel execution.
    #[serde(default = "default_true")]
    pub serial: bool,
    #[serde(default)]
    pub bypass: BypassPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuidanceSection {
    pub entry: String,
    #[serde(default)]
    pub inject: Vec<String>,
    #[serde(default)]
    pub order: i32,
    #[serde(default)]
    pub token_budget: Option<u32>,
    /// Guidance sharing a dedup key is injected once, highest precedence wins.
    #[serde(default)]
    pub dedup_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionSection {
    pub spec: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolSection {
    pub commands: Vec<String>,
    #[serde(default)]
    pub check: Vec<String>,
    #[serde(default)]
    pub minimum_version: Option<String>,
    /// A script capsule that can install the tool. Never run implicitly.
    #[serde(default)]
    pub install_hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AliasSection {
    pub name: String,
    pub body: String,
    /// Shells whose syntax the body is written in.
    #[serde(default)]
    pub shells: Vec<String>,
}

/// A whole instantiable unit of code — "define once, instantiate anywhere".
///
/// `destination` and every payload path and body may carry `{{param}}`
/// placeholders, substituted at instantiation from [`TemplateSection::params`].
/// The parameter type is [`ArgSpec`], the same one scripts and profiles already
/// use: a second parameter system would be a second set of validation rules to
/// keep in step, and there is nothing about a template parameter that a command
/// argument is not already.
// Not `Eq`: parameters are `ArgSpec`s, which carry `f64` bounds. `Payload` is
// likewise `PartialEq` only, so nothing downstream loses anything.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateSection {
    pub root: String,
    /// Where the instance lands, relative to the project root. May carry
    /// `{{param}}` placeholders. Absent means the project root itself.
    #[serde(default)]
    pub destination: Option<String>,
    /// Typed parameters, reusing the manifest's existing argument specs.
    #[serde(default)]
    pub params: Vec<ArgSpec>,
}

// ---------------------------------------------------------------------------
// The capsule envelope
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    schema: u32,
    id: String,
    kind: String,
    name: String,
    description: String,
    #[serde(default)]
    maturity: Maturity,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    platforms: Vec<Platform>,
    #[serde(default)]
    targets: Vec<TargetId>,
    #[serde(default)]
    requires: Vec<Requirement>,
    #[serde(default)]
    conflicts: Vec<Conflict>,
    #[serde(default)]
    effects: Effects,
    #[serde(default)]
    args: Vec<ArgSpec>,
    #[serde(default)]
    provenance: Provenance,
    /// How this capsule's `[config.*]` section combines across scopes. Deep-merge
    /// by default; a whole-record section (an MCP entry, a command spec) declares
    /// `config_merge = "replace"`.
    #[serde(default)]
    config_merge: ConfigMerge,
    /// Capabilities often used alongside this one, surfaced in the palette and the
    /// tree ("often used with…"). First-class metadata rather than a flat tag
    /// (PRIOR-ART-ACTIONS L5, from Hermes' `related_skills[]`).
    #[serde(default)]
    related_skills: Vec<CapsuleId>,
    /// `[metadata.*]`. AIKit's own facets live under `metadata.aikit`; every other
    /// namespace is carried verbatim, because dropping a neighbour's keys silently
    /// degrades every skill AIKit touches (PRIOR-ART-ACTIONS #30).
    #[serde(default)]
    metadata: BTreeMap<String, toml::Value>,

    #[serde(default)]
    script: Option<ScriptSection>,
    #[serde(default)]
    skill: Option<SkillSection>,
    #[serde(default)]
    hook: Option<HookSection>,
    #[serde(default)]
    guidance: Option<GuidanceSection>,
    #[serde(default)]
    session: Option<SessionSection>,
    #[serde(default)]
    tool: Option<ToolSection>,
    #[serde(default)]
    alias: Option<AliasSection>,
    #[serde(default)]
    template: Option<TemplateSection>,

    // Fields a capsule is explicitly forbidden from declaring. Captured so the
    // error can name them rather than emitting an opaque "unknown field".
    #[serde(default)]
    trust: Option<toml::Value>,
    #[serde(default)]
    trusted: Option<toml::Value>,
    #[serde(default)]
    revision: Option<toml::Value>,
}

/// The typed payload section for a capsule kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Payload {
    Script(ScriptSection),
    Skill(SkillSection),
    Hook(HookSection),
    Guidance(GuidanceSection),
    Session(SessionSection),
    Tool(ToolSection),
    Alias(AliasSection),
    Template(TemplateSection),
}

/// A parsed, validated capsule manifest plus the registry facts about it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Capsule {
    pub id: CapsuleId,
    pub kind: Kind,
    pub name: String,
    pub description: String,
    pub maturity: Maturity,
    pub tags: Vec<String>,
    /// Empty means "all platforms".
    pub platforms: Vec<Platform>,
    /// Empty means "all targets".
    pub targets: Vec<TargetId>,
    pub requires: Vec<Requirement>,
    pub conflicts: Vec<Conflict>,
    pub effects: Effects,
    pub args: Vec<ArgSpec>,
    pub provenance: Provenance,
    /// How this capsule's `[config.*]` section combines across scope layers.
    #[serde(default)]
    pub config_merge: ConfigMerge,
    /// Capabilities often used with this one ("often used with…"), for the palette
    /// and the tree. Never a dependency — purely advisory (PRIOR-ART-ACTIONS L5).
    #[serde(default)]
    pub related_skills: Vec<CapsuleId>,
    /// The `[metadata.aikit]` facets, parsed. Describes; never selects.
    #[serde(default)]
    pub facets: Facets,
    /// Every `[metadata.*]` namespace verbatim, including `aikit`, so unknown keys
    /// survive a round trip.
    #[serde(default)]
    pub metadata: BTreeMap<String, toml::Value>,
    pub payload: Payload,

    /// Set by the store when the capsule is loaded from disk.
    #[serde(default)]
    pub source: Option<RegistrySource>,
    #[serde(default)]
    pub revision: Option<Revision>,
    #[serde(default)]
    pub root: Option<PathBuf>,
}

impl Capsule {
    pub fn from_toml_str(src: &str) -> Result<Self> {
        let raw: RawManifest = toml::from_str(src).map_err(|e| {
            AikitError::new(
                "manifest.parse_error",
                format!("could not parse manifest: {e}"),
            )
        })?;
        Self::from_raw(raw)
    }

    fn from_raw(raw: RawManifest) -> Result<Self> {
        if raw.trust.is_some() || raw.trusted.is_some() {
            return err(
                "manifest.trust_not_self_declarable",
                "a capsule may not declare its own trust; trust is recorded by AIKit against the \
                 source registry and content revision",
            );
        }
        if raw.revision.is_some() {
            return err(
                "manifest.trust_not_self_declarable",
                "a capsule may not declare its own revision; the revision is the content hash",
            );
        }
        if raw.schema != SUPPORTED_SCHEMA {
            return Err(AikitError::new(
                "manifest.unsupported_schema",
                format!(
                    "manifest schema {} is not supported (this build understands {})",
                    raw.schema, SUPPORTED_SCHEMA
                ),
            )
            .with("id", &raw.id));
        }

        let id = CapsuleId::parse(&raw.id)?;
        let kind = Kind::from_str(&raw.kind)?;
        if kind != id.kind() {
            return Err(AikitError::new(
                "manifest.kind_mismatch",
                format!(
                    "manifest declares kind `{}` but the id `{}` says `{}`",
                    kind,
                    id,
                    id.kind()
                ),
            )
            .with("id", raw.id.clone()));
        }
        if raw.name.trim().is_empty() {
            return err("manifest.invalid", format!("`{id}` has an empty name"));
        }
        if raw.description.trim().is_empty() {
            return err(
                "manifest.invalid",
                format!(
                    "`{id}` has an empty description; the description is what makes it findable"
                ),
            );
        }

        let missing = || {
            AikitError::new(
                "manifest.missing_kind_section",
                format!("`{id}` is a {kind} capsule but has no [{kind}] section"),
            )
        };
        let payload = match kind {
            Kind::Script => Payload::Script(raw.script.clone().ok_or_else(missing)?),
            Kind::Skill => {
                let mut s = raw.skill.clone().ok_or_else(missing)?;
                if s.export_name.is_empty() {
                    s.export_name = id.leaf().to_string();
                }
                Payload::Skill(s)
            }
            Kind::Hook => Payload::Hook(raw.hook.clone().ok_or_else(missing)?),
            Kind::Guidance => Payload::Guidance(raw.guidance.clone().ok_or_else(missing)?),
            Kind::Session => Payload::Session(raw.session.clone().ok_or_else(missing)?),
            Kind::Tool => Payload::Tool(raw.tool.clone().ok_or_else(missing)?),
            Kind::Alias => Payload::Alias(raw.alias.clone().ok_or_else(missing)?),
            Kind::Template => Payload::Template(raw.template.clone().ok_or_else(missing)?),
        };

        // A capsule carrying a section for a different kind is a copy/paste bug
        // that would otherwise be silently ignored.
        let declared = [
            ("script", raw.script.is_some()),
            ("skill", raw.skill.is_some()),
            ("hook", raw.hook.is_some()),
            ("guidance", raw.guidance.is_some()),
            ("session", raw.session.is_some()),
            ("tool", raw.tool.is_some()),
            ("alias", raw.alias.is_some()),
            ("template", raw.template.is_some()),
        ];
        for (section, present) in declared {
            if present && section != kind.as_str() {
                return Err(AikitError::new(
                    "manifest.extraneous_kind_section",
                    format!("`{id}` is a {kind} capsule but also declares a [{section}] section"),
                )
                .with("id", id.to_string()));
            }
        }

        // `[metadata.aikit]` is parsed into typed facets; every other namespace is
        // carried verbatim. A misspelled facet value is refused rather than
        // silently read as the default — quietly doing less and reporting success
        // is the failure STANDARDS §1 names.
        let facets = match raw.metadata.get("aikit") {
            Some(value) => Facets::deserialize(value.clone()).map_err(|e| {
                AikitError::new(
                    "manifest.invalid_facet",
                    format!("`{id}` has an unusable [metadata.aikit] table: {e}"),
                )
                .with("id", id.to_string())
            })?,
            None => Facets::default(),
        };
        facets.validate(&id)?;

        for arg in &raw.args {
            arg.validate_spec()?;
        }
        let mut seen_names = std::collections::BTreeSet::new();
        for arg in &raw.args {
            if !seen_names.insert(arg.name.as_str()) {
                return err(
                    "manifest.invalid",
                    format!("`{id}` declares the argument `{}` twice", arg.name),
                );
            }
        }
        for req in &raw.requires {
            if req.id == id {
                return err("manifest.invalid", format!("`{id}` requires itself"));
            }
        }
        for con in &raw.conflicts {
            if con.id == id {
                return err("manifest.invalid", format!("`{id}` conflicts with itself"));
            }
        }

        Ok(Self {
            id,
            kind,
            name: raw.name,
            description: raw.description,
            maturity: raw.maturity,
            tags: raw.tags,
            platforms: raw.platforms,
            targets: raw.targets,
            requires: raw.requires,
            conflicts: raw.conflicts,
            effects: raw.effects,
            args: raw.args,
            provenance: raw.provenance,
            config_merge: raw.config_merge,
            related_skills: raw.related_skills,
            facets,
            metadata: raw.metadata,
            payload,
            source: None,
            revision: None,
            root: None,
        })
    }

    pub fn script(&self) -> Option<&ScriptSection> {
        match &self.payload {
            Payload::Script(s) => Some(s),
            _ => None,
        }
    }
    pub fn skill(&self) -> Option<&SkillSection> {
        match &self.payload {
            Payload::Skill(s) => Some(s),
            _ => None,
        }
    }
    pub fn hook(&self) -> Option<&HookSection> {
        match &self.payload {
            Payload::Hook(s) => Some(s),
            _ => None,
        }
    }
    pub fn guidance(&self) -> Option<&GuidanceSection> {
        match &self.payload {
            Payload::Guidance(s) => Some(s),
            _ => None,
        }
    }
    pub fn session(&self) -> Option<&SessionSection> {
        match &self.payload {
            Payload::Session(s) => Some(s),
            _ => None,
        }
    }
    pub fn tool(&self) -> Option<&ToolSection> {
        match &self.payload {
            Payload::Tool(s) => Some(s),
            _ => None,
        }
    }
    pub fn alias(&self) -> Option<&AliasSection> {
        match &self.payload {
            Payload::Alias(s) => Some(s),
            _ => None,
        }
    }
    pub fn template(&self) -> Option<&TemplateSection> {
        match &self.payload {
            Payload::Template(s) => Some(s),
            _ => None,
        }
    }

    /// Command names this capsule would place on the contextual PATH.
    pub fn exported_commands(&self) -> Vec<String> {
        match &self.payload {
            Payload::Script(s) => s.effective_exports(&self.id),
            Payload::Alias(a) => vec![a.name.clone()],
            _ => vec![],
        }
    }

    pub fn supports_platform(&self, platform: Platform) -> bool {
        self.platforms.is_empty() || self.platforms.contains(&platform)
    }

    pub fn supports_target(&self, target: &TargetId) -> bool {
        self.targets.is_empty() || self.targets.contains(target)
    }

    /// Text fields searched by the palette, in descending weight order.
    pub fn search_fields(&self) -> Vec<String> {
        let mut fields = self.exported_commands();
        fields.push(self.name.clone());
        fields.push(self.id.to_string());
        fields.extend(self.tags.clone());
        fields.push(self.description.clone());
        fields
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_declare_what_activation_means_for_them() {
        for kind in Kind::ALL {
            assert!(
                !kind.activation_meaning().is_empty(),
                "{kind} has no meaning"
            );
        }
    }

    #[test]
    fn behaviour_changing_kinds_require_trust_to_activate() {
        assert!(Kind::Hook.requires_trust_to_activate());
        assert!(Kind::Skill.requires_trust_to_activate());
        assert!(Kind::Guidance.requires_trust_to_activate());
        // A script is only ambiently exposed by activation; it still needs an
        // explicit confirmation to run, which is a different control.
        assert!(!Kind::Script.requires_trust_to_activate());
        assert!(Kind::Script.runnable_while_inactive());
    }

    #[test]
    fn a_capsule_with_a_section_for_another_kind_is_rejected() {
        let src = r#"
schema = 1
id = "script/test/thing"
kind = "script"
name = "Thing"
description = "Has a stray hook table."

[script]
entry = "payload/run.sh"

[hook]
entry = "payload/check"
events = ["PreToolUse"]
"#;
        assert_eq!(
            Capsule::from_toml_str(src).unwrap_err().code(),
            "manifest.extraneous_kind_section"
        );
    }

    #[test]
    fn a_skill_export_name_defaults_to_the_capsule_leaf() {
        let src = r#"
schema = 1
id = "skill/rust/code-review"
kind = "skill"
name = "Review"
description = "Reviews things."

[skill]
"#;
        let c = Capsule::from_toml_str(src).unwrap();
        assert_eq!(c.skill().unwrap().export_name, "code-review");
    }

    #[test]
    fn a_script_export_defaults_to_the_capsule_leaf() {
        let src = r#"
schema = 1
id = "script/test/cargo-nextest"
kind = "script"
name = "nextest"
description = "Runs tests."

[script]
entry = "payload/run.sh"
"#;
        let c = Capsule::from_toml_str(src).unwrap();
        assert_eq!(c.exported_commands(), vec!["cargo-nextest"]);
    }

    #[test]
    fn a_capsule_may_not_require_or_conflict_with_itself() {
        let src = r#"
schema = 1
id = "script/test/thing"
kind = "script"
name = "Thing"
description = "Self referential."

[script]
entry = "payload/run.sh"

[[requires]]
id = "script/test/thing"
"#;
        assert_eq!(
            Capsule::from_toml_str(src).unwrap_err().code(),
            "manifest.invalid"
        );
    }

    #[test]
    fn an_empty_description_is_rejected_because_it_destroys_discoverability() {
        let src = r#"
schema = 1
id = "script/test/thing"
kind = "script"
name = "Thing"
description = "   "

[script]
entry = "payload/run.sh"
"#;
        assert_eq!(
            Capsule::from_toml_str(src).unwrap_err().code(),
            "manifest.invalid"
        );
    }

    #[test]
    fn duplicate_argument_names_are_rejected() {
        let src = r#"
schema = 1
id = "script/test/thing"
kind = "script"
name = "Thing"
description = "Duplicate args."

[script]
entry = "payload/run.sh"

[[args]]
name = "path"
type = "path"

[[args]]
name = "path"
type = "string"
"#;
        assert_eq!(
            Capsule::from_toml_str(src).unwrap_err().code(),
            "manifest.invalid"
        );
    }

    #[test]
    fn blocked_capsules_are_never_selectable() {
        assert!(!Maturity::Blocked.is_selectable());
        assert!(Maturity::Deprecated.is_selectable());
    }
}
