//! Ollama — harness-adapter admission through the harness-adapter contract.
//!
//! Ollama is NOT an agentic coding harness. Actuation's own catalog describes it
//! as `native_kind: "model-provider"`, edition `cli+service`, "local model
//! runtime and provider; detected for model binding, not agency". The installed
//! binary agrees: `ollama --help` (0.12.6) opens with "Large language model
//! runner" and exposes only serve/create/show/run/stop/pull/push/list/ps/cp/rm
//! — model lifecycle commands, no instruction files, no sessions, no skills,
//! no hooks, no agents. This adapter therefore censuses a model server for what
//! it actually is: the overwhelming majority of the 15 harness faculties are
//! Unsupported, each with its reason, and the census says so explicitly rather
//! than padding with invented surfaces.
//!
//! Evidence base (primary sources, observed 2026-09-06/07):
//! - Actuation detection record (my own run): `actuation harness detect
//!   --json --versions` → ollama detected at /usr/local/bin/ollama, version
//!   0.12.6, service probe http 200 from http://127.0.0.1:11434, models facet
//!   count 3 under ~/.ollama/models. `actuation harness capability ollama`
//!   → undeclared (only claude-code, codex, zcode are declared), noted honestly.
//! - `actuation harness catalog --json` (catalog_revision 4) → descriptor
//!   edition "cli+service", native_owner "Ollama", probes executable+config-dir+
//!   service(http), facets models:~/.ollama/models.
//! - `ollama --help` / `ollama --version` = 0.12.6; `ollama list` via the
//!   running service returned no rows at observation time.
//! - Repo ollama/ollama docs (raw.githubusercontent.com, verified 200):
//!   docs/modelfile.mdx (Modelfile SYSTEM/TEMPLATE are baked at model-authoring
//!   time, not a runtime instruction surface), docs/api/openai-compatibility.mdx
//!   (generation-only API), README.md.
//!
//! ## Identity law
//!
//! A model running behind Ollama's API is not the Agent identity; Ollama is not
//! the World; an ollama serve process is not an AgentSession. The adapter keeps
//! `target`, `product`, and any `realised_actuation_ref` distinct, and never
//! fabricates a loaded-activation claim (`verify_activation_truth` rejects it).
//!
//! ## Projection stance
//!
//! There is no genuine on-disk instruction/skill/session surface a projection
//! could write that Ollama would read: ~/.ollama/models is model content, not
//! configuration the runtime interprets for behaviour. Projection (model
//! binding) is therefore brokered with an honest note; no tree is invented.
use std::path::{Path, PathBuf};

use aikit_core::harness_admission::{
    FacultySupport, HarnessAdmissionAdapter, HarnessAdmissionDescriptor, HarnessEditionKind,
    HarnessFaculty, HarnessFacultyObservation, HARNESS_ADAPTER_SDK_VERSION,
};
use aikit_core::platform::TargetId;
use aikit_core::projection::{
    ActivationEffect, ProjectionPlan, ResolvedContext, TargetAdapter, TargetCapabilities,
};
use aikit_core::Result;

pub const CLIENT: &str = "ollama";
pub const PRODUCT: &str = "Ollama";
/// Adapter identity inside the admission contract; stable, not the harness's own name.
pub const ADAPTER_REF: &str = "aikit:ollama-adapter";

/// Stable evidence refs pointing at primary sources, not prose.
const EV_NATIVE_HELP: &str = "native:ollama --help (0.12.6) = \"Large language model runner\"";
const EV_NATIVE_VERSION: &str = "native:ollama --version = 0.12.6";
const EV_NATIVE_LIST: &str = "native:ollama list (via service :11434) returned no rows";
const EV_MODELFILE: &str = "doc:github.com/ollama/ollama/main/docs/modelfile.mdx";
const EV_API: &str = "doc:github.com/ollama/ollama/main/docs/api/openai-compatibility.mdx";
const EV_README: &str = "doc:github.com/ollama/ollama/main/README.md";
const EV_MODELS_DIR: &str =
    "native:/Users/admin/.ollama/models (blobs+manifests; detection facet models:3)";
const EV_CAPABILITY_UNDECLARED: &str =
    "native:actuation harness capability ollama -> undeclared (declared: claude-code, codex, zcode)";
/// Actuation owns detection; AIKit consumes the record. Cited per the
/// `actuation.harness-detection/v1` schema (catalog r4); this is the record my
/// own `actuation harness detect --json --versions` run produced.
const EV_DETECTION: &str = "actuation.harness-detection/v1 detection:2026-09-06T23:42:43.943Z ollama:detected exe:/usr/local/bin/ollama sha256:5213ff550ef0be235af79afe5e239636e36fac44c5a1cedf62b26ecd604cdcfe observed:2026-09-06T23:42:43.943Z service:http-200-127.0.0.1:11434 facets:models(3) version:0.12.6";

pub struct OllamaAdapter {
    /// Where a future native model-binding projection would be written. Unused
    /// by the brokered plan in this revision; kept as the explicit
    /// projection-root seam so the next slice does not re-derive it.
    root: PathBuf,
}

impl OllamaAdapter {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

fn faculty(
    faculty: HarnessFaculty,
    support: FacultySupport,
    evidence: &[&str],
    note: Option<&str>,
) -> HarnessFacultyObservation {
    HarnessFacultyObservation {
        faculty,
        support,
        evidence_refs: evidence.iter().map(|s| (*s).to_string()).collect(),
        note: note.map(|s| s.to_string()),
    }
}

fn ollama_faculties() -> Vec<HarnessFacultyObservation> {
    vec![
        faculty(
            HarnessFaculty::StandingInstructions,
            FacultySupport::Unsupported,
            &[EV_MODELFILE, EV_NATIVE_HELP],
            Some(
                "no per-user standing-instruction file is read at runtime; the only \
                 instruction carrier is the Modelfile SYSTEM/TEMPLATE baked into a model \
                 at authoring time (`ollama create`), which is model content, not a \
                 harness instruction surface",
            ),
        ),
        faculty(
            HarnessFaculty::ProjectInstructions,
            FacultySupport::Unsupported,
            &[EV_NATIVE_HELP, EV_DETECTION],
            Some(
                "no per-project instruction surface; the CLI+service surface is model \
                 lifecycle only (serve/create/run/list/...)",
            ),
        ),
        faculty(
            HarnessFaculty::NativeSkills,
            FacultySupport::Unsupported,
            &[EV_NATIVE_HELP],
            Some("no skill concept anywhere in the ollama CLI or API"),
        ),
        faculty(
            HarnessFaculty::SessionStartHook,
            FacultySupport::Unsupported,
            &[EV_NATIVE_HELP, EV_API],
            Some("no session concept; the API is a per-request generation endpoint"),
        ),
        faculty(
            HarnessFaculty::LiveReload,
            FacultySupport::Unsupported,
            &[EV_API, EV_NATIVE_HELP],
            Some("nothing to reload: no instruction/session state exists to re-read"),
        ),
        faculty(
            HarnessFaculty::NextSessionReload,
            FacultySupport::Unsupported,
            &[EV_API],
            Some("no sessions; every request is independent"),
        ),
        faculty(
            HarnessFaculty::RestartReload,
            FacultySupport::Unsupported,
            &[EV_API, EV_MODELFILE],
            Some(
                "service restart picks up no authored configuration beyond model blobs; \
                 new models are registered by `ollama create/pull`, not by restart",
            ),
        ),
        faculty(
            HarnessFaculty::ToolProtocol,
            FacultySupport::Unsupported,
            &[EV_API, EV_NATIVE_HELP],
            Some(
                "the server is generation-only; the OpenAI-compatible endpoint accepts \
                 chat completions but never executes tool calls — tool use is entirely \
                 client-side prompt/template work",
            ),
        ),
        faculty(
            HarnessFaculty::NativeToolContribution,
            FacultySupport::Unsupported,
            &[EV_NATIVE_HELP],
            Some("no plugin/tool-contribution mechanism; commands are model lifecycle only"),
        ),
        faculty(
            HarnessFaculty::SessionResume,
            FacultySupport::Unsupported,
            &[EV_API],
            Some(
                "no conversation persistence; model keep-alive (ollama ps / stop) is GPU \
                 memory residency, not session resume",
            ),
        ),
        faculty(
            HarnessFaculty::DelegatedAgents,
            FacultySupport::Unsupported,
            &[EV_NATIVE_HELP, EV_DETECTION, EV_CAPABILITY_UNDECLARED],
            Some(
                "no agent/subagent concept; Actuation's own catalog classifies ollama as \
                 native_kind model-provider, \"detected for model binding, not agency\", \
                 and `actuation harness capability ollama` is undeclared (only claude-code, \
                 codex, zcode are)",
            ),
        ),
        faculty(
            HarnessFaculty::ProjectRoots,
            FacultySupport::Unsupported,
            &[EV_MODELS_DIR],
            Some(
                "~/.ollama/models holds model blobs/manifests (detection facet models:3); \
                 it is model content storage, not a project-root/workspace surface",
            ),
        ),
        faculty(
            HarnessFaculty::Components,
            FacultySupport::Unsupported,
            &[EV_NATIVE_HELP],
            Some("no UI-component or extension tree"),
        ),
        faculty(
            HarnessFaculty::Surfaces,
            FacultySupport::Degraded,
            &[
                EV_DETECTION,
                EV_API,
                EV_NATIVE_HELP,
                EV_NATIVE_VERSION,
                EV_README,
            ],
            Some(
                "the only surfaces are the local HTTP API (detection service probe: \
                 http 200 from http://127.0.0.1:11434) and the interactive `ollama run` \
                 REPL; there is no agent-facing UI surface comparable to harness Slots/TUI",
            ),
        ),
        faculty(
            HarnessFaculty::LiveRetraction,
            FacultySupport::Unsupported,
            &[EV_NATIVE_HELP, EV_NATIVE_LIST],
            Some(
                "no projection lifecycle to retract from; `ollama rm` removes model \
                 content, not projected instructions (and `ollama list` showed no models \
                 at observation time despite the models-dir facet count of 3)",
            ),
        ),
    ]
}

impl TargetAdapter for OllamaAdapter {
    fn target(&self) -> TargetId {
        TargetId::new(CLIENT)
    }

    fn capabilities(&self) -> TargetCapabilities {
        TargetCapabilities {
            // A model server has no reloadable authored surface at all.
            live_reload: false,
            symlinks: false,
            isolated_per_context: false,
            requires_isolated_tree_for_isolation: false,
            brokered_fallback: true,
            watches_for_changes: false,
        }
    }

    fn plan(&self, _context: &ResolvedContext) -> Result<ProjectionPlan> {
        Ok(ProjectionPlan::new(
            self.target(),
            ActivationEffect::brokered(
                "Ollama is a local model runtime (cli+service), not an agent harness; \
                 there is no instruction/skill/session surface to project onto, so \
                 model binding is brokered through AIKit rather than written to disk",
            ),
        )
        .with_note(
            "Actuation's catalog classifies ollama as native_kind model-provider, \
             edition cli+service (catalog r4, detected at /usr/local/bin/ollama 0.12.6, \
             service :11434); ~/.ollama/models is model content the runtime serves, not \
             configuration it interprets — writing a projection tree there would invent \
             a surface ollama does not read"
                .to_string(),
        ))
    }

    fn activation_effect(
        &self,
        old: Option<&ProjectionPlan>,
        new: &ProjectionPlan,
    ) -> ActivationEffect {
        if matches!(
            new.effect,
            ActivationEffect::Brokered { .. } | ActivationEffect::Unsupported { .. }
        ) {
            return new.effect.clone();
        }
        if new.is_noop_against(old) {
            ActivationEffect::immediate("already projected")
        } else {
            new.effect.clone()
        }
    }
}

impl HarnessAdmissionAdapter for OllamaAdapter {
    fn admission(&self) -> HarnessAdmissionDescriptor {
        HarnessAdmissionDescriptor {
            schema: HARNESS_ADAPTER_SDK_VERSION.to_string(),
            adapter_ref: ADAPTER_REF.to_string(),
            adapter_version: env!("CARGO_PKG_VERSION").to_string(),
            target: self.target(),
            product: PRODUCT.to_string(),
            // Catalog descriptor edition is "cli+service" (model runtime), which
            // has no HarnessEditionKind variant; Custom is the honest mapping.
            edition: HarnessEditionKind::Custom,
            // Observed locally: `ollama --version` = 0.12.6.
            native_version: Some("0.12.6".to_string()),
            source_revision: None,
            // Bound to Actuation's detection identity for this harness
            // (`actuation harness detect`, catalog r4); AIKit consumes the
            // ref, it does not mint it. `actuation harness capability ollama`
            // is undeclared (only claude-code, codex, zcode are) — noted in
            // the census rather than fabricated.
            realised_actuation_ref: Some("harness/ollama".to_string()),
            project_binding_ref: None,
            faculties: ollama_faculties(),
        }
    }
}
