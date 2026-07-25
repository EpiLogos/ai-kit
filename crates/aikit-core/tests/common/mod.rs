//! Fixtures shared by the resolver tests.
//!
//! These build real `Capsule` values by parsing real manifest text, rather than
//! hand-constructing structs, so the tests exercise the same path a registry on
//! disk would take.

#![allow(dead_code)]

use aikit_core::capsule::Capsule;
use aikit_core::catalog::{Catalog, MemoryCatalog};
use aikit_core::context::{ContextDescriptor, Isolation};
use aikit_core::id::{CapsuleId, ContextId, ProfileId, RegistrySource, Revision, SessionId};
use aikit_core::platform::{Platform, TargetId};
use aikit_core::policy::ManagedPolicy;
use aikit_core::profile::{PoolPatch, Profile};
use aikit_core::resolve::{resolve, resolve_diagnostic, Diagnosis, ResolveRequest, ResolvedView};
use aikit_core::scope::{LayerOrigin, ScopeKind, ScopeLayer};
use aikit_core::trust::{MemoryTrust, TrustState};
use aikit_core::AikitError;

pub fn cid(s: &str) -> CapsuleId {
    CapsuleId::parse(s).unwrap()
}

pub fn pid(s: &str) -> ProfileId {
    ProfileId::parse(s).unwrap()
}

/// Build a capsule manifest. `top` is spliced in at the document's top level
/// (before any table), `section` inside the kind's own table.
fn manifest(kind: &str, id: &str, top: &str, section_body: &str, section_extra: &str) -> Capsule {
    let leaf = id.rsplit('/').next().unwrap();
    let src = format!(
        r#"schema = 1
id = "{id}"
kind = "{kind}"
name = "{leaf}"
description = "Test {kind} {leaf}."
{top}

[{kind}]
{section_body}
{section_extra}
"#
    );
    stamp(
        Capsule::from_toml_str(&src)
            .unwrap_or_else(|e| panic!("fixture manifest for {id} should parse: {e}\n---\n{src}")),
    )
}

/// A minimal script capsule with the given id.
pub fn script(id: &str) -> Capsule {
    script_with(id, "")
}

/// `top` is spliced in at the top level of the manifest.
pub fn script_with(id: &str, top: &str) -> Capsule {
    manifest("script", id, top, "entry = \"payload/run.sh\"", "")
}

/// A script capsule that exports the given command names.
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
        "entry = \"payload/run.sh\"",
        &format!("exports = [{rendered}]"),
    )
}

pub fn skill(id: &str) -> Capsule {
    skill_with(id, "")
}

pub fn skill_with(id: &str, top: &str) -> Capsule {
    manifest("skill", id, top, "root = \"payload\"", "")
}

pub fn hook(id: &str) -> Capsule {
    hook_with(id, "")
}

pub fn hook_with(id: &str, top: &str) -> Capsule {
    manifest(
        "hook",
        id,
        top,
        "entry = \"payload/check\"\nevents = [\"PreToolUse\"]",
        "",
    )
}

/// A hook capsule with full control over its `[hook]` table, for the chain tests.
pub fn hook_table(id: &str, top: &str, hook_body: &str) -> Capsule {
    manifest("hook", id, top, hook_body, "")
}

/// A guidance capsule with full control over its `[guidance]` table.
pub fn guidance_table(id: &str, top: &str, guidance_body: &str) -> Capsule {
    manifest("guidance", id, top, guidance_body, "")
}

pub fn guidance(id: &str) -> Capsule {
    guidance_with(id, "")
}

pub fn guidance_with(id: &str, top: &str) -> Capsule {
    manifest("guidance", id, top, "entry = \"payload/guidance.md\"", "")
}

/// A capsule of the given kind that requires the listed capsules.
pub fn requiring(kind: &str, id: &str, requires: &[&str]) -> Capsule {
    let top = requires
        .iter()
        .map(|r| format!("\n[[requires]]\nid = \"{r}\"\n"))
        .collect::<String>();
    match kind {
        "script" => script_with(id, &top),
        "skill" => skill_with(id, &top),
        "hook" => hook_with(id, &top),
        "guidance" => guidance_with(id, &top),
        other => panic!("unsupported fixture kind {other}"),
    }
}

/// Give a fixture capsule the registry facts the store would attach.
fn stamp(mut c: Capsule) -> Capsule {
    let rev = blake3::hash(c.id.to_string().as_bytes());
    c.revision = Some(Revision::from_hash(rev));
    c.source = Some(RegistrySource::personal());
    c
}

/// Same, for fixtures built in the test file itself.
pub fn stamp_public(c: Capsule) -> Capsule {
    stamp(c)
}

pub fn profile(id: &str, enable: &[&str], disable: &[&str]) -> Profile {
    Profile {
        id: pid(id),
        description: format!("Test profile {id}"),
        extends: vec![],
        extends_uses: vec![],
        params: Default::default(),
        template: Default::default(),
        patch: PoolPatch {
            profiles: vec![],
            uses: vec![],
            enable: enable.iter().map(|s| cid(s)).collect(),
            disable: disable.iter().map(|s| cid(s)).collect(),
            config: Default::default(),
        },
    }
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
        },
    }
}

pub fn layer_using(kind: ScopeKind, profiles: &[&str]) -> ScopeLayer {
    ScopeLayer {
        kind,
        depth: 0,
        origin: LayerOrigin::new(format!("test:{}", kind.as_str())),
        patch: PoolPatch {
            profiles: profiles.iter().map(|s| pid(s)).collect(),
            uses: vec![],
            enable: vec![],
            disable: vec![],
            config: Default::default(),
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

/// A trust oracle that treats every fixture capsule as reviewed.
pub fn trusting(catalog: &MemoryCatalog) -> MemoryTrust {
    let mut trust = MemoryTrust::default();
    for capsule in catalog.capsules() {
        trust.set(
            capsule.source.clone().unwrap(),
            capsule.id.clone(),
            capsule.revision.clone().unwrap(),
            TrustState::Reviewed,
        );
    }
    trust
}

pub struct Fixture {
    pub catalog: MemoryCatalog,
    pub trust: MemoryTrust,
    pub layers: Vec<ScopeLayer>,
    pub policy: ManagedPolicy,
    pub descriptor: ContextDescriptor,
}

impl Fixture {
    pub fn new(capsules: Vec<Capsule>) -> Self {
        let mut catalog = MemoryCatalog::default();
        for c in capsules {
            catalog.insert(c);
        }
        let trust = trusting(&catalog);
        Self {
            catalog,
            trust,
            layers: vec![],
            policy: ManagedPolicy::default(),
            descriptor: descriptor(),
        }
    }

    pub fn with_profiles(mut self, profiles: Vec<Profile>) -> Self {
        for p in profiles {
            self.catalog.insert_profile(p);
        }
        self
    }

    pub fn with_layers(mut self, layers: Vec<ScopeLayer>) -> Self {
        self.layers = layers;
        self
    }

    pub fn with_policy(mut self, policy: ManagedPolicy) -> Self {
        self.policy = policy;
        self
    }

    pub fn with_descriptor(mut self, d: ContextDescriptor) -> Self {
        self.descriptor = d;
        self
    }

    pub fn untrust(mut self, id: &str) -> Self {
        let capsule = self.catalog.get(&cid(id)).unwrap().clone();
        self.trust.set(
            capsule.source.clone().unwrap(),
            capsule.id.clone(),
            capsule.revision.clone().unwrap(),
            TrustState::Unseen,
        );
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
        self
    }

    pub fn catalog_contains(&self, id: &str) -> bool {
        self.catalog.get(&cid(id)).is_some()
    }

    /// Simulate an edited payload: a new content revision, dropping back to review.
    pub fn bump_revision(&mut self, id: &str) {
        let mut capsule = self.catalog.get(&cid(id)).unwrap().clone();
        let next = Revision::from_hash(blake3::hash(format!("{id}:v2").as_bytes()));
        capsule.revision = Some(next.clone());
        self.catalog.insert(capsule.clone());
        self.trust.set(
            capsule.source.clone().unwrap(),
            capsule.id.clone(),
            next,
            TrustState::Reviewed,
        );
    }

    pub fn request(&self) -> ResolveRequest {
        ResolveRequest {
            context: self.descriptor.clone(),
            layers: self.layers.clone(),
            policy: self.policy.clone(),
        }
    }

    pub fn resolve(&self) -> Result<ResolvedView, AikitError> {
        resolve(&self.catalog, &self.trust, &self.request())
    }

    pub fn diagnose(&self) -> Diagnosis {
        resolve_diagnostic(&self.catalog, &self.trust, &self.request())
    }
}

pub fn active_ids(view: &ResolvedView) -> Vec<String> {
    view.active.keys().map(|id| id.to_string()).collect()
}
