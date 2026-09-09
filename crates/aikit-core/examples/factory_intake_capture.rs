//! Reproducible generator for Software-Factory's AIKit-side agent-capability
//! intake fixture.
//!
//! `EpiLogos/agent-system-design` (the Software Factory repository) commits an
//! evidence envelope at `contracts/factory/fixtures/intake/aikit-effective-envelope.json`
//! that its own conformance test (`factory/tests/agent_capability_intake.rs`)
//! asserts against. That envelope's `captured_from.captured_by` field says it
//! was produced "by executing the pinned revision in an isolated temp capture
//! crate" — this example *is* that capture crate, given a permanent home
//! instead of living as a loose, ungoverned file outside any repository.
//!
//! Factory issue #188 and PR #213 ("Factory #188: repin composed profile
//! intake evidence") are the request and the landing of the evidence this
//! reproduces. Factory's own `factory/README.md` draws the boundary this
//! example respects: "AIKit remains a separate control-plane product. Factory
//! may consume its public seams; this crate does not absorb AIKit internals."
//! So the capture lives here, beside the seams it exercises, and Factory only
//! ever consumes the resulting JSON.
//!
//! It builds a small in-memory catalog and trust store by hand — deliberately
//! *not* through a live AIKit home — so the scenario is exact and portable:
//!
//! - `skill/factory/return-review` — active, reviewed: this is the one member
//!   that projects.
//! - `skill/factory/legacy-orientation` — retired on its Control ground:
//!   withheld with reason `retired-standing`.
//! - `skill/factory/unreviewed-return` — active but left at `TrustState::Unseen`:
//!   withheld with reason `trust-required`.
//! - `skill/factory/unresolved-member` — referenced only as a `SkillSet`
//!   member, never inserted into the catalog: withheld with reason
//!   `not-in-catalog`.
//!
//! To regenerate the committed fixture at a newly pinned AIKit revision,
//! check out that revision (a detached worktree is enough — no live AIKit
//! home is read or written) and run:
//!
//! ```sh
//! cargo run --example factory_intake_capture -p aikit-core
//! ```
//!
//! The printed JSON is the fixture's `envelope` object verbatim; the
//! surrounding `schema`/`side`/`captured_from` wrapper and the sibling
//! `central-authored-envelope.json` / `conformance-pins.json` files are
//! assembled by the Factory-side intake process, not by this example.

use aikit_core::capsule::Capsule;
use aikit_core::catalog::{Catalog, MemoryCatalog};
use aikit_core::context::{ContextDescriptor, Isolation};
use aikit_core::id::{CapsuleId, ContextId, GenerationId, RegistrySource, Revision};
use aikit_core::platform::{Platform, TargetId};
use aikit_core::policy::ManagedPolicy;
use aikit_core::profile::PoolPatch;
use aikit_core::resolve::{resolve, ResolveRequest};
use aikit_core::scope::{LayerOrigin, ScopeKind, ScopeLayer};
use aikit_core::skillset::{self, SetMembership, SetProvenance, SkillSet};
use aikit_core::trust::{MemoryTrust, TrustState};
use serde_json::{json, Value};

fn id(raw: &str) -> CapsuleId {
    CapsuleId::parse(raw).unwrap()
}

fn skill(raw_id: &str, control: &str) -> Capsule {
    let name = raw_id.rsplit('/').next().unwrap();
    let source = format!(
        "schema = 1\nid = \"{raw_id}\"\nkind = \"skill\"\nname = \"{name}\"\ndescription = \"Factory intake {name}.\"\n{control}\n[skill]\nroot = \"payload\"\n"
    );
    let mut capsule = Capsule::from_toml_str(&source).unwrap();
    capsule.revision = Some(Revision::from_hash(blake3::hash(source.as_bytes())));
    capsule.source = Some(RegistrySource::personal());
    capsule
}

fn main() {
    let retirement = r#"
[metadata.control]
standing = "retired"
scope = "control-machine"
provenance = "human-authored"
retired-by = "owner"
retired-at-unix-seconds = 1788653215
retirement-reason = "Superseded by the composed AgentProfile orientation source; retired by owner."
"#;
    let active = r#"
[metadata.control]
standing = "active"
scope = "control-machine"
provenance = "human-authored"
"#;
    let mut catalog = MemoryCatalog::default();
    for capsule in [
        skill("skill/factory/return-review", active),
        skill("skill/factory/legacy-orientation", retirement),
        skill("skill/factory/unreviewed-return", active),
    ] {
        catalog.insert(capsule);
    }
    let mut trust = MemoryTrust::default();
    for capsule in catalog.capsules() {
        trust.set(
            capsule.source.clone().unwrap(),
            capsule.id.clone(),
            capsule.revision.clone().unwrap(),
            if capsule.id == id("skill/factory/unreviewed-return") {
                TrustState::Unseen
            } else {
                TrustState::Reviewed
            },
        );
    }
    let request = ResolveRequest {
        context: ContextDescriptor {
            context_id: ContextId::parse("ctx_FACTORYINTAKE00000000A").unwrap(),
            session_id: None,
            project_id: None,
            project_root: Some("/isolated/factory-intake".into()),
            task: None,
            isolation: Isolation::Shared,
            platform: Platform::Linux,
            targets: vec![TargetId::shell(), TargetId::codex()],
            mux: None,
            host: "isolated-capture".into(),
        },
        layers: vec![ScopeLayer {
            kind: ScopeKind::Project,
            depth: 0,
            origin: LayerOrigin::new("factory-intake-capture"),
            patch: PoolPatch {
                profiles: vec![],
                uses: vec![],
                enable: vec![
                    id("skill/factory/return-review"),
                    id("skill/factory/legacy-orientation"),
                    id("skill/factory/unreviewed-return"),
                ],
                disable: vec![],
                config: Default::default(),
                skill_overlays: Default::default(),
            },
        }],
        policy: ManagedPolicy::default(),
    };
    let view = resolve(&catalog, &trust, &request).unwrap();
    let mut set = SkillSet::new("factory-intake", SetProvenance::Composed);
    for member in [
        "skill/factory/return-review",
        "skill/factory/legacy-orientation",
        "skill/factory/unresolved-member",
        "skill/factory/unreviewed-return",
    ] {
        set = set.with_member(id(member), SetMembership::Explicit);
    }
    let projection = skillset::project(&set, &view);
    let view_bytes = serde_json::to_vec(&view).unwrap();
    let generation = GenerationId::from_hash(blake3::hash(&view_bytes));
    let withheld: Vec<Value> = projection
        .withheld
        .iter()
        .map(|member| {
            json!({
                "capability": member.capsule.to_string(),
                "reason": member.describe(),
                "withheld_reason": serde_json::to_value(&member.reason).unwrap(),
            })
        })
        .collect();
    let envelope = json!({
        "complete": projection.is_complete(),
        "generation": {
            "context_id": request.context.context_id.to_string(),
            "generation_id": generation.to_string(),
        },
        "members": set.len(),
        "projected": projection.projected.iter().map(ToString::to_string).collect::<Vec<_>>(),
        "set": {
            "label": set.label(),
            "name": set.name,
            "provenance": set.provenance.as_str(),
        },
        "summary": projection.summarize("sets/factory-intake"),
        "surface": "aikit_core resolve + skillset::project and GenerationId::from_hash over the serialized resolved view",
        "withheld": withheld,
    });
    println!("{}", serde_json::to_string_pretty(&envelope).unwrap());
}
