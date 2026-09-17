//! `aikit client install|launch|status` over the real client adapters.
//!
//! Installing a client's dispatcher entries edits files AIKit does not own —
//! `~/.claude/settings.json`, a Codex hooks file — so it is a **Procedure**:
//! planned, diffed, reversible. The adapters decide *what* the edit is (they know
//! each client's config shape); this module turns that into world edits with
//! inverses and hands them to the one engine.
//!
//! ## The surface is derived, never listed
//!
//! The client surface is the registry below joined against live intake: one
//! detection run for the presence leg, one capability intake per catalog slug
//! for the contract leg. Every registered adapter gets a row from the two
//! three-state laws — a resolved descriptor is installable; a harness that is
//! present here but whose descriptor is refused is a disclosed compatibility
//! gap (the `HarnessCompatibilityGap` contract); a harness detection cannot
//! see is absent with the evidence named. An intake that cannot be read at all
//! is disclosed as unavailable — never silence, never a hard-coded roster.

use std::path::{Path, PathBuf};

use aikit_core::capsule::Kind;
use aikit_core::harness_admission::{
    unsupported_harness_gap, HarnessAdmissionAdapter, HarnessAdmissionDescriptor,
};
use aikit_core::procedure::{Inverse, Plan, Procedure, ProcedureKind, WorldEdit};
use aikit_core::projection::{ProjectionItem, ResolvedContext, TargetAdapter};
use aikit_core::{AikitError, Result, TargetId};

use aikit_adapters::actuation_harness_capability::{
    intake_actuation_capability, CapabilityOutcome, HarnessCapability,
};
use aikit_adapters::actuation_harness_detection::{
    intake_actuation_detection, DetectionEntry, DetectionOutcome, DetectionState,
};
use aikit_adapters::clients::{
    aider::AiderAdapter, antigravity::AntigravityAdapter, broker::BrokerAdapter,
    claude::ClaudeAdapter, codex::CodexAdapter, cursor::CursorAdapter, dsh::DshAdapter,
    gemini::GeminiAdapter, goose::GooseAdapter, grokbot::GrokbotAdapter, kimi::KimiAdapter,
    ollama::OllamaAdapter, openclaw::OpenclawAdapter, opencode::OpencodeAdapter, pi::PiAdapter,
    qwen::QwenAdapter, zcode::ZcodeAdapter, ClientAdapter,
};
use aikit_adapters::runner::SystemRunner;

use crate::app::Service;

/// The Actuation binary every intake asks. Resolved at spawn time; a missing
/// or refusing binary is an intake outcome, never a build-time fact.
const ACTUATION_BIN: &str = "actuation";

/// Where each client's dispatch wiring lands, decided the same way every time:
/// Actuation's capability descriptor declares the seam when it is reachable;
/// otherwise the row carries the disclosure and the legacy default path is
/// used for read models only. The broker is AIKit's own config home.
///
/// A `~/` seam is user-level; an absolute seam is taken as-is; a **relative**
/// seam (codex's per-project `.codex/hooks.json`) is a property of the working
/// tree and resolves against the project root, never against whatever
/// directory the command happened to run from.
fn client_home(seam_path: &str, tree: &Path) -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let resolved = expand_seam(seam_path, &home, tree);
    resolved.parent().map(PathBuf::from).ok_or_else(|| {
        AikitError::new(
            "client.seam_has_no_directory",
            format!("the capability seam `{seam_path}` has no parent directory"),
        )
    })
}

/// Expand one seam path: `~/` against the given home, absolute as-is,
/// relative against the working tree.
fn expand_seam(seam_path: &str, home: &Path, tree: &Path) -> PathBuf {
    let expanded = if let Some(rest) = seam_path.strip_prefix("~/") {
        home.join(rest)
    } else {
        PathBuf::from(seam_path)
    };
    if expanded.is_absolute() {
        expanded
    } else {
        tree.join(expanded)
    }
}

/// Everything the adapter factories need, computed once per command.
struct ClientDirs {
    /// The context's projection root (adapter projections live under it).
    ctx_dir: PathBuf,
    /// The working tree (codex's per-project seam resolves against it).
    tree: PathBuf,
    /// The user home (`~/` seams and the broker's config home).
    home: PathBuf,
}

fn client_dirs(service: &Service) -> ClientDirs {
    ClientDirs {
        ctx_dir: service.context_projection_root(),
        tree: service
            .descriptor()
            .project_root
            .clone()
            .unwrap_or_else(|| PathBuf::from(".")),
        home: std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(".")),
    }
}

/// Where an admitted adapter's projection seam sits under the context root —
/// the same placement the context-effect surface uses.
fn projection_dir(dirs: &ClientDirs, name: &str) -> PathBuf {
    dirs.ctx_dir.join("projections").join(name)
}

/// What a row counts as its semantic items — the plan draws from these, and
/// the count must not silently change meaning per client.
enum SemanticBasis {
    /// Active Skill capsules projected natively by this adapter.
    Skills,
    /// No native projection: the honest count is zero, not the broker's set.
    None,
    /// The broker's whole active view.
    AllActive,
}

/// One harness adapter builder: the client dirs in, a live adapter plus its
/// storage path out.
type AdapterBuild = fn(&ClientDirs) -> Result<(Box<dyn ClientAdapter>, PathBuf)>;
type CapabilityAdapterBuild =
    fn(&ClientDirs, Option<HarnessCapability>) -> Result<(Box<dyn ClientAdapter>, PathBuf)>;

/// How AIKit reaches this harness.
enum Reach {
    /// AIKit's own client: the config home is AIKit's, and no Actuation
    /// descriptor is needed or consulted.
    SelfOwned { build: AdapterBuild },
    /// A dispatch client: launch and install ride the descriptor's seam when
    /// one resolved, and the adapter's default home is the read-model fallback
    /// when it did not.
    Client { build: CapabilityAdapterBuild },
    /// Admitted through the harness-adapter contract only: there is no launch
    /// or install seam yet, and `client install|launch` says so rather than
    /// pretending the harness is unknown.
    AdapterOnly {
        build: fn(&ClientDirs) -> Box<dyn TargetAdapter>,
    },
}

/// One registered harness on the client surface. The registry is the single
/// source of the roster: `status` enumerates it, `adapter_for` dispatches
/// through it, and neither keeps a second list.
struct RegisteredClient {
    /// The CLI-facing name (`aikit client status <name>`).
    name: &'static str,
    /// Other names accepted for the same entry.
    aliases: &'static [&'static str],
    /// The Actuation catalog slug the capability and detection intakes ask
    /// for. `None` only for the broker, which is AIKit's own.
    catalog_slug: Option<&'static str>,
    semantic: SemanticBasis,
    reach: Reach,
    /// The evidence-backed admission census, used to disclose compatibility
    /// gaps. `None` only for the broker, which needs no admission.
    admission: Option<fn(&ClientDirs) -> HarnessAdmissionDescriptor>,
}

static REGISTRY: &[RegisteredClient] = &[
    RegisteredClient {
        name: "claude",
        aliases: &["claude-code"],
        catalog_slug: Some(TargetId::CLAUDE_CODE),
        semantic: SemanticBasis::Skills,
        reach: Reach::Client {
            build: |dirs, capability| match capability {
                Some(capability) => {
                    let config_dir = client_home(&capability.install_seam.config_path, &dirs.tree)?;
                    Ok((
                        Box::new(
                            ClaudeAdapter::new(dirs.ctx_dir.clone()).with_capability(capability),
                        ) as Box<dyn ClientAdapter>,
                        config_dir,
                    ))
                }
                None => Ok((
                    Box::new(ClaudeAdapter::new(dirs.ctx_dir.clone())) as Box<dyn ClientAdapter>,
                    dirs.home.join(".claude"),
                )),
            },
        },
        admission: Some(|dirs| ClaudeAdapter::new(dirs.ctx_dir.clone()).admission()),
    },
    RegisteredClient {
        name: "codex",
        aliases: &[],
        catalog_slug: Some(TargetId::CODEX),
        semantic: SemanticBasis::Skills,
        reach: Reach::Client {
            build: |dirs, capability| match capability {
                Some(capability) => {
                    let config_dir = client_home(&capability.install_seam.config_path, &dirs.tree)?;
                    Ok((
                        Box::new(CodexAdapter::new(dirs.tree.clone()).with_capability(capability))
                            as Box<dyn ClientAdapter>,
                        config_dir,
                    ))
                }
                None => Ok((
                    Box::new(CodexAdapter::new(dirs.tree.clone())) as Box<dyn ClientAdapter>,
                    dirs.home.join(".codex"),
                )),
            },
        },
        admission: Some(|dirs| CodexAdapter::new(dirs.tree.clone()).admission()),
    },
    RegisteredClient {
        name: "zcode",
        aliases: &[],
        catalog_slug: Some(TargetId::ZCODE),
        semantic: SemanticBasis::None,
        reach: Reach::Client {
            build: |dirs, capability| match capability {
                Some(capability) => {
                    let config_dir = client_home(&capability.install_seam.config_path, &dirs.tree)?;
                    Ok((
                        Box::new(ZcodeAdapter::new().with_capability(capability))
                            as Box<dyn ClientAdapter>,
                        config_dir,
                    ))
                }
                None => Ok((
                    Box::new(ZcodeAdapter::new()) as Box<dyn ClientAdapter>,
                    dirs.home.join(".zcode/cli"),
                )),
            },
        },
        admission: Some(|_dirs| ZcodeAdapter::new().admission()),
    },
    RegisteredClient {
        name: "broker",
        aliases: &[],
        catalog_slug: None,
        semantic: SemanticBasis::AllActive,
        reach: Reach::SelfOwned {
            build: |dirs| {
                Ok((
                    Box::new(BrokerAdapter::new()) as Box<dyn ClientAdapter>,
                    dirs.home.join(".aikit"),
                ))
            },
        },
        admission: None,
    },
    RegisteredClient {
        name: TargetId::AIDER,
        aliases: &[],
        catalog_slug: Some(TargetId::AIDER),
        semantic: SemanticBasis::None,
        reach: Reach::AdapterOnly {
            build: |dirs| Box::new(AiderAdapter::new(projection_dir(dirs, "aider"))),
        },
        admission: Some(|dirs| AiderAdapter::new(projection_dir(dirs, "aider")).admission()),
    },
    RegisteredClient {
        name: TargetId::ANTIGRAVITY,
        aliases: &["antigravity"],
        catalog_slug: Some(TargetId::ANTIGRAVITY),
        semantic: SemanticBasis::None,
        reach: Reach::AdapterOnly {
            build: |dirs| Box::new(AntigravityAdapter::new(projection_dir(dirs, "antigravity"))),
        },
        admission: Some(|dirs| {
            AntigravityAdapter::new(projection_dir(dirs, "antigravity")).admission()
        }),
    },
    RegisteredClient {
        name: TargetId::CURSOR_CLI,
        aliases: &["cursor"],
        catalog_slug: Some(TargetId::CURSOR_CLI),
        semantic: SemanticBasis::None,
        reach: Reach::AdapterOnly {
            build: |dirs| Box::new(CursorAdapter::new(projection_dir(dirs, "cursor"))),
        },
        admission: Some(|dirs| CursorAdapter::new(projection_dir(dirs, "cursor")).admission()),
    },
    RegisteredClient {
        name: TargetId::DEEPSEEK_HARNESS,
        aliases: &["dsh"],
        catalog_slug: Some(TargetId::DEEPSEEK_HARNESS),
        semantic: SemanticBasis::None,
        reach: Reach::AdapterOnly {
            build: |dirs| Box::new(DshAdapter::new(projection_dir(dirs, "dsh"))),
        },
        admission: Some(|dirs| DshAdapter::new(projection_dir(dirs, "dsh")).admission()),
    },
    RegisteredClient {
        name: TargetId::GEMINI_CLI,
        aliases: &["gemini"],
        catalog_slug: Some(TargetId::GEMINI_CLI),
        semantic: SemanticBasis::None,
        reach: Reach::AdapterOnly {
            build: |dirs| Box::new(GeminiAdapter::new(projection_dir(dirs, "gemini"))),
        },
        admission: Some(|dirs| GeminiAdapter::new(projection_dir(dirs, "gemini")).admission()),
    },
    RegisteredClient {
        name: TargetId::GOOSE,
        aliases: &[],
        catalog_slug: Some(TargetId::GOOSE),
        semantic: SemanticBasis::None,
        reach: Reach::AdapterOnly {
            build: |dirs| Box::new(GooseAdapter::new(projection_dir(dirs, "goose"))),
        },
        admission: Some(|dirs| GooseAdapter::new(projection_dir(dirs, "goose")).admission()),
    },
    RegisteredClient {
        name: TargetId::GROK_BOT,
        aliases: &["grokbot"],
        catalog_slug: Some(TargetId::GROK_BOT),
        semantic: SemanticBasis::None,
        reach: Reach::AdapterOnly {
            build: |dirs| Box::new(GrokbotAdapter::new(projection_dir(dirs, "grokbot"))),
        },
        admission: Some(|dirs| GrokbotAdapter::new(projection_dir(dirs, "grokbot")).admission()),
    },
    RegisteredClient {
        name: TargetId::KIMI,
        aliases: &[],
        catalog_slug: Some(TargetId::KIMI),
        semantic: SemanticBasis::None,
        reach: Reach::AdapterOnly {
            build: |dirs| Box::new(KimiAdapter::new(projection_dir(dirs, "kimi"))),
        },
        admission: Some(|dirs| KimiAdapter::new(projection_dir(dirs, "kimi")).admission()),
    },
    RegisteredClient {
        name: TargetId::OPENCODE,
        aliases: &[],
        catalog_slug: Some(TargetId::OPENCODE),
        semantic: SemanticBasis::None,
        reach: Reach::AdapterOnly {
            build: |dirs| Box::new(OpencodeAdapter::new(projection_dir(dirs, "opencode"))),
        },
        admission: Some(|dirs| OpencodeAdapter::new(projection_dir(dirs, "opencode")).admission()),
    },
    RegisteredClient {
        name: TargetId::OPENCLAW,
        aliases: &[],
        catalog_slug: Some(TargetId::OPENCLAW),
        semantic: SemanticBasis::None,
        reach: Reach::AdapterOnly {
            build: |dirs| Box::new(OpenclawAdapter::new(projection_dir(dirs, "openclaw"))),
        },
        admission: Some(|dirs| OpenclawAdapter::new(projection_dir(dirs, "openclaw")).admission()),
    },
    RegisteredClient {
        name: TargetId::PI,
        aliases: &[],
        catalog_slug: Some(TargetId::PI),
        semantic: SemanticBasis::Skills,
        reach: Reach::AdapterOnly {
            build: |dirs| Box::new(PiAdapter::new(projection_dir(dirs, "pi"))),
        },
        admission: Some(|dirs| PiAdapter::new(projection_dir(dirs, "pi")).admission()),
    },
    RegisteredClient {
        name: TargetId::QWEN_CODE,
        aliases: &[],
        catalog_slug: Some(TargetId::QWEN_CODE),
        semantic: SemanticBasis::None,
        reach: Reach::AdapterOnly {
            build: |dirs| Box::new(QwenAdapter::new(projection_dir(dirs, "qwen"))),
        },
        admission: Some(|dirs| QwenAdapter::new(projection_dir(dirs, "qwen")).admission()),
    },
    RegisteredClient {
        name: TargetId::OLLAMA,
        aliases: &[],
        catalog_slug: Some(TargetId::OLLAMA),
        semantic: SemanticBasis::None,
        reach: Reach::AdapterOnly {
            build: |dirs| Box::new(OllamaAdapter::new(projection_dir(dirs, "ollama"))),
        },
        admission: Some(|dirs| OllamaAdapter::new(projection_dir(dirs, "ollama")).admission()),
    },
];

fn lookup(client: &str) -> Option<&'static RegisteredClient> {
    REGISTRY
        .iter()
        .find(|entry| entry.name == client || entry.aliases.contains(&client))
}

fn unknown_client_error(client: &str) -> AikitError {
    let names = REGISTRY
        .iter()
        .map(|entry| entry.name)
        .collect::<Vec<_>>()
        .join(", ");
    AikitError::new(
        "client.unknown",
        format!(
            "`{client}` is not a client AIKit knows; the registered client surface is: {names}"
        ),
    )
    .with("client", client.to_string())
}

fn not_dispatchable(entry: &RegisteredClient) -> AikitError {
    AikitError::new(
        "client.not_dispatchable",
        format!(
            "`{}` is a registered harness with no launch or install seam; \
             `aikit client install|launch` reaches dispatch clients only",
            entry.name
        ),
    )
    .with("client", entry.name.to_string())
}

/// One harness's leg of the detection record, or the disclosure for why
/// there is none. Detection's three-state law, carried across unchanged.
#[derive(Debug, Clone)]
enum DetectionLeg {
    Detected {
        /// The harness's config home as the detection probes observed it
        /// (a `~/` spec), when one was probed.
        config_dir: Option<String>,
    },
    NotInstalled,
    EntryUnavailable {
        reason: Option<String>,
    },
    /// The record ran but names no entry for this slug.
    AbsentFromRecord,
    /// The detection run itself could not be read; absence cannot be claimed.
    RunUnavailable {
        reason: String,
    },
    /// The broker: AIKit's own client, outside detection's law.
    SelfOwned,
}

fn detection_leg(detection: &DetectionOutcome, entry: &RegisteredClient) -> DetectionLeg {
    if matches!(entry.reach, Reach::SelfOwned { .. }) {
        return DetectionLeg::SelfOwned;
    }
    let Some(slug) = entry.catalog_slug else {
        return DetectionLeg::SelfOwned;
    };
    match detection {
        DetectionOutcome::Unavailable { reason } => DetectionLeg::RunUnavailable {
            reason: reason.clone(),
        },
        DetectionOutcome::Record(record) => {
            match record.harnesses.iter().find(|e| e.slug == slug) {
                None => DetectionLeg::AbsentFromRecord,
                Some(recorded) => match recorded.state {
                    DetectionState::Detected => DetectionLeg::Detected {
                        config_dir: detection_config_dir(recorded),
                    },
                    DetectionState::NotInstalled => DetectionLeg::NotInstalled,
                    DetectionState::Unavailable => DetectionLeg::EntryUnavailable {
                        reason: recorded.unavailable_reason.clone(),
                    },
                },
            }
        }
    }
}

/// The config home a passing `config-dir` probe observed, when one exists.
fn detection_config_dir(recorded: &DetectionEntry) -> Option<String> {
    recorded
        .probes
        .iter()
        .flatten()
        .find(|probe| probe.kind == "config-dir" && probe.result == "pass")
        .and_then(|probe| probe.spec.clone())
}

/// The derived surface state, from the two intake legs. Capability resolved
/// means the install leg is satisfiable; a harness that is present while its
/// descriptor is refused is a compatibility gap, not an error; detection's
/// absence evidence is honoured; an unreadable intake is disclosed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SurfaceKind {
    Installable,
    Gap,
    Absent,
    Unavailable,
    SelfOwned,
}

fn derive_surface_kind(capability: Option<&CapabilityOutcome>, leg: &DetectionLeg) -> SurfaceKind {
    match capability {
        Some(CapabilityOutcome::Descriptor(_)) => SurfaceKind::Installable,
        Some(CapabilityOutcome::Unavailable { .. }) => match leg {
            DetectionLeg::Detected { .. } => SurfaceKind::Gap,
            DetectionLeg::NotInstalled | DetectionLeg::AbsentFromRecord => SurfaceKind::Absent,
            DetectionLeg::EntryUnavailable { .. } | DetectionLeg::RunUnavailable { .. } => {
                SurfaceKind::Unavailable
            }
            DetectionLeg::SelfOwned => SurfaceKind::SelfOwned,
        },
        None => SurfaceKind::SelfOwned,
    }
}

/// The adapter plus its configuration home. `capability` is `None` when
/// Actuation's descriptor is unreachable — readable, but not installable.
///
/// Dispatch comes from the one registry: a name this surface does not register
/// is unknown, and a registered harness without a dispatch seam is told apart
/// from an unknown name.
fn adapter_for(
    service: &Service,
    client: &str,
) -> Result<(Box<dyn ClientAdapter>, Option<HarnessCapability>, PathBuf)> {
    let entry = lookup(client).ok_or_else(|| unknown_client_error(client))?;
    let dirs = client_dirs(service);
    let capability = match entry.catalog_slug {
        Some(slug) => {
            match intake_actuation_capability(&SystemRunner::new(), ACTUATION_BIN, slug) {
                CapabilityOutcome::Descriptor(capability) => Some(*capability),
                CapabilityOutcome::Unavailable { .. } => None,
            }
        }
        None => None,
    };
    match entry.reach {
        Reach::SelfOwned { build } => {
            let (adapter, config_dir) = build(&dirs)?;
            Ok((adapter, None, config_dir))
        }
        Reach::Client { build } => {
            let (adapter, config_dir) = build(&dirs, capability.clone())?;
            Ok((adapter, capability, config_dir))
        }
        Reach::AdapterOnly { .. } => Err(not_dispatchable(entry)),
    }
}

/// Plan the install as a Procedure.
pub fn plan_install(service: &Service, client: &str) -> Result<Procedure> {
    let entry = lookup(client).ok_or_else(|| unknown_client_error(client))?;
    let (adapter, capability, config_dir) = adapter_for(service, client)?;
    // The law is unchanged: AIKit installs only what Actuation declares the
    // harness to be. The broker is the one exception, because AIKit owns its
    // config home and needs no descriptor for it.
    if capability.is_none() && !matches!(entry.reach, Reach::SelfOwned { .. }) {
        return Err(AikitError::new(
            "client.capability_unavailable",
            format!(
                "cannot install for {client}: Actuation's capability descriptor is unreachable, \
                 and AIKit installs only what Actuation declares the harness to be"
            ),
        )
        .with("client", client.to_string()));
    }
    let items = adapter.install(&config_dir)?;

    let mut plan = Plan::new().with_note(format!(
        "install AIKit's {client} integration into {}",
        config_dir.display()
    ));
    for item in items {
        match item {
            ProjectionItem::Write { path, contents } => {
                let target = config_dir.join(&path);
                // The adapter has already merged with whatever was there, so the
                // inverse is restoring the previous bytes — or removing the file
                // when there were none.
                let inverse = if target.exists() {
                    Inverse::Restore {
                        blob: aikit_core::procedure::BlobId::deferred(),
                    }
                } else {
                    Inverse::Remove
                };
                plan = plan.with_edit(WorldEdit::WriteFile {
                    path: target,
                    contents: contents.into_bytes(),
                    inverse,
                });
            }
            // An install emits configuration, never payload links.
            other => {
                return Err(AikitError::new(
                    "client.unexpected_install_item",
                    format!(
                    "the {client} adapter asked for an install item AIKit cannot stage: {other:?}"
                ),
                ))
            }
        }
    }

    if plan.is_empty() {
        return Err(AikitError::new(
            "client.nothing_to_install",
            format!("the {client} adapter needs no durable configuration"),
        )
        .with("client", client.to_string()));
    }
    aikit_store::procedure::plan_procedure(
        service.home(),
        ProcedureKind::ClientInstall {
            client: aikit_core::TargetId::new(client),
        },
        plan,
    )
}

/// The argv that starts a client against this context's projection.
pub fn launch_command(service: &Service, client: &str) -> Result<Vec<String>> {
    let (adapter, _, _) = adapter_for(service, client)?;
    let rc = service.projection_context()?;
    let argv = adapter.launch_command(&rc);
    if argv.is_empty() {
        return Err(AikitError::new(
            "client.not_launchable",
            format!("{client} is reached through another client, so there is no command to run"),
        )
        .with("client", client.to_string()));
    }
    Ok(argv)
}

/// What each registered client's derived surface says: the two intake legs
/// (capability descriptor, detection), the lower-level materialisation work
/// required to realise its projection, and whether the client is installed.
///
/// Every registered adapter gets a row — installable, gap, absent or
/// unavailable, each with its intake evidence — so a harness can never be
/// missing from the surface while its descriptor resolves.
///
/// `items` deliberately counts selected semantic resources, not filesystem
/// operations. A managed actor bootstrap can add a second generated projection
/// item for one selected Skill; reporting that as "2 items" makes a correctly
/// Skill-Set-filtered projection look as though it leaked another capability.
/// `materialization_items` exposes the adapter plan count separately for callers
/// interested in the physical work.
pub fn status(service: &Service, only: Option<&str>) -> Result<Vec<serde_json::Value>> {
    let rc = service.projection_context()?;
    let dirs = client_dirs(service);
    let detection = intake_actuation_detection(&SystemRunner::new(), ACTUATION_BIN);
    let mut rows = Vec::new();
    for entry in REGISTRY {
        if let Some(only) = only {
            if only != entry.name && !entry.aliases.contains(&only) {
                continue;
            }
        }
        let capability = entry
            .catalog_slug
            .map(|slug| intake_actuation_capability(&SystemRunner::new(), ACTUATION_BIN, slug));
        rows.push(client_row(entry, &rc, &dirs, &detection, capability)?);
    }
    Ok(rows)
}

/// Derive one registry entry's row from its intake outcomes.
fn client_row(
    entry: &'static RegisteredClient,
    rc: &ResolvedContext,
    dirs: &ClientDirs,
    detection: &DetectionOutcome,
    capability: Option<CapabilityOutcome>,
) -> Result<serde_json::Value> {
    let leg = detection_leg(detection, entry);
    let kind = derive_surface_kind(capability.as_ref(), &leg);

    // The adapter for planning, and the config home the row reports. The
    // descriptor's seam wins when it resolved; the detection probe is the
    // read-model fallback for adapter-only harnesses; the adapter's default
    // home is the dispatch clients' fallback.
    let (adapter, config_dir): (Box<dyn TargetAdapter>, Option<PathBuf>) = match entry.reach {
        Reach::SelfOwned { build } => {
            let (adapter, config_dir) = build(dirs)?;
            (adapter as Box<dyn TargetAdapter>, Some(config_dir))
        }
        Reach::Client { build } => {
            let resolved = match &capability {
                Some(CapabilityOutcome::Descriptor(capability)) => Some((**capability).clone()),
                _ => None,
            };
            let (adapter, config_dir) = build(dirs, resolved)?;
            (adapter as Box<dyn TargetAdapter>, Some(config_dir))
        }
        Reach::AdapterOnly { build } => {
            let config_dir = match &capability {
                Some(CapabilityOutcome::Descriptor(capability)) => Some(client_home(
                    &capability.install_seam.config_path,
                    &dirs.tree,
                )?),
                _ => leg
                    .detected_config_dir()
                    .map(|spec| expand_seam(&spec, &dirs.home, &dirs.tree)),
            };
            (build(dirs), config_dir)
        }
    };

    let planned = adapter.plan(rc);
    let semantic_items = match entry.semantic {
        SemanticBasis::Skills => rc.view.active_of_kind(Kind::Skill).len(),
        SemanticBasis::None => 0,
        SemanticBasis::AllActive => rc.view.active.len(),
    };
    let mut notes = planned
        .as_ref()
        .map(|p| p.notes.clone())
        .unwrap_or_default();

    let (state, gap) = match kind {
        SurfaceKind::Installable => ("installable", None),
        SurfaceKind::Absent => ("absent", None),
        SurfaceKind::Unavailable => ("unavailable", None),
        SurfaceKind::SelfOwned => ("self", None),
        SurfaceKind::Gap => ("gap", Some(gap_disclosure(entry, dirs, &mut notes))),
    };

    let (capability_name, capability_reason) = match &capability {
        None => ("self", None),
        Some(CapabilityOutcome::Descriptor(_)) => ("descriptor", None),
        Some(CapabilityOutcome::Unavailable { reason }) => ("unavailable", Some(reason.clone())),
    };
    let (detection_name, detection_reason) = match &leg {
        DetectionLeg::Detected { .. } => ("detected", None),
        DetectionLeg::NotInstalled => ("not-installed", None),
        DetectionLeg::EntryUnavailable { reason } => ("unavailable", reason.clone()),
        DetectionLeg::AbsentFromRecord => (
            "absent-from-record",
            Some(format!(
                "the detection record names no entry for slug {}",
                entry.catalog_slug.unwrap_or(entry.name)
            )),
        ),
        DetectionLeg::RunUnavailable { reason } => ("unavailable", Some(reason.clone())),
        DetectionLeg::SelfOwned => ("self", None),
    };
    let dispatch_name = match entry.reach {
        Reach::SelfOwned { .. } => "self",
        Reach::Client { .. } => "client",
        Reach::AdapterOnly { .. } => "adapter-only",
    };

    Ok(serde_json::json!({
        "client": entry.name,
        "harness": entry.catalog_slug,
        "state": state,
        "dispatch": dispatch_name,
        "config_dir": config_dir.as_ref().map(|d| d.display().to_string()),
        "installed": config_dir.as_ref().map(|d| d.exists()),
        "effect": planned.as_ref().ok().map(|p| adapter.activation_effect(None, p).describe()),
        "items": planned.as_ref().ok().map(|_| semantic_items),
        "materialization_items": planned.as_ref().ok().map(|p| p.items.len()),
        "actor_bootstrap": rc.actor_bootstrap.is_some(),
        "capability": capability_name,
        "capability_reason": capability_reason,
        "detection": detection_name,
        "detection_reason": detection_reason,
        "gap": gap,
        "notes": notes,
        "error": planned.as_ref().err().map(|e| e.message().to_string()),
    }))
}

/// The compatibility-gap disclosure for a harness that is present here while
/// its capability descriptor is refused: the harness-adapter contract's own
/// structured gap, built from the adapter's evidence-backed admission census.
/// A census that cannot be built is disclosed in the notes, never silently
/// dropped.
fn gap_disclosure(
    entry: &'static RegisteredClient,
    dirs: &ClientDirs,
    notes: &mut Vec<String>,
) -> Option<serde_json::Value> {
    let Some(admission) = entry.admission else {
        notes.push(format!(
            "no admission census is registered for {}; the gap is disclosed by its intake reasons alone",
            entry.name
        ));
        return None;
    };
    let descriptor = admission(dirs);
    match unsupported_harness_gap(
        descriptor.target.clone(),
        descriptor.product.clone(),
        descriptor.edition,
        descriptor.native_version.clone(),
        descriptor.faculties.clone(),
    ) {
        Ok(gap) => match serde_json::to_value(&gap) {
            Ok(value) => Some(value),
            Err(error) => {
                notes.push(format!(
                    "the compatibility-gap disclosure could not be serialised: {error}"
                ));
                None
            }
        },
        Err(error) => {
            notes.push(format!(
                "the compatibility-gap disclosure could not be built: {}",
                error.message()
            ));
            None
        }
    }
}

impl DetectionLeg {
    fn detected_config_dir(&self) -> Option<String> {
        match self {
            DetectionLeg::Detected { config_dir } => config_dir.clone(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_adapters::actuation_harness_detection::ActuationDetectionRecord;

    fn unavailable(reason: &str) -> Option<CapabilityOutcome> {
        Some(CapabilityOutcome::Unavailable {
            reason: reason.to_string(),
        })
    }

    fn detected_leg() -> DetectionLeg {
        DetectionLeg::Detected {
            config_dir: Some("~/.pi/agent".to_string()),
        }
    }

    #[test]
    fn capability_resolved_is_installable_regardless_of_detection() {
        for leg in [
            DetectionLeg::Detected { config_dir: None },
            DetectionLeg::NotInstalled,
            DetectionLeg::AbsentFromRecord,
            DetectionLeg::RunUnavailable {
                reason: "no bin".into(),
            },
        ] {
            assert_eq!(
                derive_surface_kind(Some(&descriptor_outcome()), &leg),
                SurfaceKind::Installable,
                "a resolved descriptor is the install leg, whatever detection saw"
            );
        }
    }

    fn descriptor_outcome() -> CapabilityOutcome {
        CapabilityOutcome::Descriptor(Box::new(HarnessCapability {
            schema: "actuation.harness-capability/v1".into(),
            document: "capability".into(),
            harness_slug: "pi".into(),
            summary: None,
            native_events: Vec::new(),
            injection_channel:
                aikit_adapters::actuation_harness_capability::CapabilityInjectionChannel {
                    kind: "hooks".into(),
                    mechanism: "settings".into(),
                    notes: None,
                },
            blocking_semantics:
                aikit_adapters::actuation_harness_capability::CapabilityBlockingSemantics {
                    kind: "exit-code".into(),
                    notes: None,
                },
            wake_capability: aikit_adapters::actuation_harness_capability::CapabilityWake {
                kind: "none".into(),
                notes: None,
            },
            install_seam: aikit_adapters::actuation_harness_capability::CapabilitySeam {
                config_path: "~/.pi/agent/settings.json".into(),
                format: "json".into(),
                entry_shape: "object".into(),
                ownership_marker: "aikit".into(),
                preserves_foreign_entries: true,
            },
            uninstall_seam: aikit_adapters::actuation_harness_capability::CapabilitySeam {
                config_path: "~/.pi/agent/settings.json".into(),
                format: "json".into(),
                entry_shape: "object".into(),
                ownership_marker: "aikit".into(),
                preserves_foreign_entries: true,
            },
            provenance: aikit_adapters::actuation_harness_capability::CapabilityProvenance {
                authored_by: "test".into(),
                source_refs: None,
                catalog_revision: Some(7),
            },
        }))
    }

    #[test]
    fn capability_refused_while_detected_is_a_gap() {
        assert_eq!(
            derive_surface_kind(
                unavailable("no descriptor declared").as_ref(),
                &detected_leg()
            ),
            SurfaceKind::Gap
        );
    }

    #[test]
    fn capability_refused_with_detection_absence_is_absent_never_gap() {
        for leg in [DetectionLeg::NotInstalled, DetectionLeg::AbsentFromRecord] {
            assert_eq!(
                derive_surface_kind(unavailable("no descriptor declared").as_ref(), &leg),
                SurfaceKind::Absent,
                "detection's absence evidence is honoured even when capability refuses"
            );
        }
    }

    #[test]
    fn unreadable_intakes_are_unavailable_never_absence() {
        for leg in [
            DetectionLeg::RunUnavailable {
                reason: "spawn lost".into(),
            },
            DetectionLeg::EntryUnavailable {
                reason: Some("probes failed".into()),
            },
        ] {
            assert_eq!(
                derive_surface_kind(unavailable("could not run actuation").as_ref(), &leg),
                SurfaceKind::Unavailable,
                "an unreadable intake leg cannot be read as absence or as a gap"
            );
        }
    }

    #[test]
    fn self_owned_is_outside_the_three_state_law() {
        assert_eq!(
            derive_surface_kind(None, &DetectionLeg::SelfOwned),
            SurfaceKind::SelfOwned
        );
    }

    fn detection_record(json: &str) -> DetectionOutcome {
        DetectionOutcome::Record(Box::new(
            serde_json::from_str::<ActuationDetectionRecord>(json).unwrap(),
        ))
    }

    #[test]
    fn detection_leg_maps_the_record_three_state_law() {
        let outcome = detection_record(
            r#"{
              "schema": "actuation.harness-detection/v1",
              "detection_ref": "detection:fixture",
              "observed_at": "2026-09-15T00:00:00Z",
              "catalog_revision": 7,
              "detector": {"implementation": "fixture"},
              "harnesses": [
                {"slug": "pi", "harness_ref": "harness/pi", "state": "detected",
                 "probes": [{"kind": "config-dir", "result": "pass", "spec": "~/.pi/agent"}]},
                {"slug": "aider", "harness_ref": "harness/aider", "state": "not-installed"},
                {"slug": "flaky", "harness_ref": "harness/flaky", "state": "unavailable",
                 "unavailable_reason": "probes failed"}
              ],
              "absent": [],
              "availability": "complete"
            }"#,
        );
        let pi = REGISTRY.iter().find(|e| e.name == "pi").unwrap();
        let aider = REGISTRY.iter().find(|e| e.name == TargetId::AIDER).unwrap();
        let flaky = RegisteredClient {
            name: "flaky",
            aliases: &[],
            catalog_slug: Some("flaky"),
            semantic: SemanticBasis::None,
            reach: Reach::AdapterOnly {
                build: |_| Box::new(PiAdapter::new(".")),
            },
            admission: None,
        };

        match detection_leg(&outcome, pi) {
            DetectionLeg::Detected { config_dir } => {
                assert_eq!(config_dir.as_deref(), Some("~/.pi/agent"))
            }
            other => panic!("pi must be detected with its config probe, got {other:?}"),
        }
        assert_eq!(
            derive_surface_kind(
                unavailable("refused").as_ref(),
                &detection_leg(&outcome, pi)
            ),
            SurfaceKind::Gap
        );
        assert_eq!(
            derive_surface_kind(
                unavailable("refused").as_ref(),
                &detection_leg(&outcome, aider)
            ),
            SurfaceKind::Absent
        );
        match detection_leg(&outcome, &flaky) {
            DetectionLeg::EntryUnavailable { reason } => {
                assert_eq!(reason.as_deref(), Some("probes failed"))
            }
            other => panic!("flaky must carry its unavailable reason, got {other:?}"),
        }
    }

    #[test]
    fn slug_absent_from_record_is_disclosed_absence() {
        let outcome = detection_record(
            r#"{
              "schema": "actuation.harness-detection/v1",
              "detection_ref": "detection:fixture",
              "observed_at": "2026-09-15T00:00:00Z",
              "catalog_revision": 7,
              "detector": {"implementation": "fixture"},
              "harnesses": [],
              "absent": [],
              "availability": "complete"
            }"#,
        );
        let pi = REGISTRY.iter().find(|e| e.name == "pi").unwrap();
        assert!(matches!(
            detection_leg(&outcome, pi),
            DetectionLeg::AbsentFromRecord
        ));
    }

    #[test]
    fn registry_is_one_roster_keyed_by_target_id() {
        let mut names: Vec<&str> = REGISTRY.iter().map(|e| e.name).collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count, "registry names must be unique");
        for entry in REGISTRY {
            match entry.reach {
                Reach::SelfOwned { .. } => {
                    assert!(
                        entry.catalog_slug.is_none(),
                        "{} owns its config home",
                        entry.name
                    );
                    assert!(
                        entry.admission.is_none(),
                        "{} needs no admission",
                        entry.name
                    );
                }
                _ => {
                    assert!(
                        entry.catalog_slug.is_some(),
                        "{} must carry its catalog slug",
                        entry.name
                    );
                    assert!(
                        entry.admission.is_some(),
                        "{} must carry an admission census for gap disclosure",
                        entry.name
                    );
                }
            }
        }
    }

    #[test]
    fn every_harness_client_effects_dispatches_has_a_registry_row() {
        // The harness targets `app::client_effects` dispatches on, pinned here
        // so a harness added to the effects dispatch without a registry row
        // fails this test instead of silently diverging from the roster.
        let dispatched: &[&str] = &[
            TargetId::CLAUDE_CODE,
            TargetId::CODEX,
            TargetId::ZCODE,
            TargetId::DEEPSEEK_HARNESS,
            TargetId::AIDER,
            TargetId::CURSOR_CLI,
            TargetId::GEMINI_CLI,
            TargetId::GOOSE,
            TargetId::OPENCODE,
            TargetId::QWEN_CODE,
            TargetId::ANTIGRAVITY,
            TargetId::GROK_BOT,
            TargetId::KIMI,
            TargetId::OLLAMA,
            TargetId::OPENCLAW,
            TargetId::PI,
        ];
        for target in dispatched {
            let resolved = lookup(target).is_some()
                || REGISTRY
                    .iter()
                    .any(|entry| entry.catalog_slug == Some(*target));
            assert!(
                resolved,
                "{target} is dispatched by client_effects but has no registry row"
            );
        }

        // The dispatch cannot grow a harness arm without this guard knowing:
        // the function body must name exactly these targets plus the shell
        // projection target (the `_` arm is the broker fallback and names no
        // TargetId).
        let body = include_str!("app/mod.rs")
            .split("fn client_effects")
            .nth(1)
            .and_then(|rest| rest.split("\n    fn shell_plan").next())
            .expect("client_effects' dispatch body must be findable; update this guard");
        assert_eq!(
            body.matches("TargetId::").count(),
            dispatched.len() + 1,
            "client_effects names a target this guard does not list; \
             extend the guard and the registry together with the dispatch"
        );

        // Where the profile surface joins a roster slug to a catalog slug, the
        // joined profile must exist. Slugs the profile surface does not map
        // (dsh, grok-bot, aider, …) are roster-only and assert nothing here.
        for entry in REGISTRY {
            let Some(slug) = entry.catalog_slug else {
                continue; // the broker: AIKit's own, no catalog join
            };
            if let Some(profile_slug) =
                aikit_adapters::profiles::slug_for_target(&TargetId::new(slug))
            {
                assert!(
                    aikit_adapters::profiles::for_slug(profile_slug).is_some(),
                    "{slug} joins to profile `{profile_slug}` but no embedded profile carries it"
                );
            }
        }
    }

    #[test]
    fn aliases_resolve_to_the_same_entry() {
        for (alias, name) in [
            ("claude-code", "claude"),
            ("antigravity", TargetId::ANTIGRAVITY),
            ("cursor", TargetId::CURSOR_CLI),
            ("dsh", TargetId::DEEPSEEK_HARNESS),
            ("gemini", TargetId::GEMINI_CLI),
            ("grokbot", TargetId::GROK_BOT),
        ] {
            let via_alias = lookup(alias).expect("alias must resolve");
            let via_name = lookup(name).expect("name must resolve");
            assert!(
                std::ptr::eq(via_alias, via_name),
                "{alias} and {name} must be the same registry entry"
            );
        }
    }

    #[test]
    fn every_registered_harness_gap_disclosure_builds() {
        // The admission census of every registered harness must satisfy the
        // gap contract, or a gap row would lose its structured disclosure.
        let dirs = ClientDirs {
            ctx_dir: PathBuf::from("/tmp/ctx"),
            tree: PathBuf::from("/tmp/tree"),
            home: PathBuf::from("/tmp/home"),
        };
        for entry in REGISTRY {
            let Some(admission) = entry.admission else {
                continue;
            };
            let descriptor = admission(&dirs);
            unsupported_harness_gap(
                descriptor.target.clone(),
                descriptor.product.clone(),
                descriptor.edition,
                descriptor.native_version.clone(),
                descriptor.faculties.clone(),
            )
            .unwrap_or_else(|error| {
                panic!(
                    "{}'s admission must build a valid gap: {}",
                    entry.name,
                    error.message()
                )
            });
        }
    }

    #[test]
    fn client_roster_is_derived_never_a_literal() {
        let source = include_str!("client.rs");
        // Split the needles so this check's own text cannot match them.
        let loop_needle = format!("for {} in [", "client");
        assert!(
            !source.contains(&loop_needle),
            "the client roster must be enumerated from the registry, not a literal loop"
        );
        assert!(
            source.contains("for entry in REGISTRY"),
            "status must enumerate the registry"
        );
        let roster_needle = format!("\"claude\", \"{}\", \"zcode\", \"{}\"", "codex", "broker");
        assert!(
            !source.contains(&roster_needle),
            "the client roster literal must not return"
        );
    }
}
