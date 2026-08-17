//! Fixtures for the V2 application-surface tests.
//!
//! Everything here is built from real manifest text through
//! `Capsule::from_toml_str` and resolved by the real `aikit_core::resolve`, so a
//! surface test asserting availability or trust still exercises the resolver's
//! actual answer. No fixture helper drives a retired Palette/Tree controller.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use aikit_core::arg::ArgValues;
use aikit_core::capsule::{Capsule, Kind};
use aikit_core::catalog::{Catalog, MemoryCatalog};
use aikit_core::context::{ContextDescriptor, Isolation};
use aikit_core::id::{
    CapsuleId, ContextId, GenerationId, ProfileId, RegistrySource, Revision, SessionId,
};
use aikit_core::platform::{Platform, TargetId};
use aikit_core::policy::ManagedPolicy;
use aikit_core::profile::PoolPatch;
use aikit_core::projection::ActivationEffect;
use aikit_core::resolve::{resolve, ResolveRequest, ResolvedView};
use aikit_core::scope::{LayerOrigin, ScopeKind, ScopeLayer};
use aikit_core::search::{SearchDoc, UsageStats};
use aikit_core::trust::{MemoryTrust, TrustState};
use aikit_core::{AikitError, Result};

use aikit_store::events::Timestamp;
use aikit_store::inbox::{Candidate, CandidateState, PromotionEdits, Similarity, SimilarityBasis};
use aikit_store::scan::{Family, Finding};

use aikit_tui::backend::{
    ClientEffect, JobOutput, PaletteBackend, Projected, PromotionDraft, RunIntent, Toggle,
};

pub fn cid(s: &str) -> CapsuleId {
    CapsuleId::parse(s).unwrap()
}

/// Build a capsule from real manifest text and stamp it with the registry facts
/// the store would attach.
pub fn manifest(kind: &str, id: &str, top: &str, section: &str) -> Capsule {
    let leaf = id.rsplit('/').next().unwrap();
    let src = format!(
        r#"schema = 1
id = "{id}"
kind = "{kind}"
name = "{leaf}"
description = "Test {kind} {leaf}."
{top}

[{kind}]
{section}
"#
    );
    let mut capsule = Capsule::from_toml_str(&src)
        .unwrap_or_else(|e| panic!("fixture manifest for {id} should parse: {e}\n---\n{src}"));
    capsule.revision = Some(Revision::from_raw(format!("rev-{id}")));
    capsule.source = Some(RegistrySource::personal());
    capsule.root = Some(PathBuf::from(format!("/registry/{}", id.replace('/', "-"))));
    capsule
}

pub fn script(id: &str) -> Capsule {
    manifest("script", id, "", "entry = \"payload/run.sh\"")
}

pub fn script_with(id: &str, top: &str) -> Capsule {
    manifest("script", id, top, "entry = \"payload/run.sh\"")
}

pub fn script_exporting(id: &str, exports: &[&str]) -> Capsule {
    let rendered = exports
        .iter()
        .map(|e| format!("\"{e}\""))
        .collect::<Vec<_>>()
        .join(", ");
    manifest(
        "script",
        id,
        "",
        &format!("entry = \"payload/run.sh\"\nexports = [{rendered}]"),
    )
}

pub fn skill(id: &str) -> Capsule {
    manifest("skill", id, "", "root = \"payload\"")
}

pub fn skill_with(id: &str, top: &str) -> Capsule {
    manifest("skill", id, top, "root = \"payload\"")
}

pub fn hook(id: &str) -> Capsule {
    manifest(
        "hook",
        id,
        "",
        "entry = \"payload/check\"\nevents = [\"PreToolUse\"]",
    )
}

pub fn requiring(kind: &str, id: &str, requires: &[&str]) -> Capsule {
    let top: String = requires
        .iter()
        .map(|r| format!("\n[[requires]]\nid = \"{r}\"\n"))
        .collect();
    match kind {
        "script" => script_with(id, &top),
        "skill" => skill_with(id, &top),
        other => panic!("unsupported fixture kind {other}"),
    }
}

pub fn conflicting(id: &str, with: &str) -> Capsule {
    script_with(id, &format!("\n[[conflicts]]\nid = \"{with}\"\n"))
}

pub fn layer(kind: ScopeKind, enable: &[&str], disable: &[&str]) -> ScopeLayer {
    ScopeLayer {
        kind,
        depth: 0,
        origin: LayerOrigin::new(format!("test:{}", kind.as_str())),
        patch: PoolPatch {
            profiles: vec![],
            uses: vec![],
            enable: enable.iter().map(|s| cid(s)).collect(),
            disable: disable.iter().map(|s| cid(s)).collect(),
            config: Default::default(),
            skill_overlays: Default::default(),
        },
    }
}

pub fn descriptor() -> ContextDescriptor {
    ContextDescriptor {
        context_id: ContextId::parse("ctx_TESTCONTEXT000000000000").unwrap(),
        session_id: Some(SessionId::parse("ses_TESTSESSION000000000000").unwrap()),
        project_id: None,
        project_root: Some("/work/payments".into()),
        task: None,
        isolation: Isolation::Shared,
        platform: Platform::Linux,
        targets: vec![TargetId::shell(), TargetId::claude_code()],
        mux: None,
        host: "test-host".into(),
    }
}

// ---------------------------------------------------------------------------
// A backend over a real catalog and a real overlay file
// ---------------------------------------------------------------------------

/// A [`PaletteBackend`] backed by the real resolver and a real overlay file on
/// disk. `PaletteBackend` is currently only the compatibility name of the shared
/// application backend contract; tests interact through `ApplicationService` and
/// `ApplicationSurfaceController`.
pub struct Fixture {
    pub catalog: MemoryCatalog,
    pub trust: MemoryTrust,
    pub descriptor: ContextDescriptor,
    pub policy: ManagedPolicy,
    layers: BTreeMap<ScopeKind, PoolPatch>,
    overlay_path: PathBuf,
    view: ResolvedView,
    pub usage: BTreeMap<CapsuleId, UsageStats>,
    pub effects: Vec<ClientEffect>,
    pub recent: Vec<RunIntent>,
    pub drafts: Vec<PromotionDraft>,
    pub job: JobOutput,
    pub applied: Vec<(ScopeKind, Vec<Toggle>)>,
    pub promoted: Vec<CapsuleId>,
}

impl Fixture {
    pub fn new(dir: &std::path::Path, capsules: Vec<Capsule>) -> Self {
        let mut catalog = MemoryCatalog::default();
        for capsule in capsules {
            catalog.insert(capsule);
        }
        let mut trust = MemoryTrust::default();
        for capsule in catalog.capsules() {
            trust.set(
                capsule.source.clone().unwrap(),
                capsule.id.clone(),
                capsule.revision.clone().unwrap(),
                TrustState::Reviewed,
            );
        }
        let descriptor = descriptor();
        let policy = ManagedPolicy::default();
        let view = resolve(
            &catalog,
            &trust,
            &ResolveRequest {
                context: descriptor.clone(),
                layers: vec![],
                policy: policy.clone(),
            },
        )
        .expect("an empty layer stack always resolves");
        let mut fixture = Self {
            catalog,
            trust,
            descriptor,
            policy,
            layers: BTreeMap::new(),
            overlay_path: dir.join("overlay.toml"),
            view,
            usage: BTreeMap::new(),
            effects: vec![ClientEffect {
                target: TargetId::claude_code(),
                effect: ActivationEffect::live(),
            }],
            recent: Vec::new(),
            drafts: Vec::new(),
            job: JobOutput::default(),
            applied: Vec::new(),
            promoted: Vec::new(),
        };
        fixture.write_overlay();
        fixture.refresh();
        fixture
    }

    pub fn enable(mut self, scope: ScopeKind, ids: &[&str]) -> Self {
        let patch = self.layers.entry(scope).or_default();
        for id in ids {
            patch.set(&cid(id), true);
        }
        self.write_overlay();
        self.refresh();
        self
    }

    pub fn disable(mut self, scope: ScopeKind, ids: &[&str]) -> Self {
        let patch = self.layers.entry(scope).or_default();
        for id in ids {
            patch.set(&cid(id), false);
        }
        self.write_overlay();
        self.refresh();
        self
    }

    pub fn set_trust(mut self, id: &str, state: TrustState) -> Self {
        let capsule = self.catalog.get(&cid(id)).unwrap().clone();
        self.trust.set(
            capsule.source.clone().unwrap(),
            capsule.id.clone(),
            capsule.revision.clone().unwrap(),
            state,
        );
        self.refresh();
        self
    }

    pub fn with_descriptor(mut self, descriptor: ContextDescriptor) -> Self {
        self.descriptor = descriptor;
        self.refresh();
        self
    }

    pub fn with_effects(mut self, effects: Vec<ClientEffect>) -> Self {
        self.effects = effects;
        self
    }

    pub fn with_usage(mut self, id: &str, usage: UsageStats) -> Self {
        self.usage.insert(cid(id), usage);
        self
    }

    pub fn with_recent(mut self, recent: Vec<RunIntent>) -> Self {
        self.recent = recent;
        self
    }

    pub fn with_drafts(mut self, drafts: Vec<PromotionDraft>) -> Self {
        self.drafts = drafts;
        self
    }

    pub fn with_job(mut self, job: JobOutput) -> Self {
        self.job = job;
        self
    }

    pub fn overlay_path(&self) -> &std::path::Path {
        &self.overlay_path
    }

    pub fn overlay_bytes(&self) -> Vec<u8> {
        std::fs::read(&self.overlay_path).expect("the fixture overlay must exist")
    }

    fn scope_layers(&self) -> Vec<ScopeLayer> {
        self.layers
            .iter()
            .map(|(kind, patch)| ScopeLayer {
                kind: *kind,
                depth: 0,
                origin: LayerOrigin::new(format!(".aikit/{}.toml", kind.as_str())),
                patch: patch.clone(),
            })
            .collect()
    }

    fn write_overlay(&self) {
        let mut out = String::from("schema = 1\n");
        for (kind, patch) in &self.layers {
            out.push_str(&format!("\n[{}]\n", kind.as_str()));
            for id in &patch.enable {
                out.push_str(&format!("enable = \"{id}\"\n"));
            }
            for id in &patch.disable {
                out.push_str(&format!("disable = \"{id}\"\n"));
            }
        }
        std::fs::write(&self.overlay_path, out).expect("the fixture overlay must be writable");
    }

    fn resolve_with(&self, extra: &[ScopeLayer]) -> Result<ResolvedView> {
        let mut layers = self.scope_layers();
        layers.extend_from_slice(extra);
        resolve(
            &self.catalog,
            &self.trust,
            &ResolveRequest {
                context: self.descriptor.clone(),
                layers,
                policy: self.policy.clone(),
            },
        )
    }

    fn refresh(&mut self) {
        self.view = self
            .resolve_with(&[])
            .expect("the fixture's own layers must resolve");
    }

    fn toggle_layer(&self, scope: ScopeKind, toggles: &[Toggle]) -> ScopeLayer {
        let mut patch = self.layers.get(&scope).cloned().unwrap_or_default();
        for toggle in toggles {
            patch.set(&toggle.capsule, toggle.enable);
        }
        ScopeLayer {
            kind: scope,
            depth: 1,
            origin: LayerOrigin::new("staged"),
            patch,
        }
    }
}

impl PaletteBackend for Fixture {
    fn context(&self) -> &ContextDescriptor {
        &self.descriptor
    }

    fn view(&self) -> &ResolvedView {
        &self.view
    }

    fn documents(&self) -> Vec<SearchDoc> {
        self.view
            .catalog_index
            .keys()
            .filter_map(|id| {
                SearchDoc::from_view(
                    &self.view,
                    id,
                    self.usage.get(id).cloned().unwrap_or_default(),
                )
            })
            .collect()
    }

    fn capsule(&self, id: &CapsuleId) -> Option<&Capsule> {
        self.catalog.get(id)
    }

    fn preview(&self, scope: ScopeKind, toggles: &[Toggle]) -> Result<Projected> {
        let view = self.resolve_with(&[self.toggle_layer(scope, toggles)])?;
        Ok(Projected {
            view,
            effects: self.effects.clone(),
        })
    }

    fn apply(&mut self, scope: ScopeKind, toggles: &[Toggle]) -> Result<GenerationId> {
        let view = self.resolve_with(&[self.toggle_layer(scope, toggles)])?;
        let patch = self.layers.entry(scope).or_default();
        for toggle in toggles {
            patch.set(&toggle.capsule, toggle.enable);
        }
        self.write_overlay();
        self.applied.push((scope, toggles.to_vec()));
        self.view = view;
        Ok(self.view.hash.generation_id())
    }

    fn start(&mut self, _intent: &RunIntent) -> Result<JobOutput> {
        Ok(self.job.clone())
    }

    fn recent(&self) -> Vec<RunIntent> {
        self.recent.clone()
    }

    fn promotion_drafts(&self) -> Vec<PromotionDraft> {
        self.drafts.clone()
    }

    fn promote(&mut self, draft: &PromotionDraft) -> Result<CapsuleId> {
        if draft.withheld_reason().is_some() {
            return Err(AikitError::new(
                "inbox.quarantined",
                "a quarantined candidate is never promoted",
            ));
        }
        self.promoted.push(draft.edits.id.clone());
        Ok(draft.edits.id.clone())
    }
}

// ---------------------------------------------------------------------------
// Promotion fixtures retained for tests that exercise backend/package boundaries.
// ---------------------------------------------------------------------------

fn candidate(id: &str, title: &str, state: CandidateState, findings: Vec<Finding>) -> Candidate {
    Candidate {
        id: id.to_string(),
        kind: Kind::Script,
        title: title.to_string(),
        state,
        findings,
        body_hash: format!("hash-{id}"),
        normalized_hash: format!("norm-{id}"),
        exports: vec!["rebuild-index".into()],
        project_root: Some(PathBuf::from("/work/payments")),
        path: PathBuf::from(format!("/inbox/ready/{id}")),
        created_at: Timestamp::from_nanos(0),
    }
}

pub fn ready_draft() -> PromotionDraft {
    let candidate = candidate(
        "cnd_READY000000000000000000",
        "Rebuild the search index",
        CandidateState::Ready,
        vec![],
    );
    PromotionDraft::new(
        candidate,
        PromotionEdits::new(
            cid("script/ops/rebuild-index"),
            "Rebuilds the project search index from scratch.",
        )
        .with_exports(["rebuild-index"]),
    )
    .with_similar(vec![Similarity {
        other: "script/ops/reindex".into(),
        basis: SimilarityBasis::Shingles,
        percentage: 62,
        summary: "same three shell steps, different index path".into(),
    }])
    .with_body(vec![
        "#!/usr/bin/env bash".into(),
        "set -euo pipefail".into(),
        "cargo run --bin reindex".into(),
    ])
}

pub fn quarantined_draft() -> PromotionDraft {
    let candidate = candidate(
        "cnd_HELD0000000000000000000",
        "Deploy with the release token",
        CandidateState::Quarantined,
        vec![Finding {
            rule: "github-token".into(),
            family: Family::TokenPrefix,
            range: 0..12,
            preview: "ghp_…8 chars…".into(),
            description: "a GitHub personal access token".into(),
        }],
    );
    PromotionDraft::new(
        candidate,
        PromotionEdits::new(
            cid("script/ops/deploy-token"),
            "Deploys using a personal access token.",
        ),
    )
    .with_body(vec!["export GH_TOKEN=ghp_REALSECRETVALUE".into()])
}

pub fn values(pairs: &[(&str, aikit_core::arg::ArgValue)]) -> ArgValues {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

pub fn kind_of(id: &str) -> Kind {
    cid(id).kind()
}

pub fn profile_id(s: &str) -> ProfileId {
    ProfileId::parse(s).unwrap()
}
