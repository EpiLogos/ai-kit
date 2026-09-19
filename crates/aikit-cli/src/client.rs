//! `aikit client install|launch|status` over the real client adapters.
//!
//! Installing a client's dispatcher entries edits files AIKit does not own —
//! `~/.claude/settings.json`, a Codex hooks file — so it is a **Procedure**:
//! planned, diffed, reversible. The adapters decide *what* the edit is (they know
//! each client's config shape); this module turns that into world edits with
//! inverses and hands them to the one engine.
//!
//! ## The roster derives from detection, never a list
//!
//! The universe of harness rows is the live `actuation harness detect --json`
//! output — every descriptor the detector reports, whatever its state — plus
//! exactly one synthetic non-harness row, the broker (AIKit itself). Detection
//! owns the list; AIKit adds detail to it. The detail lives in the per-client
//! overlay below, keyed by catalog slug: config-home fallbacks, dispatch
//! decisions, materialisation adapters and admission censuses. A catalog slug
//! with no overlay renders as an honest adapter-only row (the gemini
//! precedent: the `aikit.harness-adapter/v1` missing-contract disclosure); a
//! slug with no catalog entry has no row at all — it appears when Actuation's
//! descriptor lands, never before. An intake that cannot be read at all is
//! disclosed as unavailable — never silence, never a hard-coded roster.

use std::path::{Path, PathBuf};

use aikit_core::capsule::Kind;
use aikit_core::harness_admission::{
    HarnessAdmissionAdapter, HarnessAdmissionDescriptor, HarnessEditionKind,
    unsupported_harness_gap,
};
use aikit_core::procedure::{Inverse, Plan, Procedure, ProcedureKind, WorldEdit};
use aikit_core::projection::{ProjectionItem, ResolvedContext, TargetAdapter};
use aikit_core::{AikitError, Result, TargetId};

use aikit_adapters::actuation_harness_capability::{
    CapabilityOutcome, HarnessCapability, intake_actuation_capability,
};
use aikit_adapters::actuation_harness_detection::{
    DetectionEntry, DetectionOutcome, DetectionState, intake_actuation_detection,
};
use aikit_adapters::clients::{
    ClientAdapter, antigravity::AntigravityAdapter, broker::BrokerAdapter, claude::ClaudeAdapter,
    codex::CodexAdapter, gemini::GeminiAdapter, grokbot::GrokbotAdapter, hermes::HermesAdapter,
    kimi::KimiAdapter, ollama::OllamaAdapter, openclaw::OpenclawAdapter, pi::PiAdapter,
    zcode::ZcodeAdapter,
};
use aikit_adapters::runner::SystemRunner;

use crate::app::Service;

/// The Actuation binary every intake asks. Resolved at spawn time; a missing
/// or refusing binary is an intake outcome, never a build-time fact.
const ACTUATION_BIN: &str = "actuation";

/// The one synthetic non-harness row: AIKit's own client, outside the
/// catalog's law by design.
const BROKER: &str = "broker";

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
}

/// How AIKit reaches a harness it carries detail for. Every variant carries
/// function pointers only, so a reach copies out of the static overlay table.
#[derive(Clone, Copy)]
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

/// One harness adapter builder: the client dirs in, a live adapter plus its
/// storage path out.
type AdapterBuild = fn(&ClientDirs) -> Result<(Box<dyn ClientAdapter>, PathBuf)>;

/// A dispatch client's adapter builder: the client dirs and the resolved
/// capability in, a live adapter plus its storage path out.
type CapabilityAdapterBuild =
    fn(&ClientDirs, Option<HarnessCapability>) -> Result<(Box<dyn ClientAdapter>, PathBuf)>;

/// The broker's reach — the one SelfOwned resident. AIKit's own client needs
/// no descriptor and no admission: the config home is AIKit's (`~/.aikit`),
/// and the build is the real broker adapter, not a stub. The roster law is
/// untouched: the broker is not an overlay (it has no catalog slug), it is
/// the synthetic row the derived roster appends.
fn broker_reach() -> Reach {
    Reach::SelfOwned {
        build: |dirs| {
            Ok((
                Box::new(BrokerAdapter::new()) as Box<dyn ClientAdapter>,
                dirs.home.join(".aikit"),
            ))
        },
    }
}

/// AIKit's detail for one catalog slug: the per-client overlay. The overlay is
/// keyed by `catalog_slug` and carries everything detection cannot say — the
/// CLI-facing name, aliases, the adapter, the admission census. A catalog slug
/// without an overlay is a real harness row all the same: the generic row
/// renders it honestly from its record entry alone. An overlay whose slug is
/// absent from the catalog is unrepresentable: the key is not an `Option`.
struct ClientOverlay {
    /// The CLI-facing name (`aikit client status <name>`).
    name: &'static str,
    /// Other names accepted for the same entry.
    aliases: &'static [&'static str],
    /// The Actuation catalog slug this overlay details — the detection and
    /// capability intakes ask for it.
    catalog_slug: &'static str,
    semantic: SemanticBasis,
    reach: Reach,
    /// The evidence-backed admission census, used to disclose compatibility
    /// gaps.
    admission: fn(&ClientDirs) -> HarnessAdmissionDescriptor,
}

/// The overlay surface: one entry per catalog slug AIKit carries an adapter
/// for. This is detail ON detection's list, never a second list — a harness
/// that Actuation stops declaring loses its overlay's row basis and shows up
/// (if at all) through the record, and a harness Actuation declares without an
/// overlay here still gets its honest generic row.
static OVERLAYS: &[ClientOverlay] = &[
    ClientOverlay {
        name: "claude",
        aliases: &["claude-code"],
        catalog_slug: TargetId::CLAUDE_CODE,
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
        admission: |dirs| ClaudeAdapter::new(dirs.ctx_dir.clone()).admission(),
    },
    ClientOverlay {
        name: "codex",
        aliases: &[],
        catalog_slug: TargetId::CODEX,
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
        admission: |dirs| CodexAdapter::new(dirs.tree.clone()).admission(),
    },
    ClientOverlay {
        name: "zcode",
        aliases: &[],
        catalog_slug: TargetId::ZCODE,
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
        admission: |_dirs| ZcodeAdapter::new().admission(),
    },
    ClientOverlay {
        name: TargetId::GEMINI_CLI,
        aliases: &["gemini"],
        // The catalog slug is `gemini` (Round 4, TargetId::GEMINI): the
        // client keeps the name `gemini-cli` with its `gemini` alias, and the
        // detection/capability intakes ask the catalog what it actually
        // declares — the claude/claude-code precedent on the join key.
        catalog_slug: TargetId::GEMINI,
        semantic: SemanticBasis::None,
        reach: Reach::AdapterOnly {
            build: |dirs| Box::new(GeminiAdapter::new(projection_dir(dirs, "gemini"))),
        },
        admission: |dirs| GeminiAdapter::new(projection_dir(dirs, "gemini")).admission(),
    },
    ClientOverlay {
        name: TargetId::PI,
        aliases: &[],
        catalog_slug: TargetId::PI,
        semantic: SemanticBasis::Skills,
        reach: Reach::AdapterOnly {
            build: |dirs| Box::new(PiAdapter::new(projection_dir(dirs, "pi"))),
        },
        admission: |dirs| PiAdapter::new(projection_dir(dirs, "pi")).admission(),
    },
    ClientOverlay {
        name: TargetId::ANTIGRAVITY,
        aliases: &["antigravity"],
        catalog_slug: TargetId::ANTIGRAVITY,
        semantic: SemanticBasis::None,
        reach: Reach::AdapterOnly {
            build: |dirs| Box::new(AntigravityAdapter::new(projection_dir(dirs, "antigravity"))),
        },
        admission: |dirs| AntigravityAdapter::new(projection_dir(dirs, "antigravity")).admission(),
    },
    ClientOverlay {
        name: TargetId::GROK_BOT,
        aliases: &["grokbot"],
        catalog_slug: TargetId::GROK_BOT,
        semantic: SemanticBasis::None,
        reach: Reach::AdapterOnly {
            build: |dirs| Box::new(GrokbotAdapter::new(projection_dir(dirs, "grokbot"))),
        },
        admission: |dirs| GrokbotAdapter::new(projection_dir(dirs, "grokbot")).admission(),
    },
    ClientOverlay {
        name: TargetId::KIMI,
        aliases: &[],
        catalog_slug: TargetId::KIMI,
        semantic: SemanticBasis::None,
        reach: Reach::AdapterOnly {
            build: |dirs| Box::new(KimiAdapter::new(projection_dir(dirs, "kimi"))),
        },
        admission: |dirs| KimiAdapter::new(projection_dir(dirs, "kimi")).admission(),
    },
    ClientOverlay {
        name: TargetId::OPENCLAW,
        aliases: &[],
        catalog_slug: TargetId::OPENCLAW,
        semantic: SemanticBasis::None,
        reach: Reach::AdapterOnly {
            build: |dirs| Box::new(OpenclawAdapter::new(projection_dir(dirs, "openclaw"))),
        },
        admission: |dirs| OpenclawAdapter::new(projection_dir(dirs, "openclaw")).admission(),
    },
    ClientOverlay {
        name: TargetId::OLLAMA,
        aliases: &[],
        catalog_slug: TargetId::OLLAMA,
        semantic: SemanticBasis::None,
        reach: Reach::AdapterOnly {
            build: |dirs| Box::new(OllamaAdapter::new(projection_dir(dirs, "ollama"))),
        },
        admission: |dirs| OllamaAdapter::new(projection_dir(dirs, "ollama")).admission(),
    },
    ClientOverlay {
        name: TargetId::HERMES,
        aliases: &[],
        catalog_slug: TargetId::HERMES,
        semantic: SemanticBasis::None,
        reach: Reach::AdapterOnly {
            build: |dirs| Box::new(HermesAdapter::new(projection_dir(dirs, "hermes"))),
        },
        admission: |dirs| HermesAdapter::new(projection_dir(dirs, "hermes")).admission(),
    },
    ClientOverlay {
        name: TargetId::HERMES_ACP,
        aliases: &[],
        catalog_slug: TargetId::HERMES_ACP,
        semantic: SemanticBasis::None,
        reach: Reach::AdapterOnly {
            build: |dirs| Box::new(HermesAdapter::acp(projection_dir(dirs, "hermes-acp"))),
        },
        admission: |dirs| HermesAdapter::acp(projection_dir(dirs, "hermes-acp")).admission(),
    },
];

fn client_overlay(client: &str) -> Option<&'static ClientOverlay> {
    OVERLAYS.iter().find(|overlay| {
        overlay.name == client
            || overlay.aliases.contains(&client)
            || overlay.catalog_slug == client
    })
}

fn unknown_client_error(client: &str) -> AikitError {
    let mut names: Vec<&str> = OVERLAYS.iter().map(|overlay| overlay.name).collect();
    names.push(BROKER);
    let names = names.join(", ");
    AikitError::new(
        "client.unknown",
        format!(
            "`{client}` is not a client AIKit has an adapter for; the adapter surface is: {names}. \
             The full harness roster derives from `actuation harness detect` — \
             `aikit client status` shows every descriptor it reports, overlaid or not"
        ),
    )
    .with("client", client.to_string())
}

fn not_dispatchable(overlay: &ClientOverlay) -> AikitError {
    AikitError::new(
        "client.not_dispatchable",
        format!(
            "`{}` is a registered harness with no launch or install seam; \
             `aikit client install|launch` reaches dispatch clients only",
            overlay.name
        ),
    )
    .with("client", overlay.name.to_string())
}

/// One row of the derived client surface. The type makes the wrong state
/// unrepresentable: every row is the broker, or one descriptor read from the
/// live detection record, or an overlay whose declared catalog slug the record
/// does not name (or could not be read at all). A row keyed by nothing at all
/// cannot be constructed.
enum RosterMember {
    /// A descriptor the live record reports, with AIKit's overlay when one
    /// exists for its slug.
    Descriptor {
        entry: Box<DetectionEntry>,
        overlay: Option<&'static ClientOverlay>,
    },
    /// An overlay whose catalog slug the record does not name (a detector
    /// lagging the catalog), or whose record could not be read: AIKit's own
    /// integrations stay visible with the disclosure instead of vanishing.
    Unrecorded { overlay: &'static ClientOverlay },
    /// The broker: AIKit itself, outside detection's law.
    Broker,
}

impl RosterMember {
    /// Whether this member answers to the given name: its CLI-facing name, an
    /// alias, or — for a generic row — its catalog slug.
    fn answers_to(&self, name: &str) -> bool {
        match self {
            RosterMember::Descriptor {
                entry: _,
                overlay: Some(overlay),
            } => overlay.name == name || overlay.aliases.contains(&name),
            RosterMember::Descriptor {
                entry,
                overlay: None,
            } => entry.slug == name,
            RosterMember::Unrecorded { overlay } => {
                overlay.name == name || overlay.aliases.contains(&name)
            }
            RosterMember::Broker => name == BROKER,
        }
    }
}

/// Derive the roster members from one detection outcome. The record's entries
/// come first (record order); overlays the record does not name follow; the
/// broker closes the surface. With an unreadable record only the overlays and
/// the broker render — nothing is invented in detection's place.
fn roster_members(detection: &DetectionOutcome) -> Vec<RosterMember> {
    let mut members = Vec::new();
    match detection {
        DetectionOutcome::Record(record) => {
            for entry in &record.harnesses {
                let overlay = OVERLAYS
                    .iter()
                    .find(|o| o.catalog_slug == entry.slug.as_str());
                members.push(RosterMember::Descriptor {
                    entry: Box::new(entry.clone()),
                    overlay,
                });
            }
            for overlay in OVERLAYS {
                if !record
                    .harnesses
                    .iter()
                    .any(|entry| entry.slug == overlay.catalog_slug)
                {
                    members.push(RosterMember::Unrecorded { overlay });
                }
            }
        }
        DetectionOutcome::Unavailable { .. } => {
            for overlay in OVERLAYS {
                members.push(RosterMember::Unrecorded { overlay });
            }
        }
    }
    members.push(RosterMember::Broker);
    members
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
}

fn detection_leg(detection: &DetectionOutcome, slug: &str) -> DetectionLeg {
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

/// The derived surface state of an overlaid harness, from the two intake legs.
/// Capability resolved means the install leg is satisfiable; a harness that is
/// present while its descriptor is refused is a compatibility gap, not an
/// error; detection's absence evidence is honoured; an unreadable intake is
/// disclosed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SurfaceKind {
    Installable,
    Gap,
    Absent,
    Unavailable,
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
        },
        None => SurfaceKind::Unavailable,
    }
}

/// The state of a catalog slug AIKit carries no overlay for. There is no
/// adapter to install or plan through, so the row can never claim
/// installable: a descriptor on the contract leg does not make the
/// harness-adapter contract resolved. Present-without-adapter is the same
/// honest gap the gemini precedent discloses.
fn derive_generic_kind(leg: &DetectionLeg) -> SurfaceKind {
    match leg {
        DetectionLeg::Detected { .. } => SurfaceKind::Gap,
        DetectionLeg::NotInstalled | DetectionLeg::AbsentFromRecord => SurfaceKind::Absent,
        DetectionLeg::EntryUnavailable { .. } | DetectionLeg::RunUnavailable { .. } => {
            SurfaceKind::Unavailable
        }
    }
}

/// The adapter plus its configuration home. `capability` is `None` when
/// Actuation's descriptor is unreachable — readable, but not installable.
///
/// Dispatch comes from the one overlay surface: a name no overlay carries is
/// unknown to install and launch (the harness may still be a real catalog row;
/// `aikit client status` shows it).
fn adapter_for(
    service: &Service,
    client: &str,
) -> Result<(Box<dyn ClientAdapter>, Option<HarnessCapability>, PathBuf)> {
    let dirs = client_dirs(service);
    let overlay = if client == BROKER {
        None
    } else {
        Some(client_overlay(client).ok_or_else(|| unknown_client_error(client))?)
    };
    // The broker reaches through `SelfOwned`: no descriptor is consulted,
    // because AIKit owns its own config home. Every overlay harness answers
    // to Actuation's capability intake first.
    let (reach, capability) = match overlay {
        None => (broker_reach(), None),
        Some(overlay) => {
            let capability = match intake_actuation_capability(
                &SystemRunner::new(),
                ACTUATION_BIN,
                overlay.catalog_slug,
            ) {
                CapabilityOutcome::Descriptor(capability) => Some(*capability),
                CapabilityOutcome::Unavailable { .. } => None,
            };
            (overlay.reach, capability)
        }
    };
    match reach {
        Reach::SelfOwned { build } => {
            let (adapter, config_dir) = build(&dirs)?;
            Ok((adapter, None, config_dir))
        }
        Reach::Client { build } => {
            let (adapter, config_dir) = build(&dirs, capability.clone())?;
            Ok((adapter, capability, config_dir))
        }
        Reach::AdapterOnly { .. } => Err(not_dispatchable(
            overlay.expect("only the broker is overlay-less"),
        )),
    }
}

/// Plan the install as a Procedure.
pub fn plan_install(service: &Service, client: &str) -> Result<Procedure> {
    // The extension-carrier seam: a harness whose profile declares a managed
    // hooks layer through the pi-extensions-record grammar has no dispatcher
    // entries to install — its managed install is the carrier itself,
    // projected through the settings `extensions` array and gated by capsule
    // trust. The reach stays AdapterOnly for launch: pi is launched through
    // its own per-invocation CLI, not through AIKit's projection. The gate is
    // profile-derived on the overlay's catalog slug, so a harness Actuation
    // stops declaring loses the seam's row basis with its roster row.
    let carrier_profile = client_overlay(client)
        .map(|overlay| overlay.catalog_slug)
        .and_then(aikit_adapters::profiles::for_slug)
        .filter(|profile| {
            profile.hooks.as_ref().is_some_and(|hooks| {
                hooks.posture == aikit_core::harness_profile::LayerPosture::Managed
                    && hooks.project.as_ref().is_some_and(|project| {
                        project.format
                            == aikit_core::harness_profile::MergeGrammar::PiExtensionsRecord
                    })
            })
        });
    if let Some(profile) = carrier_profile {
        return plan_carrier_install(service, client, profile);
    }
    let (adapter, capability, config_dir) = adapter_for(service, client)?;
    // The law is unchanged: AIKit installs only what Actuation declares the
    // harness to be. The broker is the one exception, because AIKit owns its
    // config home and needs no descriptor for it.
    if capability.is_none() && client != BROKER {
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
                ));
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

/// The registered harnesses whose managed hooks seam is **project-relative** —
/// a seam of the working tree, not of the machine. `aikit apply` keeps these
/// current, because applying a project is what materialises that tree's
/// declarations; machine-level seams (`~/.claude/settings.json`,
/// `~/.zcode/cli/config.json`) stay with the explicit `aikit client install`
/// procedure. Selection is derived, never a list: a harness qualifies when its
/// profile declares a managed hooks layer whose project file is neither
/// home-relative nor absolute.
pub fn project_scoped_hook_clients() -> Vec<&'static str> {
    OVERLAYS
        .iter()
        .filter_map(|overlay| {
            // The overlay's catalog slug is the profile join key — the same
            // key the roster joins detection by.
            let profile = aikit_adapters::profiles::for_slug(overlay.catalog_slug)?;
            let hooks = profile.hooks.as_ref()?;
            if hooks.posture != aikit_core::harness_profile::LayerPosture::Managed {
                return None;
            }
            let file = &hooks.project.as_ref()?.file;
            if file.starts_with('~') || Path::new(file).is_absolute() {
                None
            } else {
                Some(overlay.name)
            }
        })
        .collect()
}

/// One project-scoped hook-seam install, as `apply` reports it. `refused`
/// carries the plain reason nothing was written — a missing descriptor is a
/// disclosure, never a failed apply.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct HookSeamOutcome {
    pub client: &'static str,
    /// `installed` (edits applied), `satisfied` (already in place), or
    /// `refused` (nothing written; `reason` says why).
    pub state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub procedure: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub undo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edits: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Plan and run every project-scoped managed hook seam (`aikit apply`'s tail).
/// Each client installs through the same `plan_install` procedure pipeline the
/// explicit command uses — descriptor intake, transport-filtered events, the
/// profile-declared merge grammar with foreign entries preserved and owned
/// entries swept — so apply's seam write is diffable and reversible exactly
/// like `aikit client install`. A refusal is an outcome, not an error: apply
/// must not fail because one harness's descriptor is unreachable.
pub fn install_project_hook_seams(service: &Service) -> Vec<HookSeamOutcome> {
    project_scoped_hook_clients()
        .into_iter()
        .map(|client| match plan_install(service, client) {
            Ok(procedure) => {
                let runner = aikit_store::procedure::ProcedureRunner::new(service.home());
                match runner.run(&procedure) {
                    Ok(outcome) => HookSeamOutcome {
                        client,
                        state: if outcome.already_satisfied {
                            "satisfied"
                        } else {
                            "installed"
                        },
                        procedure: Some(procedure.id.to_string()),
                        undo: Some(format!("aikit procedure undo {}", procedure.id)),
                        edits: Some(outcome.applied),
                        reason: None,
                    },
                    Err(error) => HookSeamOutcome {
                        client,
                        state: "refused",
                        procedure: None,
                        undo: None,
                        edits: None,
                        reason: Some(error.message().to_string()),
                    },
                }
            }
            Err(error) => HookSeamOutcome {
                client,
                state: "refused",
                procedure: None,
                undo: None,
                edits: None,
                reason: Some(error.message().to_string()),
            },
        })
        .collect()
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

/// What the derived surface says: the live detection record enumerates the
/// harness rows, each joined to its overlay detail when one exists, and the
/// broker closes the surface. The two intake legs (capability descriptor,
/// detection), the lower-level materialisation work required to realise a
/// projection, and whether the client is installed ride each row.
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
    let members = roster_members(&detection);
    let mut rows = Vec::new();
    for member in &members {
        if let Some(only) = only {
            if !member.answers_to(only) {
                continue;
            }
        }
        rows.push(client_row(member, &rc, &dirs, &detection)?);
    }
    Ok(rows)
}

/// Derive one roster member's row from the live intake outcomes.
fn client_row(
    member: &RosterMember,
    rc: &ResolvedContext,
    dirs: &ClientDirs,
    detection: &DetectionOutcome,
) -> Result<serde_json::Value> {
    match member {
        RosterMember::Broker => broker_row(rc, dirs),
        RosterMember::Descriptor { entry, overlay } => match overlay {
            Some(overlay) => overlaid_row(overlay, rc, dirs, detection),
            None => generic_row(entry, rc, dirs, detection),
        },
        RosterMember::Unrecorded { overlay } => overlaid_row(overlay, rc, dirs, detection),
    }
}

/// The broker's row: AIKit's own client, outside the three-state law.
fn broker_row(rc: &ResolvedContext, dirs: &ClientDirs) -> Result<serde_json::Value> {
    let adapter = BrokerAdapter::new();
    let config_dir = dirs.home.join(".aikit");
    let planned = adapter.plan(rc);
    Ok(serde_json::json!({
        "client": BROKER,
        "harness": null,
        "state": "self",
        "dispatch": "self",
        "config_dir": config_dir.display().to_string(),
        "installed": config_dir.exists(),
        "effect": planned.as_ref().ok().map(|p| adapter.activation_effect(None, p).describe()),
        "items": planned.as_ref().ok().map(|_| rc.view.active.len()),
        "materialization_items": planned.as_ref().ok().map(|p| p.items.len()),
        "actor_bootstrap": rc.actor_bootstrap.is_some(),
        "capability": "self",
        "capability_reason": null,
        "detection": "self",
        "detection_reason": null,
        "gap": null,
        "notes": planned.as_ref().map(|p| p.notes.clone()).unwrap_or_default(),
        "error": planned.as_ref().err().map(|e| e.message().to_string()),
    }))
}

/// An overlaid harness's row: the full detail surface — adapter plan, semantic
/// items, admission census-backed gap disclosure.
fn overlaid_row(
    overlay: &'static ClientOverlay,
    rc: &ResolvedContext,
    dirs: &ClientDirs,
    detection: &DetectionOutcome,
) -> Result<serde_json::Value> {
    let leg = detection_leg(detection, overlay.catalog_slug);
    let capability = Some(intake_actuation_capability(
        &SystemRunner::new(),
        ACTUATION_BIN,
        overlay.catalog_slug,
    ));
    let kind = derive_surface_kind(capability.as_ref(), &leg);

    // The adapter for planning, and the config home the row reports. The
    // descriptor's seam wins when it resolved; the detection probe is the
    // read-model fallback for adapter-only harnesses; the adapter's default
    // home is the dispatch clients' fallback.
    let (adapter, config_dir): (Box<dyn TargetAdapter>, Option<PathBuf>) = match overlay.reach {
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
    let semantic_items = match overlay.semantic {
        SemanticBasis::Skills => rc.view.active_of_kind(Kind::Skill).len(),
        SemanticBasis::None => 0,
    };
    let mut notes = planned
        .as_ref()
        .map(|p| p.notes.clone())
        .unwrap_or_default();

    let (state, gap) = match kind {
        SurfaceKind::Installable => ("installable", None),
        SurfaceKind::Absent => ("absent", None),
        SurfaceKind::Unavailable => ("unavailable", None),
        SurfaceKind::Gap => ("gap", Some(gap_disclosure(overlay, dirs, &mut notes))),
    };

    let (capability_name, capability_reason) = match &capability {
        Some(CapabilityOutcome::Descriptor(_)) => ("descriptor", None),
        Some(CapabilityOutcome::Unavailable { reason }) => ("unavailable", Some(reason.clone())),
        None => ("unavailable", None),
    };
    let (detection_name, detection_reason) = leg_names(&leg, overlay.catalog_slug);
    let dispatch_name = match overlay.reach {
        Reach::SelfOwned { .. } => "self",
        Reach::Client { .. } => "client",
        Reach::AdapterOnly { .. } => "adapter-only",
    };

    Ok(serde_json::json!({
        "client": overlay.name,
        "harness": overlay.catalog_slug,
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

/// A catalog slug's row when AIKit carries no overlay for it: the honest
/// adapter-only shape. There is no adapter to plan or install through, so
/// effect and item counts stay null; the detection probe is the config-home
/// evidence; a present harness discloses the missing harness-adapter contract.
fn generic_row(
    entry: &DetectionEntry,
    rc: &ResolvedContext,
    dirs: &ClientDirs,
    detection: &DetectionOutcome,
) -> Result<serde_json::Value> {
    let leg = detection_leg(detection, &entry.slug);
    let capability = intake_actuation_capability(&SystemRunner::new(), ACTUATION_BIN, &entry.slug);
    let kind = derive_generic_kind(&leg);

    let (state, gap) = match kind {
        SurfaceKind::Gap => ("gap", generic_gap_disclosure(entry)),
        SurfaceKind::Absent => ("absent", None),
        SurfaceKind::Unavailable => ("unavailable", None),
        // Unreachable by construction: `derive_generic_kind` never yields it.
        SurfaceKind::Installable => ("installable", None),
    };

    let (capability_name, capability_reason) = match &capability {
        CapabilityOutcome::Descriptor(_) => ("descriptor", None),
        CapabilityOutcome::Unavailable { reason } => ("unavailable", Some(reason.clone())),
    };
    let (detection_name, detection_reason) = leg_names(&leg, &entry.slug);
    let config_dir = leg
        .detected_config_dir()
        .map(|spec| expand_seam(&spec, &dirs.home, &dirs.tree));

    Ok(serde_json::json!({
        "client": entry.slug,
        "harness": entry.slug,
        "state": state,
        "dispatch": "adapter-only",
        "config_dir": config_dir.as_ref().map(|d| d.display().to_string()),
        "installed": config_dir.as_ref().map(|d| d.exists()),
        "effect": null,
        "items": null,
        "materialization_items": null,
        "actor_bootstrap": rc.actor_bootstrap.is_some(),
        "capability": capability_name,
        "capability_reason": capability_reason,
        "detection": detection_name,
        "detection_reason": detection_reason,
        "gap": gap,
        "notes": ["no AIKit adapter carries this catalog slug; the row is the \
                   detection record's own disclosure"],
        "error": null,
    }))
}

/// The display names of a detection leg, shared by overlaid and generic rows.
fn leg_names(leg: &DetectionLeg, slug: &str) -> (&'static str, Option<String>) {
    match leg {
        DetectionLeg::Detected { .. } => ("detected", None),
        DetectionLeg::NotInstalled => ("not-installed", None),
        DetectionLeg::EntryUnavailable { reason } => ("unavailable", reason.clone()),
        DetectionLeg::AbsentFromRecord => (
            "absent-from-record",
            Some(format!(
                "the detection record names no entry for slug {slug}"
            )),
        ),
        DetectionLeg::RunUnavailable { reason } => ("unavailable", Some(reason.clone())),
    }
}

/// The compatibility-gap disclosure for an overlaid harness that is present
/// here while its capability descriptor is refused: the harness-adapter
/// contract's own structured gap, built from the adapter's evidence-backed
/// admission census. A census that cannot be built is disclosed in the notes,
/// never silently dropped.
fn gap_disclosure(
    overlay: &'static ClientOverlay,
    dirs: &ClientDirs,
    notes: &mut Vec<String>,
) -> Option<serde_json::Value> {
    let descriptor = (overlay.admission)(dirs);
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

/// The generic slug's gap disclosure: no admission census exists because no
/// adapter does, so the missing contract is named plainly from the record's
/// own entry.
fn generic_gap_disclosure(entry: &DetectionEntry) -> Option<serde_json::Value> {
    let gap = unsupported_harness_gap(
        TargetId::new(entry.slug.clone()),
        entry.slug.clone(),
        HarnessEditionKind::Custom,
        entry.version.clone(),
        Vec::new(),
    )
    .ok()?;
    serde_json::to_value(&gap).ok()
}

impl DetectionLeg {
    fn detected_config_dir(&self) -> Option<String> {
        match self {
            DetectionLeg::Detected { config_dir } => config_dir.clone(),
            _ => None,
        }
    }
}

/// Plan the carrier install for a profile whose managed hooks layer projects
/// through the settings `extensions` array (pi, today).
///
/// The trust gate is not re-implemented here: the resolver only yields the
/// carrier capsule as active for a trust-recorded revision, so an untrusted
/// or blocked carrier plans as a sweep — every owned registration entry and
/// carrier file leaves, and pi loads no AIKit extension at all.
fn plan_carrier_install(
    service: &Service,
    client: &str,
    profile: &'static aikit_core::harness_profile::HarnessProfile,
) -> Result<Procedure> {
    // Actuation still declares what the harness is before AIKit writes its
    // native configuration — the same law the dispatcher-entry installs keep.
    // The overlay is the detail source: its catalog slug is what the
    // capability intake asks for.
    let overlay = client_overlay(client).ok_or_else(|| unknown_client_error(client))?;
    let capability = match intake_actuation_capability(
        &SystemRunner::new(),
        ACTUATION_BIN,
        overlay.catalog_slug,
    ) {
        CapabilityOutcome::Descriptor(capability) => Some(*capability),
        CapabilityOutcome::Unavailable { .. } => None,
    };
    if capability.is_none() {
        return Err(AikitError::new(
            "client.capability_unavailable",
            format!(
                "cannot install for {client}: Actuation's capability descriptor is unreachable, \
                 and AIKit installs only what Actuation declares the harness to be"
            ),
        )
        .with("client", client.to_string()));
    }

    // The carrier payload comes from the catalogued capsule, not from this
    // working tree: what gets projected is exactly the revision that was
    // reviewed.
    let carrier_id = aikit_adapters::CARRIER_CAPSULE_ID;
    let carrier_capsule = {
        use aikit_core::CapsuleId;
        use aikit_core::catalog::Catalog;
        let snapshot = service.snapshot();
        CapsuleId::parse(carrier_id)
            .ok()
            .and_then(|id| Catalog::get(snapshot, &id).cloned())
    };
    let payload = match &carrier_capsule {
        Some(capsule) => {
            let hook = capsule.hook().ok_or_else(|| {
                AikitError::new(
                    "client.carrier_not_a_hook",
                    format!(
                        "{carrier_id} is not a hook capsule; the carrier projection cannot proceed"
                    ),
                )
            })?;
            let path = capsule
                .root
                .as_ref()
                .ok_or_else(|| {
                    AikitError::new(
                        "client.carrier_unrooted",
                        format!("{carrier_id} has no payload root on this machine"),
                    )
                })?
                .join(&hook.entry);
            std::fs::read_to_string(&path).map_err(|error| {
                AikitError::new(
                    "client.carrier_unreadable",
                    format!(
                        "could not read the carrier payload at {}: {error}",
                        path.display()
                    ),
                )
                .with("path", path.display().to_string())
            })?
        }
        None => String::new(),
    };
    let carrier_active = service
        .resolved()
        .active_of_kind(Kind::Hook)
        .iter()
        .any(|active| active.id.to_string() == carrier_id);
    if carrier_active && carrier_capsule.is_none() {
        return Err(AikitError::new(
            "client.carrier_unrooted",
            format!("{carrier_id} is active but has no payload root on this machine"),
        ));
    }

    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let project = profile
        .hooks
        .as_ref()
        .and_then(|hooks| hooks.project.as_ref())
        .ok_or_else(|| {
            AikitError::new(
                "client.carrier_without_seam",
                format!(
                    "the {client} profile's hooks layer declares no project seam; the carrier cannot be installed"
                ),
            )
            .with("client", client.to_string())
        })?;
    let settings_target = expand_home(&project.file, &home);
    let projection_absolute = service.context_projection_root().join("projections/pi");
    let projection = aikit_adapters::ProjectionDir::new(
        &projection_absolute,
        declared_home_relative(&projection_absolute, &home),
    );

    let outcome = aikit_adapters::plan_hooks_projection(
        carrier_active.then(|| aikit_adapters::HookCarrierSource {
            payload: payload.clone(),
        }),
        profile,
        &projection,
        |asked| {
            let seeded = expand_home(asked, &home);
            if seeded.is_file() {
                std::fs::read_to_string(&seeded).map(Some)
            } else {
                Ok(None)
            }
        },
        |dir| {
            Ok(std::fs::read_dir(dir)
                .map(|entries| {
                    entries
                        .filter_map(|entry| entry.ok())
                        .map(|entry| entry.file_name().to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default())
        },
    )?;

    let mut plan = Plan::new().with_note(format!(
        "install AIKit's {client} integration: the extension carrier registered through {}",
        project.file
    ));
    let outcome = match outcome {
        aikit_adapters::HooksProjectionOutcome::NotProjected { reason } => {
            return Err(AikitError::new("client.nothing_to_install", reason)
                .with("client", client.to_string()));
        }
        other => other,
    };
    let (settings_item, carrier_item, stale_files, activation_note) = match &outcome {
        aikit_adapters::HooksProjectionOutcome::Projected(plan) => (
            Some(&plan.settings_item),
            Some(&plan.carrier_item),
            plan.stale_carrier_files.as_slice(),
            "active: pi loads the carrier at the next session (a running TUI can /reload)"
                .to_string(),
        ),
        aikit_adapters::HooksProjectionOutcome::Swept(plan) => (
            Some(&plan.settings_item),
            None,
            plan.stale_carrier_files.as_slice(),
            "inactive: the carrier is not trust-active, so every owned registration and file was swept"
                .to_string(),
        ),
        aikit_adapters::HooksProjectionOutcome::NotProjected { .. } => unreachable!(),
    };
    if let Some(aikit_core::projection::ProjectionItem::Write { contents, .. }) = settings_item {
        plan = plan.with_edit(WorldEdit::WriteFile {
            path: settings_target.clone(),
            contents: contents.clone().into_bytes(),
            inverse: if settings_target.exists() {
                Inverse::Restore {
                    blob: aikit_core::procedure::BlobId::deferred(),
                }
            } else {
                Inverse::Remove
            },
        });
    }
    if let Some(aikit_core::projection::ProjectionItem::Write { contents, .. }) = carrier_item {
        plan = plan.with_edit(WorldEdit::WriteFile {
            path: projection.absolute.join(
                aikit_adapters::HookCarrierSource {
                    payload: payload.clone(),
                }
                .file_name(),
            ),
            contents: contents.clone().into_bytes(),
            inverse: Inverse::Remove,
        });
    }
    for stale in stale_files {
        plan = plan.with_edit(WorldEdit::DeleteFile {
            path: stale.clone(),
            inverse: Inverse::Restore {
                blob: aikit_core::procedure::BlobId::deferred(),
            },
        });
    }
    let _ = activation_note;

    if plan.is_empty() {
        return Err(AikitError::new(
            "client.nothing_to_install",
            format!(
                "the {client} carrier is inactive and nothing AIKit owns is registered; there is nothing to install or sweep"
            ),
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

/// Expand a leading `~/` against `home`, the spelling profile declarations
/// and plan write items use.
fn expand_home(path: &str, home: &Path) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => PathBuf::from(path),
    }
}

/// The home-relative spelling of an absolute path under `home`, for plan
/// write destinations; an unrelated absolute path passes through untouched.
fn declared_home_relative(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) => format!("~/{}", rest.to_string_lossy()),
        Err(_) => path.to_string_lossy().into_owned(),
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
    fn a_generic_slug_never_claims_installable() {
        // No overlay, no adapter: even detection's best evidence cannot make
        // the row installable — the missing contract is the adapter contract.
        for leg in [
            DetectionLeg::Detected { config_dir: None },
            DetectionLeg::NotInstalled,
            DetectionLeg::AbsentFromRecord,
            DetectionLeg::RunUnavailable {
                reason: "spawn lost".into(),
            },
        ] {
            assert_ne!(
                derive_generic_kind(&leg),
                SurfaceKind::Installable,
                "a generic row has nothing to install through"
            );
        }
        assert_eq!(
            derive_generic_kind(&DetectionLeg::Detected { config_dir: None }),
            SurfaceKind::Gap
        );
        assert_eq!(
            derive_generic_kind(&DetectionLeg::NotInstalled),
            SurfaceKind::Absent
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
                {"slug": "unrecorded-anywhere", "harness_ref": "harness/unrecorded-anywhere", "state": "not-installed"},
                {"slug": "flaky", "harness_ref": "harness/flaky", "state": "unavailable",
                 "unavailable_reason": "probes failed"}
              ],
              "absent": [],
              "availability": "complete"
            }"#,
        );
        match detection_leg(&outcome, "pi") {
            DetectionLeg::Detected { config_dir } => {
                assert_eq!(config_dir.as_deref(), Some("~/.pi/agent"))
            }
            other => panic!("pi must be detected with its config probe, got {other:?}"),
        }
        assert_eq!(
            derive_surface_kind(
                unavailable("refused").as_ref(),
                &detection_leg(&outcome, "pi")
            ),
            SurfaceKind::Gap
        );
        assert_eq!(
            derive_surface_kind(
                unavailable("refused").as_ref(),
                &detection_leg(&outcome, "unrecorded-anywhere")
            ),
            SurfaceKind::Absent
        );
        match detection_leg(&outcome, "flaky") {
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
        assert!(matches!(
            detection_leg(&outcome, "pi"),
            DetectionLeg::AbsentFromRecord
        ));
    }

    #[test]
    fn the_project_scoped_hook_seam_surface_is_derived_from_the_profiles() {
        // Codex's managed hooks seam is the working tree's `.codex/hooks.json`,
        // so `aikit apply` keeps it current. Claude and zcode name home-level
        // seams — machine state that stays with the explicit
        // `aikit client install` — and must never be swept into apply.
        assert_eq!(
            project_scoped_hook_clients(),
            vec!["codex"],
            "selection is derived from profile facts: managed hooks layers whose \
             project file is neither home-relative nor absolute"
        );
    }

    #[test]
    fn the_roster_is_the_record_plus_unrecorded_overlays_plus_the_broker() {
        // A record naming a generic slug and an overlaid slug: the generic slug
        // renders from the record alone, the overlay joins by catalog slug,
        // overlays the record does not name stay visible as unrecorded, and the
        // broker closes the surface.
        let outcome = detection_record(
            r#"{
              "schema": "actuation.harness-detection/v1",
              "detection_ref": "detection:fixture",
              "observed_at": "2026-09-15T00:00:00Z",
              "catalog_revision": 7,
              "detector": {"implementation": "fixture"},
              "harnesses": [
                {"slug": "pi", "harness_ref": "harness/pi", "state": "detected"},
                {"slug": "future-harness", "harness_ref": "harness/future-harness", "state": "detected"}
              ],
              "absent": [],
              "availability": "complete"
            }"#,
        );
        let members = roster_members(&outcome);
        let broker = members.last().expect("the broker closes the surface");
        assert!(matches!(broker, RosterMember::Broker));

        let generics: Vec<_> = members
            .iter()
            .filter_map(|m| match m {
                RosterMember::Descriptor { entry, overlay } if overlay.is_none() => {
                    Some(entry.slug.as_str())
                }
                _ => None,
            })
            .collect();
        assert_eq!(generics, vec!["future-harness"]);

        // pi joins its overlay by catalog slug; the record's name for the row
        // is the overlay's CLI-facing name, not the slug.
        let pi = members.iter().find_map(|m| match m {
            RosterMember::Descriptor {
                overlay: Some(o), ..
            } if o.catalog_slug == "pi" => Some(o.name),
            _ => None,
        });
        assert_eq!(pi, Some("pi"));

        // Every overlay slug the record does not name is an unrecorded row.
        let unrecorded: Vec<_> = members
            .iter()
            .filter_map(|m| match m {
                RosterMember::Unrecorded { overlay } => Some(overlay.catalog_slug),
                _ => None,
            })
            .collect();
        assert_eq!(
            unrecorded.len(),
            OVERLAYS.len() - 1,
            "the record named exactly one overlay slug (pi)"
        );
        assert!(!unrecorded.contains(&"pi"));
    }

    #[test]
    fn an_unreadable_record_leaves_the_overlays_and_the_broker_only() {
        let outcome = DetectionOutcome::Unavailable {
            reason: "no bin".to_string(),
        };
        let members = roster_members(&outcome);
        assert!(
            members
                .iter()
                .all(|m| matches!(m, RosterMember::Unrecorded { .. } | RosterMember::Broker))
        );
        assert_eq!(
            members
                .iter()
                .filter(|m| matches!(m, RosterMember::Broker))
                .count(),
            1,
            "exactly one broker row"
        );
    }

    #[test]
    fn the_overlay_surface_is_keyed_by_catalog_slug() {
        let mut names: Vec<&str> = OVERLAYS.iter().map(|o| o.name).collect();
        let mut slugs: Vec<&str> = OVERLAYS.iter().map(|o| o.catalog_slug).collect();
        for checked in [&mut names, &mut slugs] {
            let count = checked.len();
            checked.sort_unstable();
            checked.dedup();
            assert_eq!(
                checked.len(),
                count,
                "overlay names and slugs must be unique"
            );
        }
        for overlay in OVERLAYS {
            assert!(
                !overlay.catalog_slug.trim().is_empty(),
                "{} must carry a catalog slug: an overlay without one is unrepresentable",
                overlay.name
            );
            // The SelfOwned reach belongs to the broker alone (outside this
            // surface): an overlay is detail ON a catalog slug, so it cannot
            // also be the harness that owns its own config home. The broker
            // carries its real builder through `broker_reach` instead of an
            // overlay-shaped admission-less stub.
            assert!(
                !matches!(overlay.reach, Reach::SelfOwned { .. }),
                "{} must not claim the self-owned reach; the broker is its one resident",
                overlay.name
            );
        }
    }

    #[test]
    fn every_harness_client_effects_arm_resolves_detail_ground() {
        // The harness targets `app::client_effects` dispatches on, pinned here
        // so a harness added to the effects dispatch without detail ground
        // fails this test instead of silently diverging. The client-status
        // roster, by contrast, derives from detection — an effects arm is a
        // context-binding surface, not a roster claim.
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
            TargetId::HERMES,
            TargetId::HERMES_ACP,
        ];
        for target in dispatched {
            let slug = aikit_adapters::profiles::slug_for_target(&TargetId::new(*target))
                .unwrap_or_else(|| panic!("{target} is dispatched but joins no catalog slug"));
            assert!(
                aikit_adapters::profiles::for_slug(slug).is_some(),
                "{target} joins to profile `{slug}` but no embedded profile carries it"
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
             extend the guard and the overlay together with the dispatch"
        );
    }

    #[test]
    fn aliases_resolve_to_the_same_entry() {
        for (alias, name) in [
            ("claude-code", "claude"),
            ("antigravity", TargetId::ANTIGRAVITY),
            ("gemini", TargetId::GEMINI_CLI),
            ("grokbot", TargetId::GROK_BOT),
        ] {
            let via_alias = client_overlay(alias).expect("alias must resolve");
            let via_name = client_overlay(name).expect("name must resolve");
            assert!(
                std::ptr::eq(via_alias, via_name),
                "{alias} and {name} must be the same overlay entry"
            );
        }
        // A catalog slug resolves to the same entry as its client name —
        // the gemini precedent on the join key.
        let via_slug = client_overlay(TargetId::GEMINI).expect("catalog slug must resolve");
        let via_name = client_overlay(TargetId::GEMINI_CLI).expect("name must resolve");
        assert!(std::ptr::eq(via_slug, via_name));
    }

    #[test]
    fn every_overlay_admission_builds() {
        // The admission census of every overlaid harness must satisfy the gap
        // contract, or a gap row would lose its structured disclosure.
        let dirs = ClientDirs {
            ctx_dir: PathBuf::from("/tmp/ctx"),
            tree: PathBuf::from("/tmp/tree"),
            home: PathBuf::from("/tmp/home"),
        };
        for overlay in OVERLAYS {
            let descriptor = (overlay.admission)(&dirs);
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
                    overlay.name,
                    error.message()
                )
            });
        }
    }

    #[test]
    fn the_surface_is_derived_never_a_literal() {
        let source = include_str!("client.rs");
        // Split the needles so this check's own text cannot match them.
        let loop_needle = format!("for {} in [", "client");
        assert!(
            !source.contains(&loop_needle),
            "the client roster must be enumerated from the derived members, not a literal loop"
        );
        assert!(
            source.contains("for member in &members"),
            "status must enumerate the derived roster members"
        );
        let roster_needle = format!("\"claude\", \"{}\", \"zcode\", \"{}\"", "codex", "broker");
        assert!(
            !source.contains(&roster_needle),
            "the client roster literal must not return"
        );
    }
}
