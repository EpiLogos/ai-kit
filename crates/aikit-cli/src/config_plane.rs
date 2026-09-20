//! The owner side of the O:I configuration plane (#299 C0,
//! `09-CONFIGURATION-PLANE.md`): `aikit config-contribution` and the four
//! owner-native verbs `aikit config validate|plan|apply|reset`.
//!
//! Where `system.rs` discloses what is true, this module contracts what the
//! composed World may *address*: setting identity (`ai-kit:<section>:<key>`,
//! exactly the v2 disclosure's section/key pairing), value schema, allowed
//! scopes, expected effect, and the owner-native operation behind each verb.
//! The two planes are never mixed: a contribution carries no
//! declared/effective/active values, and nothing here writes into a v2
//! document.
//!
//! The AIKit-specific laws this module is the Gate-B specimen for:
//!
//! * **Native profiles stay native.** A setting value for profile selection is
//!   the owner's own profile id (`profile/<group>/<name>`), passed opaquely and
//!   resolved natively. No profile internals are copied into any document.
//! * **Mutations go through AIKit's own services** (`use_profile`, the toggle
//!   apply pipeline, `project defaults`), so `aikit system --json` keeps
//!   reporting truthful effective/active axes afterwards.
//! * **Secrets are presence and reference only.** The credential setting
//!   refuses material values with a structured error; the material itself is
//!   bound only through AIKit's native `aikit credential setup`.
//! * **Idempotency is owner-side.** `(owner_ref, changeset_id, setting_ref,
//!   scope, plan_digest)` is checked against AIKit's own receipt history
//!   (`state/config/receipts.jsonl`); a replay returns `no_op` naming the
//!   original receipt and never re-executes.
//!
//! Harness trust/permissions settings ride the same contract as the general
//! pattern: each harness's embedded profile (`aikit.harness-profile/v1`,
//! `settings.trust-settings`) declares what its own config supports, and
//! `harness_sections` below derives the per-harness sections from those
//! declarations — disclosure-only, `ai-kit:<slug>:<key>`.
//!
//! One additive extension beyond the frozen `oi.config-plan/v1` properties:
//! plans carry the requested `value` as a top-level field, so that the
//! `plan_digest` (computed over the plan body) genuinely pins the change it
//! anchors. Consumers must accept unknown fields (C0 §15), and without the
//! value in the body an `apply --plan-file` could not know what to apply.

use std::path::Path;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use ulid::Ulid;

use aikit_core::catalog::Catalog;
use aikit_core::harness_profile::{TrustSettingScope, TrustValueKind};
use aikit_core::id::{CapsuleId, ProfileId};
use aikit_core::scope::ScopeKind;
use aikit_core::AikitError;
use aikit_store::config_plane::{ConfigReceiptStore, ExecutedKey};

use crate::app::{AikitApplication, ApplyRequest, Service};
use crate::cli::ConfigCmd;
use crate::json;
use aikit_tui::backend::Toggle;
pub const CONTRIBUTION_SCHEMA: &str = "oi.configuration-contribution/v1";
pub const VALIDATION_SCHEMA: &str = "oi.config-validation/v1";
pub const PLAN_SCHEMA: &str = "oi.config-plan/v1";
pub const RECEIPT_SCHEMA: &str = "oi.config-receipt/v1";
pub const ERROR_SCHEMA: &str = "oi.config-error/v1";
pub const CONTRACT_REVISION: &str = "configuration-plane/contribution.1";
pub const OWNER_REF: &str = "ai-kit";
pub const CONTRIBUTION_COMMAND: [&str; 3] = ["aikit", "config-contribution", "--json"];
pub const DIGEST_COVERS: &str = "07 §4.5 convention";

// ---------------------------------------------------------------------------
// The contributed settings
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Setting {
    /// Full `owner:section:key` identity.
    setting_ref: &'static str,
    section_ref: &'static str,
    /// The v2 disclosure key this setting maps onto (C0 §17). `skill-sets.default`
    /// has no v2 axis: reconciliation for it is `unknown`, which is the honest
    /// reading until the disclosure plane grows the key.
    key: &'static str,
    title: &'static str,
    description: &'static str,
    value_schema: Value,
    allowed_scopes: &'static [&'static str],
    writable: bool,
    profileable: bool,
    sensitive: bool,
    default: Option<Value>,
    default_semantics: &'static str,
    effect_kind: &'static str,
    effect_summary: &'static str,
    effect_ref: Option<&'static str>,
    /// validate / plan / apply / reset
    operations: (bool, bool, bool, bool),
    native_ref: &'static str,
}

impl Setting {
    fn value_kind(&self) -> &str {
        self.value_schema["type"].as_str().unwrap_or_default()
    }

    fn spec(&self) -> Value {
        let mut spec = json!({
            "setting_ref": self.setting_ref,
            "section_ref": self.section_ref,
            "title": self.title,
            "description": self.description,
            "value_schema": self.value_schema,
            "allowed_scopes": self.allowed_scopes
                .iter()
                .map(|kind| json!({ "scope_kind": kind, "scope_ref": null }))
                .collect::<Vec<_>>(),
            "writable": self.writable,
            "profileable": self.profileable,
            "sensitive": self.sensitive,
            "default_semantics": self.default_semantics,
            "effect": {
                "kind": self.effect_kind,
                "summary": self.effect_summary,
                "ref": self.effect_ref,
            },
            "operations": {
                "validate": self.operations.0,
                "plan": self.operations.1,
                "apply": self.operations.2,
                "reset": self.operations.3,
            },
            "native_ref": self.native_ref,
        });
        if let Some(default) = &self.default {
            spec["default"] = default.clone();
        }
        spec
    }
}

fn settings() -> Vec<Setting> {
    vec![
        Setting {
            setting_ref: "ai-kit:resolution:resolution.profiles",
            section_ref: "resolution",
            key: "resolution.profiles",
            title: "Native profile selection",
            description: "Which native AIKit profile the scope declares (the resolution basis). \
                      Values are AIKit's own profile ids, passed by reference and resolved \
                      natively; profile internals are never copied into the plane.",
            value_schema: json!({ "type": "reference", "subject_kind": "aikit.profile-id" }),
            allowed_scopes: &["machine", "project", "agent-session"],
            writable: true,
            profileable: true,
            sensitive: false,
            default: None,
            default_semantics: "computed",
            effect_kind: "session-restart-required",
            effect_summary: "New sessions compose with the new profile; harness sessions already \
                         running keep their composition until they are relaunched.",
            effect_ref: Some("aikit diff --json"),
            operations: (true, true, true, true),
            native_ref: "aikit:scope:profiles",
        },
        Setting {
            setting_ref: "ai-kit:skills:skills.capabilities",
            section_ref: "skills",
            key: "skills.capabilities",
            title: "Capability toggles",
            description: "The scope's declared capability enable/disable deltas, as an object \
                      mapping capability id to boolean. Applied through AIKit's native \
                      toggle pipeline, which re-materialises the scope's generation.",
            value_schema: json!({
                "type": "table",
                "columns": [
                    { "name": "capability", "type": "scalar" },
                    { "name": "enabled", "type": "boolean" }
                ]
            }),
            allowed_scopes: &["machine", "project", "agent-session"],
            writable: true,
            profileable: false,
            sensitive: false,
            default: None,
            default_semantics: "computed",
            effect_kind: "session-restart-required",
            effect_summary: "The next compose resolves the new capability horizon; sessions \
                         already running keep the horizon they were launched with.",
            effect_ref: Some("aikit diff --json"),
            operations: (true, true, true, true),
            native_ref: "aikit:scope:toggles",
        },
        Setting {
            setting_ref: "ai-kit:resolution:skill-sets.default",
            section_ref: "resolution",
            key: "skill-sets.default",
            title: "Default skill-sets",
            description: "The skill-sets an unconfigured project inherits by default (AIKit home \
                      `config.toml`). No v2 disclosure axis exists for this setting yet, so \
                      reconciliation reads `unknown` while mutation still works.",
            value_schema: json!({ "type": "list", "items": { "type": "scalar" } }),
            allowed_scopes: &["machine"],
            writable: true,
            profileable: false,
            sensitive: false,
            default: Some(json!([])),
            default_semantics: "constant",
            effect_kind: "value-change",
            effect_summary: "The next project match inherits the new default skill-sets; \
                         already-resolved contexts keep theirs.",
            effect_ref: Some("aikit project defaults --json"),
            operations: (true, true, true, true),
            native_ref: "aikit:config:default-skill-sets",
        },
        Setting {
            setting_ref: "ai-kit:models:models.candidates",
            section_ref: "models",
            key: "models.candidates",
            title: "Model candidates",
            description: "The model candidates a launch composes from. AIKit authors no \
                      default-model setting: the model is resolved per session launch \
                      (`aikit compose --model`), so this subject is disclosure-only — there \
                      is nothing native for the plane to write.",
            value_schema: json!({ "type": "reference", "subject_kind": "model.stable-id" }),
            allowed_scopes: &["world"],
            writable: false,
            profileable: false,
            sensitive: false,
            default: None,
            default_semantics: "computed",
            effect_kind: "none",
            effect_summary: "Model choice resolves per launch; there is no restart axis.",
            effect_ref: Some("aikit compose --json"),
            operations: (false, false, false, false),
            native_ref: "aikit:model-catalogue",
        },
        Setting {
            setting_ref: "ai-kit:models:models.credentials",
            section_ref: "models",
            key: "models.credentials",
            title: "Provider credential reference",
            description: "Presence and reference for provider credentials — classic LLMs and \
                      voice models alike, one inventory per provider. The material is bound \
                      owner-natively (`aikit credential setup`, optionally declaring an \
                      external store location with `--ref op://…`); the lifecycle is closed \
                      with `aikit credential rotate` and `aikit credential revoke`, and \
                      `aikit credential discover` surfaces candidate keys already on the \
                      machine, presence only. This plane never carries or mutates a value.",
            value_schema: json!({ "type": "secret" }),
            allowed_scopes: &["world"],
            writable: false,
            profileable: false,
            sensitive: true,
            default: None,
            default_semantics: "none",
            effect_kind: "provider-reconnect-required",
            effect_summary: "When a credential is re-bound owner-natively, providers reconnect \
                         with it.",
            effect_ref: Some("aikit credential setup --json"),
            operations: (true, false, false, false),
            native_ref: "aikit:credentials",
        },
    ]
}

fn find_setting(setting_ref: &str) -> Option<Setting> {
    settings()
        .into_iter()
        .find(|s| s.setting_ref == setting_ref)
}

fn expected_effect(setting: &Setting) -> Value {
    json!({
        "kind": setting.effect_kind,
        "summary": setting.effect_summary,
        "ref": setting.effect_ref,
    })
}

// ---------------------------------------------------------------------------
// Failures: oi.config-error/v1 on stdout, non-zero exit
// ---------------------------------------------------------------------------

/// A refused request: the structured error document plus the exit status.
pub struct Failure {
    pub doc: Value,
    pub exit: i32,
}

fn error_doc(code: &str, message: String) -> Value {
    json!({
        "schema": ERROR_SCHEMA,
        "error_code": code,
        "message": message,
        "setting_ref": null,
        "scope_kind": null,
        "retryable": false,
        "detail_ref": null,
    })
}

fn fail(code: &str, message: impl std::fmt::Display) -> Failure {
    Failure {
        doc: error_doc(code, message.to_string()),
        // Refusals the caller caused are usage; only the owner being broken is
        // a generic runtime failure.
        exit: if matches!(code, "owner_unavailable" | "internal") {
            json::EXIT_GENERIC
        } else {
            json::EXIT_USAGE
        },
    }
}

fn fail_with(
    mut doc: Value,
    code: &str,
    message: impl std::fmt::Display,
    setting: &str,
) -> Failure {
    doc["error_code"] = json!(code);
    doc["message"] = json!(message.to_string());
    doc["setting_ref"] = json!(setting);
    Failure {
        doc,
        exit: json::EXIT_USAGE,
    }
}

// ---------------------------------------------------------------------------
// Scope addressing
// ---------------------------------------------------------------------------

/// One resolved scope address. The frozen seed kinds that AIKit can mean.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopeAddress {
    World,
    Machine,
    Project(String),
    AgentSession(String),
}

impl ScopeAddress {
    fn kind(&self) -> &'static str {
        match self {
            ScopeAddress::World => "world",
            ScopeAddress::Machine => "machine",
            ScopeAddress::Project(_) => "project",
            ScopeAddress::AgentSession(_) => "agent-session",
        }
    }

    fn scope_ref(&self) -> Option<&str> {
        match self {
            ScopeAddress::Project(reference) | ScopeAddress::AgentSession(reference) => {
                Some(reference)
            }
            _ => None,
        }
    }

    fn wire(&self) -> Value {
        json!({
            "scope_kind": self.kind(),
            "scope_ref": self.scope_ref().map(str::to_string),
        })
    }

    fn compact(&self) -> String {
        match self.scope_ref() {
            Some(reference) => format!("{}:{reference}", self.kind()),
            None => self.kind().to_string(),
        }
    }

    /// The AIKit scope this address writes through, when it maps to one.
    fn aikit_kind(&self) -> Option<ScopeKind> {
        match self {
            ScopeAddress::Machine => Some(ScopeKind::Global),
            ScopeAddress::Project(_) => Some(ScopeKind::Project),
            ScopeAddress::AgentSession(_) => Some(ScopeKind::Session),
            ScopeAddress::World => None,
        }
    }

    /// Parse a compact `kind[:ref]` address against the frozen seed registry.
    fn parse(raw: &str) -> Result<Self, Failure> {
        let (kind, reference) = match raw.split_once(':') {
            Some((kind, reference)) => (kind, Some(reference)),
            None => (raw, None),
        };
        match kind {
            "world" | "machine" if reference.is_some() => Err(fail(
                "unsupported_scope",
                format!("`{kind}` is a singular scope kind: it takes no scope_ref"),
            )),
            "world" => Ok(ScopeAddress::World),
            "machine" => Ok(ScopeAddress::Machine),
            "project" | "agent-session" => {
                let reference = reference
                    .map(str::trim)
                    .filter(|r| !r.is_empty())
                    .ok_or_else(|| {
                        fail(
                            "unsupported_scope",
                            format!("`{kind}` addresses one instance: pass {kind}:<ref>"),
                        )
                    })?;
                if kind == "project" {
                    Ok(ScopeAddress::Project(reference.to_string()))
                } else {
                    Ok(ScopeAddress::AgentSession(reference.to_string()))
                }
            }
            other
                if matches!(
                    other,
                    "ground" | "workcell" | "agency" | "agent" | "provider" | "connector-relation"
                ) =>
            {
                Err(fail(
                    "unsupported_scope",
                    format!("no AIKit setting is addressable at `{other}` scope"),
                ))
            }
            other => Err(fail(
                "unknown_scope_kind",
                format!("`{other}` is not in the frozen scope registry"),
            )),
        }
    }

    /// Resolve the address against this context: a project must be the bound
    /// project here, a session must be the ambient session — the plane
    /// addresses *this* world's scopes, never a remote one.
    fn resolve_against(&self, service: &Service, cwd: &Path) -> Result<(), Failure> {
        match self {
            ScopeAddress::Project(reference) => {
                let matched = crate::projects::resolve(service.home(), cwd)
                    .map_err(|error| fail("owner_unavailable", error.message()))?;
                let bound = matched.as_ref().map(|m| m.spec.id.clone()).ok_or_else(|| {
                    fail(
                        "unsupported_scope",
                        "no project is bound for this directory: the config plane addresses \
                             projects by their bound identity (`aikit project bind <id> <dir>`)",
                    )
                })?;
                if bound != *reference {
                    return Err(fail(
                        "unsupported_scope",
                        format!(
                            "the bound project here is `{bound}`, not `{reference}`: never \
                             reinterpreted at another scope"
                        ),
                    ));
                }
                Ok(())
            }
            ScopeAddress::AgentSession(reference) => {
                let ambient = service.descriptor().session_id.as_ref();
                let matches = ambient
                    .map(|session| session.as_str() == reference.as_str())
                    .unwrap_or(false);
                if !matches {
                    return Err(fail(
                        "unsupported_scope",
                        "AIKit writes session declarations only for the ambient AIKit session \
                         (AIKIT_SESSION_ID); no other session's overlay is writable from here",
                    ));
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

/// The address to use when `--scope` is omitted: the context's own default
/// mutation scope, mapped back into the frozen registry — or an explicit
/// refusal when this context has no addressable default.
fn default_address(service: &Service) -> Result<ScopeAddress, Failure> {
    let descriptor = service.descriptor();
    if let Some(session) = &descriptor.session_id {
        return Ok(ScopeAddress::AgentSession(session.as_str().to_string()));
    }
    match &descriptor.project_root {
        Some(root) => {
            let matched = crate::projects::resolve(service.home(), root)
                .map_err(|error| fail("owner_unavailable", error.message()))?;
            match matched {
                Some(matched) => Ok(ScopeAddress::Project(matched.spec.id)),
                None => Err(fail(
                    "unsupported_scope",
                    "this directory's project is not bound, so it has no addressable scope; \
                     pass --scope (for example --scope machine) or bind the project",
                )),
            }
        }
        None => Ok(ScopeAddress::Machine),
    }
}

// ---------------------------------------------------------------------------
// Contribution
// ---------------------------------------------------------------------------

fn finalize_digest(body: &mut Value) -> Result<(), AikitError> {
    let mut canonical = body.clone();
    canonical["owner"]["disclosed_at_unix_ms"] = json!(0);
    canonical["owner"]["reading_digest"] = Value::Null;
    let bytes = serde_json::to_vec(&canonical).map_err(|error| {
        AikitError::new(
            "config.digest_encode_failed",
            format!("could not encode contribution for digest: {error}"),
        )
    })?;
    body["owner"]["reading_digest"] = json!(format!("{:x}", Sha256::digest(&bytes)));
    Ok(())
}

fn sections() -> Vec<Value> {
    let mut sections: Vec<Value> = Vec::new();
    for setting in settings() {
        let title = match setting.section_ref {
            "resolution" => "Project / Profile / scope",
            "skills" => "Skills / SkillSets / Methods / UsageOverlays",
            "models" => "Models / providers / credential refs",
            other => unreachable!("unmapped section {other}"),
        };
        let entry = sections.iter_mut().find(|s| s["id"] == setting.section_ref);
        match entry {
            Some(section) => {
                if let Some(settings) = section["settings"].as_array_mut() {
                    settings.push(setting.spec());
                }
            }
            None => sections.push(json!({
                "id": setting.section_ref,
                "title": title,
                "settings": [setting.spec()],
            })),
        }
    }
    sections
}

/// The harness trust/permissions sections, derived from the embedded
/// harness profiles (`aikit.harness-profile/v1`): every profile whose
/// `settings.trust-settings` declares entries yields one section keyed by
/// the harness slug, and every declaration becomes a disclosure-only plane
/// setting `ai-kit:<slug>:<key>`. This is the general pattern the owner
/// commissioned: a harness's trust surface is declared in the harness's own
/// profile and the plane surfaces it — the next harness plugs in by
/// declaring its settings, with no new code here.
///
/// Declarations are disclosed `writable: false` with no plan/apply/reset:
/// the harness owns the native write, AIKit reads and desires. That is the
/// `models.candidates` precedent (disclosure-only), applied to harness
/// trust.
fn harness_sections() -> Vec<Value> {
    let scope_wire = |scope: TrustSettingScope| match scope {
        TrustSettingScope::Machine => "machine",
        TrustSettingScope::Project => "project",
    };
    let mut sections: Vec<Value> = Vec::new();
    for (slug, profile) in aikit_adapters::profiles::all() {
        let Some(settings) = &profile.settings else {
            continue;
        };
        if settings.trust_settings.is_empty() {
            continue;
        }
        let disclosed: Vec<Value> = settings
            .trust_settings
            .iter()
            .map(|declaration| {
                let value_schema = match declaration.value_schema.kind {
                    TrustValueKind::Enum => json!({
                        "type": "enum",
                        "options": declaration
                            .value_schema
                            .options
                            .iter()
                            .map(|option| json!({ "value": option }))
                            .collect::<Vec<_>>(),
                    }),
                    TrustValueKind::Boolean => json!({ "type": "boolean" }),
                    TrustValueKind::Scalar => json!({ "type": "scalar" }),
                };
                json!({
                    "setting_ref": format!("{OWNER_REF}:{slug}:{}", declaration.key),
                    "section_ref": slug,
                    "title": declaration.title,
                    "description": declaration.description,
                    "value_schema": value_schema,
                    "allowed_scopes": declaration
                        .scopes
                        .iter()
                        .map(|scope| json!({
                            "scope_kind": scope_wire(*scope),
                            "scope_ref": null,
                        }))
                        .collect::<Vec<_>>(),
                    "writable": false,
                    "profileable": true,
                    "sensitive": false,
                    "default_semantics": "none",
                    "effect": {
                        "kind": "none",
                        "summary": "The harness owns this entry natively; AIKit reads and \
                                    desires it, and writes no trust change through this plane.",
                        "ref": declaration.config,
                    },
                    "operations": {
                        "validate": true,
                        "plan": false,
                        "apply": false,
                        "reset": false,
                    },
                    "native_ref": declaration.config,
                })
            })
            .collect();
        sections.push(json!({
            "id": slug,
            "title": format!("{slug} — the harness's own trust/permissions surface"),
            "settings": disclosed,
        }));
    }
    sections
}

/// Every section the contribution discloses: the owner's own static
/// settings plus the derived per-harness trust sections.
fn all_sections() -> Vec<Value> {
    let mut all = sections();
    all.extend(harness_sections());
    all
}

/// The full contribution for a resolvable context. Availability is probed, not
/// asserted: the document degrades honestly when the owner context cannot be
/// resolved rather than failing the read (the Wave-5 convention).
pub fn contribution_document(cwd: &Path) -> Value {
    match Service::discover(cwd) {
        Ok(_) => {
            let mut body = json!({
                "schema": CONTRIBUTION_SCHEMA,
                "contract_revision": CONTRACT_REVISION,
                "owner": {
                    "owner_ref": OWNER_REF,
                    "owner_kind": "product",
                    "owner_version": env!("CARGO_PKG_VERSION"),
                    "contribution_command": CONTRIBUTION_COMMAND,
                    "disclosed_at_unix_ms": now_ms(),
                    "reading_digest": null,
                    "reading_digest_covers": DIGEST_COVERS,
                },
                "about": "Resolution and composition: which native profiles, capability \
                          toggles, default skill-sets and credential references the composed \
                          World may address, plus each declared harness's own \
                          trust/permissions surface. Models are resolved per launch, \
                          credentials are bound owner-natively, and harness trust is \
                          harness-owned, so all three are disclosed without a write path \
                          through this plane.",
                "sections": all_sections(),
                "operations": {
                    "transport": "cli/v1",
                    "validate": { "availability": "disclosed", "reason": null },
                    "plan": { "availability": "disclosed", "reason": null },
                    "apply": { "availability": "disclosed", "reason": null },
                    "reset": { "availability": "disclosed", "reason": null },
                },
                "availability": { "state": "available", "reason": null },
                "degradations": [],
                "obligations": [
                    "SessionSpace authoring and model selection stay native (`aikit session`, \
                     `aikit compose --model`); they are relational or per-launch choices, not \
                     addressable settings.",
                    "Credential material is bound only through `aikit credential setup` (or \
                     declared with `--ref`); the plane discloses presence and reference, and \
                     the per-provider inventory rides `aikit system --json` \
                     (`ai-kit:credential:inventory`).",
                ],
            });
            finalize_digest(&mut body).unwrap_or(());
            body
        }
        Err(error) => {
            let reason = "the AIKit owner context could not be resolved on this machine";
            json!({
                "schema": CONTRIBUTION_SCHEMA,
                "contract_revision": CONTRACT_REVISION,
                "owner": {
                    "owner_ref": OWNER_REF,
                    "owner_kind": "product",
                    "owner_version": env!("CARGO_PKG_VERSION"),
                    "contribution_command": CONTRIBUTION_COMMAND,
                    "disclosed_at_unix_ms": now_ms(),
                    "reading_digest": null,
                    "reading_digest_covers": DIGEST_COVERS,
                },
                "about": "The owner could not resolve its context; absence is data and nothing \
                          is fabricated to fill the registry.",
                "sections": [],
                "operations": {
                    "transport": "cli/v1",
                    "validate": { "availability": "unavailable", "reason": reason },
                    "plan": { "availability": "unavailable", "reason": reason },
                    "apply": { "availability": "unavailable", "reason": reason },
                    "reset": { "availability": "unavailable", "reason": reason },
                },
                "availability": { "state": "unavailable", "reason": reason },
                "degradations": [{
                    "subject_ref": null,
                    "state": "unavailable",
                    "reason": reason,
                    "native_error": error.message(),
                }],
                "obligations": [],
            })
        }
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Value validation (owner-native; the frozen value_schema kinds are hints)
// ---------------------------------------------------------------------------

/// Check one requested value against one setting's native validation. Returns
/// the violations list; empty means valid.
fn validate_value(service: &Service, setting: &Setting, value: &Value) -> Vec<Value> {
    let mut violations: Vec<Value> = Vec::new();
    let violation =
        |code: &str, message: String| json!({ "code": code, "message": message, "path": null });
    match setting.value_kind() {
        "secret" => {
            // Representation law: the only acceptable value shape is a
            // secret_reference object. Anything else — above all a plain
            // string, which is what credential material looks like — is
            // refused, and the message never echoes the material.
            let ok = value.as_object().is_some_and(|object| {
                object.len() == 1
                    && object
                        .get("secret_reference")
                        .and_then(|r| r.as_object())
                        .is_some_and(|reference| {
                            reference.contains_key("ref") && reference.contains_key("present")
                        })
            });
            if !ok {
                violations.push(violation(
                    "secret_material_forbidden",
                    "secret-kind settings carry only a secret_reference \
                     ({\"secret_reference\":{\"ref\":...,\"present\":...}}); credential \
                     material never crosses the configuration plane"
                        .to_string(),
                ));
            }
        }
        "reference" => {
            let raw = value.as_str().unwrap_or_default();
            match ProfileId::parse(raw) {
                Ok(profile) => {
                    if Catalog::profile(service.snapshot(), &profile).is_none() {
                        violations.push(violation(
                            "unknown_profile",
                            format!("{profile} is not in any registry"),
                        ));
                    }
                }
                Err(error) => violations.push(violation(
                    "invalid_profile_ref",
                    format!(
                        "profile ids look like `profile/<group>/<name>`: {}",
                        error.message()
                    ),
                )),
            }
        }
        "table" => {
            let Some(map) = value.as_object() else {
                violations.push(violation(
                    "invalid_table",
                    "capability toggles are an object mapping capability id to boolean".to_string(),
                ));
                return violations;
            };
            if map.is_empty() {
                violations.push(violation(
                    "invalid_table",
                    "the toggle map is empty: name the capabilities to enable or disable"
                        .to_string(),
                ));
            }
            for (raw_id, enabled) in map {
                let parsed = CapsuleId::parse(raw_id)
                    .map_err(|error| violation("invalid_capability", error.message().to_string()));
                let id = match parsed {
                    Ok(id) => id,
                    Err(item) => {
                        violations.push(item);
                        continue;
                    }
                };
                if Catalog::get(service.snapshot(), &id).is_none() {
                    violations.push(violation(
                        "unknown_capability",
                        format!("{id} is not in the catalogue"),
                    ));
                }
                if !enabled.is_boolean() {
                    violations.push(violation(
                        "invalid_table",
                        format!("{raw_id} must map to true or false"),
                    ));
                }
            }
        }
        "list" => {
            let Some(items) = value.as_array() else {
                violations.push(violation(
                    "invalid_list",
                    "default skill-sets are a JSON array of set names".to_string(),
                ));
                return violations;
            };
            for item in items {
                match item.as_str().map(str::trim) {
                    Some(name) if !name.is_empty() => {}
                    _ => violations.push(violation(
                        "invalid_list",
                        format!("set name must be a non-empty string, got {item}"),
                    )),
                }
            }
        }
        other => violations.push(violation(
            "internal",
            format!("setting kind {other} has no validator"),
        )),
    }
    violations
}

// ---------------------------------------------------------------------------
// The verbs
// ---------------------------------------------------------------------------

struct Request {
    setting: Setting,
    address: ScopeAddress,
    value: Value,
}

/// Shared preamble for validate/plan/apply: resolve the setting, the scope and
/// (for validate-shaped checks) the value's native validation.
fn resolve_request(
    service: &Service,
    cwd: &Path,
    setting_ref: &str,
    scope: Option<String>,
    value: Value,
) -> Result<Request, Failure> {
    let setting = find_setting(setting_ref).ok_or_else(|| {
        fail(
            "unsupported_setting",
            format!("`{setting_ref}` is not a setting this owner contributes"),
        )
    })?;
    let address = match scope {
        Some(raw) => ScopeAddress::parse(&raw)?,
        None => default_address(service)?,
    };
    if !setting.allowed_scopes.contains(&address.kind()) {
        return Err(fail_with(
            error_doc("unsupported_scope", String::new()),
            "unsupported_scope",
            format!(
                "`{}` is not addressable at `{}` scope (allowed: {})",
                setting.setting_ref,
                address.compact(),
                setting.allowed_scopes.join(", ")
            ),
            setting.setting_ref,
        ));
    }
    address.resolve_against(service, cwd)?;
    if setting.value_kind() == "secret" {
        // The refusal of material comes before every other verdict: the
        // representation law is absolute, whatever the setting's writability.
        let violations = validate_value(service, &setting, &value);
        if !violations.is_empty() {
            return Err(fail_with(
                error_doc("invalid_value", String::new()),
                "invalid_value",
                violations[0]["message"].as_str().unwrap_or_default(),
                setting.setting_ref,
            ));
        }
    }
    Ok(Request {
        setting,
        address,
        value,
    })
}

fn read_value(value: Option<String>, value_file: Option<String>) -> Result<Value, Failure> {
    let text = match (value, value_file) {
        (Some(raw), None) => raw,
        (None, Some(path)) => match path.as_str() {
            "-" => {
                let mut buffer = String::new();
                std::io::Read::read_to_string(&mut std::io::stdin(), &mut buffer).map_err(
                    |error| fail("invalid_value", format!("could not read stdin: {error}")),
                )?;
                buffer
            }
            real => std::fs::read_to_string(real).map_err(|error| {
                fail(
                    "invalid_value",
                    format!("could not read --value-file {real}: {error}"),
                )
            })?,
        },
        (Some(_), Some(_)) => unreachable!("clap rejects --value with --value-file"),
        (None, None) => {
            return Err(fail(
                "invalid_value",
                "validate and plan need a value: pass --value <json> or --value-file <path|->",
            ))
        }
    };
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(fail(
            "invalid_value",
            "the value is empty: pass --value <json> or --value-file <path|->",
        ));
    }
    serde_json::from_str(trimmed).map_err(|error| {
        fail(
            "invalid_value",
            format!("the value is not valid JSON: {error}"),
        )
    })
}

pub fn validate(
    service: &Service,
    cwd: &Path,
    args: crate::cli::ConfigValidateArgs,
) -> Result<Value, Failure> {
    let value = read_value(args.value, args.value_file)?;
    let request = resolve_request(service, cwd, &args.setting, args.scope, value)?;
    if !request.setting.operations.0 {
        return Err(not_writable(&request.setting));
    }
    let violations = validate_value(service, &request.setting, &request.value);
    let mut doc = json!({
        "schema": VALIDATION_SCHEMA,
        "setting_ref": request.setting.setting_ref,
        "scope": request.address.wire(),
        "valid": violations.is_empty(),
        "expected_effect": expected_effect(&request.setting),
    });
    if !violations.is_empty() {
        doc["violations"] = json!(violations);
    }
    Ok(doc)
}

fn not_writable(setting: &Setting) -> Failure {
    let hint = if setting.value_kind() == "secret" {
        "credential material is bound only through `aikit credential setup`"
    } else {
        "the subject is disclosed, not addressed"
    };
    // The operation gates use this refusal for every verb the setting does not
    // disclose, so the message names the plane's law rather than one verb.
    fail_with(
        error_doc("unsupported_setting", String::new()),
        "unsupported_setting",
        format!(
            "`{}` is disclosure-only (writable: false): {hint}",
            setting.setting_ref
        ),
        setting.setting_ref,
    )
}

/// The canonical plan body: plan_id and every `*_unix_ms` field zeroed (§6).
/// This is what `plan_digest` is taken over, so a digest genuinely pins the
/// change — including the requested value, which rides the body as the owner's
/// additive `value` field.
fn canonical_plan_body(plan: &Value) -> Value {
    let mut canonical = plan.clone();
    canonical["plan_id"] = json!("");
    canonical["expires_at_unix_ms"] = json!(0);
    // The digest cannot cover itself; it is null in the canonical body, exactly
    // as `reading_digest` is in the disclosure convention.
    canonical["plan_digest"] = Value::Null;
    canonical
}

fn plan_digest(plan: &Value) -> Result<String, Failure> {
    let bytes = serde_json::to_vec(&canonical_plan_body(plan)).map_err(|error| {
        fail(
            "internal",
            format!("could not encode plan for digest: {error}"),
        )
    })?;
    Ok(format!("{:x}", Sha256::digest(&bytes)))
}

fn change_summary(setting: &Setting, value: &Value, scope: &ScopeAddress) -> String {
    match setting.value_kind() {
        "reference" => format!(
            "declare profile `{}` at {}",
            value.as_str().unwrap_or_default(),
            scope.compact()
        ),
        "table" => {
            let mut parts: Vec<String> = Vec::new();
            if let Some(map) = value.as_object() {
                for (id, enabled) in map {
                    parts.push(format!(
                        "{id} {}",
                        if enabled.as_bool() == Some(true) {
                            "enabled"
                        } else {
                            "disabled"
                        }
                    ));
                }
                parts.sort();
            }
            format!(
                "capability toggles at {}: {}",
                scope.compact(),
                parts.join(", ")
            )
        }
        "list" => format!(
            "default skill-sets become {:?} at {}",
            value,
            scope.compact()
        ),
        _ => format!("update {} at {}", setting.key, scope.compact()),
    }
}

pub fn plan(
    service: &Service,
    cwd: &Path,
    args: crate::cli::ConfigPlanArgs,
) -> Result<Value, Failure> {
    let value = read_value(args.value, args.value_file)?;
    let request = resolve_request(service, cwd, &args.setting, args.scope, value)?;
    if !request.setting.operations.1 {
        return Err(not_writable(&request.setting));
    }
    let violations = validate_value(service, &request.setting, &request.value);
    if !violations.is_empty() {
        let messages: Vec<String> = violations
            .iter()
            .filter_map(|v| v["message"].as_str().map(str::to_string))
            .collect();
        return Err(fail_with(
            error_doc("validation_failed", String::new()),
            "validation_failed",
            format!("the owner refuses this value: {}", messages.join("; ")),
            request.setting.setting_ref,
        ));
    }

    let plan_id = format!("plan-{}", Ulid::generate());
    let mut body = json!({
        "schema": PLAN_SCHEMA,
        "plan_id": plan_id,
        "plan_digest": null,
        "setting_ref": request.setting.setting_ref,
        "scope": request.address.wire(),
        "value": request.value,
        "changes": [{
            "summary": change_summary(&request.setting, &request.value, &request.address),
            "native_ref": request.setting.native_ref,
            "before_ref": null,
            "after_ref": null,
        }],
        "expected_effect": expected_effect(&request.setting),
        "expires_at_unix_ms": null,
        "explain_ref": request.setting.effect_ref,
        "authority": { "requires": [], "granted_by": "operator:aikit", "evidence_ref": null },
    });
    let digest = plan_digest(&body)?;
    body["plan_digest"] = json!(digest);
    Ok(body)
}

#[allow(clippy::too_many_arguments)]
fn receipt(
    setting: &Setting,
    scope: &ScopeAddress,
    changeset_id: &str,
    plan_digest: Option<String>,
    operation: &str,
    outcome: &str,
    native_ref: String,
    original_receipt_id: Option<String>,
) -> Value {
    json!({
        "schema": RECEIPT_SCHEMA,
        "receipt_id": format!("rcpt-{}", Ulid::generate()),
        "owner_ref": OWNER_REF,
        "changeset_id": changeset_id,
        "plan_digest": plan_digest,
        "setting_ref": setting.setting_ref,
        "scope": scope.wire(),
        "operation": operation,
        "outcome": outcome,
        "applied_at_unix_ms": now_ms(),
        "native_ref": native_ref,
        "expected_effect": expected_effect(setting),
        "original_receipt_id": original_receipt_id,
        "error": null,
    })
}

fn executed_key(
    changeset_id: &str,
    setting: &Setting,
    scope: &ScopeAddress,
    digest: Option<String>,
) -> ExecutedKey {
    ExecutedKey {
        owner_ref: OWNER_REF.to_string(),
        changeset_id: changeset_id.to_string(),
        setting_ref: setting.setting_ref.to_string(),
        scope_kind: scope.kind().to_string(),
        scope_ref: scope.scope_ref().map(str::to_string),
        plan_digest: digest,
    }
}

/// Execute one validated change through AIKit's own services. The value has
/// already passed native validation; the services still own the write.
fn execute(
    service: &mut Service,
    setting: &Setting,
    address: &ScopeAddress,
    value: &Value,
) -> Result<(), Failure> {
    let scope_kind = address
        .aikit_kind()
        .expect("writable settings always map to an AIKit scope");
    let result = match setting.value_kind() {
        "reference" => {
            let profile = ProfileId::parse(value.as_str().unwrap_or_default())
                .map_err(|error| fail("invalid_value", error.message()))?;
            service.use_profile(&profile, scope_kind).map(|_| ())
        }
        "table" => {
            let mut toggles: Vec<Toggle> = Vec::new();
            if let Some(map) = value.as_object() {
                for (raw_id, enabled) in map {
                    let id = CapsuleId::parse(raw_id)
                        .map_err(|error| fail("invalid_value", error.message()))?;
                    toggles.push(Toggle::new(id, enabled.as_bool() == Some(true)));
                }
            }
            service
                .apply(ApplyRequest {
                    scope: scope_kind,
                    toggles,
                    label: None,
                })
                .map(|_| ())
        }
        "list" => {
            let sets: Vec<String> = value
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(|s| s.trim().to_string()))
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default();
            crate::projects::set_defaults(service.home(), &sets).map(|_| ())
        }
        other => unreachable!("no executor for kind {other}"),
    };
    result.map_err(|error| {
        fail(
            "internal",
            format!("the native mutation failed: {}", error.message()),
        )
    })
}

/// Restore the owner baseline for one setting at one scope: remove the scope's
/// own declarations so lower scopes decide again.
fn execute_reset(
    service: &mut Service,
    setting: &Setting,
    address: &ScopeAddress,
) -> Result<(), Failure> {
    let scope_kind = address
        .aikit_kind()
        .expect("writable settings always map to an AIKit scope");
    let result = match setting.setting_ref {
        "ai-kit:resolution:resolution.profiles" => {
            service.reset_scope_profiles(scope_kind).map(|_| ())
        }
        "ai-kit:skills:skills.capabilities" => service.clear_scope_toggles(scope_kind).map(|_| ()),
        "ai-kit:resolution:skill-sets.default" => {
            crate::projects::set_defaults(service.home(), &[]).map(|_| ())
        }
        other => unreachable!("no reset executor for {other}"),
    };
    result.map_err(|error| {
        fail(
            "internal",
            format!("the native reset failed: {}", error.message()),
        )
    })
}

pub fn apply(service: &mut Service, args: crate::cli::ConfigApplyArgs) -> Result<Value, Failure> {
    let text = match args.plan_file.as_str() {
        "-" => {
            let mut buffer = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut buffer)
                .map_err(|error| fail("internal", format!("could not read stdin: {error}")))?;
            buffer
        }
        path => std::fs::read_to_string(path).map_err(|error| {
            fail(
                "validation_failed",
                format!("could not read --plan-file {}: {error}", path),
            )
        })?,
    };
    let plan: Value = serde_json::from_str(text.trim()).map_err(|error| {
        fail(
            "validation_failed",
            format!("the plan is not valid JSON: {error}"),
        )
    })?;
    if plan["schema"] != json!(PLAN_SCHEMA) {
        return Err(fail(
            "unsupported_schema",
            format!(
                "the plan carries schema `{}`, not `{PLAN_SCHEMA}`",
                plan["schema"].as_str().unwrap_or_default()
            ),
        ));
    }
    let setting_ref = plan["setting_ref"].as_str().unwrap_or_default();
    let setting = find_setting(setting_ref).ok_or_else(|| {
        fail(
            "unsupported_setting",
            format!("the plan names `{setting_ref}`, which this owner does not contribute"),
        )
    })?;
    if !setting.writable {
        return Err(not_writable(&setting));
    }
    let digest = plan["plan_digest"].as_str().unwrap_or_default();
    let recomputed = plan_digest(&plan)?;
    if digest.is_empty() || digest != recomputed {
        return Err(fail_with(
            error_doc("validation_failed", String::new()),
            "validation_failed",
            "the plan's plan_digest does not match its own body: the plan is stale or was \
             modified after it was minted",
            setting_ref,
        ));
    }
    if plan["value"].is_null() {
        return Err(fail_with(
            error_doc("validation_failed", String::new()),
            "validation_failed",
            "the plan carries no value; this owner's plans pin the requested change",
            setting_ref,
        ));
    }

    // Rebuild the address from the plan's wire scope and hold it to the same
    // laws a direct request would face.
    let scope_wire = &plan["scope"];
    let kind = scope_wire["scope_kind"].as_str().unwrap_or_default();
    let scope_ref = scope_wire["scope_ref"].as_str().unwrap_or_default();
    let address = match kind {
        "world" => ScopeAddress::World,
        "machine" => ScopeAddress::Machine,
        "project" => ScopeAddress::Project(scope_ref.to_string()),
        "agent-session" => ScopeAddress::AgentSession(scope_ref.to_string()),
        other => {
            return Err(fail(
                "unknown_scope_kind",
                format!("the plan names unknown scope kind `{other}`"),
            ))
        }
    };
    if !setting.allowed_scopes.contains(&address.kind()) {
        return Err(fail_with(
            error_doc("unsupported_scope", String::new()),
            "unsupported_scope",
            format!(
                "the plan addresses `{}` outside the setting's allowed scopes",
                address.compact()
            ),
            setting_ref,
        ));
    }

    let changeset_id = args
        .changeset
        .unwrap_or_else(|| format!("cs-aikit-{}", Ulid::generate()));

    // Owner-side idempotency: an executed key replays as no_op, naming the
    // original receipt. The mutation never runs twice under one key.
    let store = ConfigReceiptStore::new(service.home());
    let key = executed_key(&changeset_id, &setting, &address, Some(digest.to_string()));
    if let Some(original) = store
        .find_executed(&key)
        .map_err(|error| fail("internal", error.message()))?
    {
        let original_id = original["receipt_id"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let native_ref = original["native_ref"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let replay = receipt(
            &setting,
            &address,
            &changeset_id,
            Some(digest.to_string()),
            "apply",
            "no_op",
            native_ref,
            Some(original_id),
        );
        return Ok(replay);
    }

    // The value is re-validated natively at apply time: the world may have
    // moved since the plan was minted.
    let violations = validate_value(service, &setting, &plan["value"]);
    if !violations.is_empty() {
        let messages: Vec<String> = violations
            .iter()
            .filter_map(|v| v["message"].as_str().map(str::to_string))
            .collect();
        return Err(fail_with(
            error_doc("validation_failed", String::new()),
            "validation_failed",
            format!(
                "the plan's value no longer validates: {}",
                messages.join("; ")
            ),
            setting_ref,
        ));
    }

    execute(service, &setting, &address, &plan["value"])?;
    let mut document = receipt(
        &setting,
        &address,
        &changeset_id,
        Some(digest.to_string()),
        "apply",
        "applied",
        String::new(),
        None,
    );
    let receipt_id = document["receipt_id"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    document["native_ref"] = json!(format!("aikit:config:receipts/{receipt_id}"));
    store
        .record(&document)
        .map_err(|error| fail("internal", error.message()))?;
    Ok(document)
}

pub fn reset(
    service: &mut Service,
    cwd: &Path,
    args: crate::cli::ConfigResetArgs,
) -> Result<Value, Failure> {
    let setting = find_setting(&args.setting).ok_or_else(|| {
        fail(
            "unsupported_setting",
            format!("`{}` is not a setting this owner contributes", args.setting),
        )
    })?;
    if !setting.operations.3 {
        return Err(not_writable(&setting));
    }
    let address = match args.scope {
        Some(raw) => ScopeAddress::parse(&raw)?,
        None => default_address(service)?,
    };
    if !setting.allowed_scopes.contains(&address.kind()) {
        return Err(fail_with(
            error_doc("unsupported_scope", String::new()),
            "unsupported_scope",
            format!(
                "`{}` is not addressable at `{}` scope (allowed: {})",
                setting.setting_ref,
                address.compact(),
                setting.allowed_scopes.join(", ")
            ),
            setting.setting_ref,
        ));
    }
    address.resolve_against(service, cwd)?;
    let changeset_id = args
        .changeset
        .unwrap_or_else(|| format!("cs-aikit-{}", Ulid::generate()));

    let store = ConfigReceiptStore::new(service.home());
    let key = executed_key(&changeset_id, &setting, &address, None);
    if let Some(original) = store
        .find_executed(&key)
        .map_err(|error| fail("internal", error.message()))?
    {
        let original_id = original["receipt_id"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let native_ref = original["native_ref"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        return Ok(receipt(
            &setting,
            &address,
            &changeset_id,
            None,
            "reset",
            "no_op",
            native_ref,
            Some(original_id),
        ));
    }

    execute_reset(service, &setting, &address)?;
    let mut document = receipt(
        &setting,
        &address,
        &changeset_id,
        None,
        "reset",
        "applied",
        String::new(),
        None,
    );
    let receipt_id = document["receipt_id"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    document["native_ref"] = json!(format!("aikit:config:receipts/{receipt_id}"));
    store
        .record(&document)
        .map_err(|error| fail("internal", error.message()))?;
    Ok(document)
}

/// The whole `aikit config` surface. Errors never escape as envelope failures:
/// they come back as the frozen error document plus a non-zero exit.
pub fn dispatch(cwd: &Path, command: ConfigCmd) -> Result<Value, Failure> {
    match command.command {
        crate::cli::ConfigSub::Validate(args) => {
            let service = discover(cwd)?;
            validate(&service, cwd, args)
        }
        crate::cli::ConfigSub::Plan(args) => {
            let service = discover(cwd)?;
            plan(&service, cwd, args)
        }
        crate::cli::ConfigSub::Apply(args) => {
            let mut service = discover(cwd)?;
            apply(&mut service, args)
        }
        crate::cli::ConfigSub::Reset(args) => {
            let mut service = discover(cwd)?;
            reset(&mut service, cwd, args)
        }
    }
}

fn discover(cwd: &Path) -> Result<Service, Failure> {
    Service::discover(cwd).map_err(|error| {
        fail(
            "owner_unavailable",
            format!(
                "the AIKit owner context is unavailable: {}",
                error.message()
            ),
        )
    })
}
