//! Fixtures shared by the adapter tests.
//!
//! Session plans are built by compiling real session-spec TOML rather than by
//! hand-constructing `SessionPlan`, so the adapters are driven by exactly the
//! document a user would write.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use aikit_core::actor_bootstrap::ActorBootstrap;
use aikit_core::capsule::{Capsule, Kind};
use aikit_core::catalog::MemoryCatalog;
use aikit_core::context::{ContextDescriptor, Isolation};
use aikit_core::id::{CapsuleId, ContextId, RegistrySource, Revision, SessionId};
use aikit_core::platform::{Platform, TargetId};
use aikit_core::policy::ManagedPolicy;
use aikit_core::profile::PoolPatch;
use aikit_core::projection::ResolvedContext;
use aikit_core::resolve::{resolve, ResolveRequest};
use aikit_core::scope::{LayerOrigin, ScopeKind, ScopeLayer};
use aikit_core::session::{SessionPlan, SessionSpec};
use aikit_core::trust::{MemoryTrust, TrustState};

// ---------------------------------------------------------------------------
// Session plans
// ---------------------------------------------------------------------------

pub fn plan_from(toml_src: &str) -> SessionPlan {
    SessionSpec::from_toml_str(toml_src)
        .unwrap_or_else(|e| panic!("fixture session spec should parse: {e}"))
        .compile()
        .unwrap_or_else(|e| panic!("fixture session spec should compile: {e}"))
}

/// One view, one pane. The smallest plan that still creates something.
pub fn single_pane_plan(name: &str) -> SessionPlan {
    plan_from(&format!(
        r#"
schema = 1
id = "{name}"
name = "{name}"

[[views]]
id = "main"
[[views.panes]]
id = "shell"
"#
    ))
}

/// Two views; the first has a 30% right split and a 40% bottom split.
pub fn three_pane_plan(name: &str, root: &Path) -> SessionPlan {
    plan_from(&format!(
        r#"
schema = 1
id = "{name}"
name = "{name}"
root = "{}"

[[views]]
id = "code"
name = "code"
[[views.panes]]
id = "editor"
focus = true
[[views.panes]]
id = "tests"
split_from = "editor"
direction = "right"
ratio = 0.3
[[views.panes]]
id = "logs"
split_from = "editor"
direction = "down"
ratio = 0.4

[[views]]
id = "ops"
name = "ops"
[[views.panes]]
id = "watch"
"#,
        root.display()
    ))
}

// ---------------------------------------------------------------------------
// Resolved contexts, for the client adapters
// ---------------------------------------------------------------------------

pub fn cid(s: &str) -> CapsuleId {
    CapsuleId::parse(s).unwrap()
}

/// A skill capsule whose payload root is `payload`.
pub fn skill_capsule(id: &str, description: &str) -> Capsule {
    let leaf = id.rsplit('/').next().unwrap();
    stamp(
        Capsule::from_toml_str(&format!(
            r#"schema = 1
id = "{id}"
kind = "skill"
name = "{leaf}"
description = "{description}"

[skill]
root = "payload"
"#
        ))
        .unwrap_or_else(|e| panic!("fixture skill {id} should parse: {e}")),
    )
}

pub fn script_capsule(id: &str, description: &str) -> Capsule {
    let leaf = id.rsplit('/').next().unwrap();
    stamp(
        Capsule::from_toml_str(&format!(
            r#"schema = 1
id = "{id}"
kind = "script"
name = "{leaf}"
description = "{description}"

[script]
entry = "payload/run.sh"
"#
        ))
        .unwrap_or_else(|e| panic!("fixture script {id} should parse: {e}")),
    )
}

fn stamp(mut c: Capsule) -> Capsule {
    c.revision = Some(Revision::from_raw(format!("rev-{}", c.id.slug())));
    c.source = Some(RegistrySource::personal());
    c
}

pub fn descriptor(isolation: Isolation) -> ContextDescriptor {
    ContextDescriptor {
        context_id: ContextId::parse("ctx_TESTCONTEXT000000000000").unwrap(),
        session_id: Some(SessionId::parse("ses_TESTSESSION000000000000").unwrap()),
        project_id: None,
        project_root: Some("/work/payments".into()),
        task: Some("migration-review".into()),
        isolation,
        platform: Platform::Macos,
        targets: vec![
            TargetId::shell(),
            TargetId::claude_code(),
            TargetId::codex(),
        ],
        mux: None,
        host: "test-host".into(),
    }
}

fn layer(kind: ScopeKind, enable: &[&str]) -> ScopeLayer {
    ScopeLayer {
        kind,
        depth: 0,
        origin: LayerOrigin::new(format!("test:{}", kind.as_str())),
        patch: PoolPatch {
            profiles: vec![],
            uses: vec![],
            enable: enable.iter().map(|s| cid(s)).collect(),
            disable: vec![],
            config: Default::default(),
            skill_overlays: Default::default(),
        },
    }
}

/// A resolved context whose skills come from two different scopes, so the Codex
/// adapter's project-stable / session-delta split has something to divide.
pub struct ContextBuilder {
    capsules: Vec<Capsule>,
    project: Vec<String>,
    session: Vec<String>,
    isolation: Isolation,
    roots: BTreeMap<CapsuleId, PathBuf>,
    actor_bootstrap: Option<ActorBootstrap>,
}

impl ContextBuilder {
    pub fn new() -> Self {
        Self {
            capsules: Vec::new(),
            project: Vec::new(),
            session: Vec::new(),
            isolation: Isolation::Shared,
            roots: BTreeMap::new(),
            actor_bootstrap: None,
        }
    }

    #[must_use]
    pub fn isolation(mut self, isolation: Isolation) -> Self {
        self.isolation = isolation;
        self
    }

    #[must_use]
    pub fn actor_bootstrap(mut self, bootstrap: ActorBootstrap) -> Self {
        self.actor_bootstrap = Some(bootstrap);
        self
    }

    /// A skill enabled at project scope: every task in the tree gets it.
    #[must_use]
    pub fn project_skill(mut self, id: &str, description: &str, root: impl Into<PathBuf>) -> Self {
        self.capsules.push(skill_capsule(id, description));
        self.project.push(id.to_string());
        self.roots.insert(cid(id), root.into());
        self
    }

    /// A skill enabled by the session overlay: a delta this context alone has.
    #[must_use]
    pub fn session_skill(mut self, id: &str, description: &str, root: impl Into<PathBuf>) -> Self {
        self.capsules.push(skill_capsule(id, description));
        self.session.push(id.to_string());
        self.roots.insert(cid(id), root.into());
        self
    }

    #[must_use]
    pub fn project_script(mut self, id: &str, description: &str, root: impl Into<PathBuf>) -> Self {
        self.capsules.push(script_capsule(id, description));
        self.project.push(id.to_string());
        self.roots.insert(cid(id), root.into());
        self
    }

    pub fn build(self) -> ResolvedContext {
        let mut catalog = MemoryCatalog::default();
        for c in &self.capsules {
            catalog.insert(c.clone());
        }
        let mut trust = MemoryTrust::default();
        for c in &self.capsules {
            trust.set(
                c.source.clone().unwrap(),
                c.id.clone(),
                c.revision.clone().unwrap(),
                TrustState::Reviewed,
            );
        }

        let mut layers = Vec::new();
        if !self.project.is_empty() {
            let refs: Vec<&str> = self.project.iter().map(String::as_str).collect();
            layers.push(layer(ScopeKind::Project, &refs));
        }
        if !self.session.is_empty() {
            let refs: Vec<&str> = self.session.iter().map(String::as_str).collect();
            layers.push(layer(ScopeKind::Session, &refs));
        }

        let request = ResolveRequest {
            context: descriptor(self.isolation),
            layers,
            policy: ManagedPolicy::default(),
        };
        let view = resolve(&catalog, &trust, &request).expect("fixture context should resolve");
        assert!(
            view.active_of_kind(Kind::Skill).len() + view.active_of_kind(Kind::Script).len() > 0,
            "a fixture context with nothing active proves nothing"
        );

        ResolvedContext {
            view,
            capsule_roots: self.roots,
            actor_bootstrap: self.actor_bootstrap,
        }
    }
}

impl Default for ContextBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Real payload trees
// ---------------------------------------------------------------------------

/// Write a valid native Agent Skill tree, with the progressive-disclosure
/// subdirectories, and return its root.
pub fn write_agent_skill(base: &Path, name: &str, description: &str) -> PathBuf {
    let root = base.join(name);
    std::fs::create_dir_all(root.join("scripts")).unwrap();
    std::fs::create_dir_all(root.join("references")).unwrap();
    std::fs::write(
        root.join("SKILL.md"),
        format!(
            "---\nname: {name}\ndescription: {description}\n---\n\n\
             # {name}\n\nAIKIT-PAYLOAD-BODY-MARKER: the full instructions live here.\n"
        ),
    )
    .unwrap();
    std::fs::write(root.join("scripts/check.sh"), "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::write(
        root.join("references/deep.md"),
        "AIKIT-PAYLOAD-BODY-MARKER\n",
    )
    .unwrap();
    root
}

/// Write a capsule's `payload/` directory as a valid Agent Skill.
pub fn write_payload_skill(capsule_root: &Path, skill_name: &str, description: &str) -> PathBuf {
    let payload = capsule_root.join("payload");
    std::fs::create_dir_all(payload.join("scripts")).unwrap();
    std::fs::create_dir_all(payload.join("references")).unwrap();
    std::fs::write(
        payload.join("SKILL.md"),
        format!(
            "---\nname: {skill_name}\ndescription: {description}\n---\n\n\
             # {skill_name}\n\nAIKIT-PAYLOAD-BODY-MARKER: the full instructions live here.\n"
        ),
    )
    .unwrap();
    std::fs::write(payload.join("scripts/check.sh"), "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::write(
        payload.join("references/deep.md"),
        "AIKIT-PAYLOAD-BODY-MARKER\n",
    )
    .unwrap();
    payload
}

/// Write a projection plan's items to disk.
pub fn materialize(items: &[aikit_core::projection::ProjectionItem], root: &Path) {
    use aikit_core::projection::ProjectionItem;

    for item in items {
        match item {
            ProjectionItem::Link { from, to } => {
                let target = root.join(to);
                std::fs::create_dir_all(target.parent().unwrap()).unwrap();
                std::os::unix::fs::symlink(from, &target).unwrap();
            }
            ProjectionItem::Copy { from, to } => {
                let target = root.join(to);
                std::fs::create_dir_all(target.parent().unwrap()).unwrap();
                std::fs::copy(from, &target).unwrap();
            }
            ProjectionItem::Write { path, contents } => {
                let target = root.join(path);
                std::fs::create_dir_all(target.parent().unwrap()).unwrap();
                std::fs::write(&target, contents).unwrap();
            }
            ProjectionItem::Shim { .. } | ProjectionItem::Env { .. } => {}
        }
    }
}
