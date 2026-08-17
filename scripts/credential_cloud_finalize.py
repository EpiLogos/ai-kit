#!/usr/bin/env python3
"""Idempotent source integration used by the cloud-only implementation session.

The connected GitHub API can replace files but cannot apply textual patches. This
script makes the small surgical edits to the two large CLI source files inside a
GitHub Actions checkout, then the workflow formats/tests the real result before
committing it. It is deleted once the integration commit is proven.
"""
from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    target = Path(path)
    text = target.read_text()
    if new in text:
        return
    if old not in text:
        raise SystemExit(f"expected integration anchor missing in {path}: {old[:120]!r}")
    target.write_text(text.replace(old, new, 1))


# Public core exports for the provider-neutral seam.
replace_once(
    "crates/aikit-core/src/lib.rs",
    """pub use credential::{\n    resolve_credential, CredentialBindingState, CredentialProviderRejection, CredentialRef,\n    CredentialResolution, CredentialResolutionRequest, ProviderResolutionExplanation,\n    SecretMaterialisationClass, SecretProviderDescriptor, SecretProviderRef, SecretProviderTier,\n    SecretRequirement, SecretRequirementRef, CREDENTIAL_RESOLUTION_VERSION,\n};""",
    """pub use credential::{\n    resolve_credential, resolve_registered_credential, CredentialBindingState,\n    CredentialProviderRejection, CredentialRef, CredentialResolution, CredentialResolutionRequest,\n    ProviderResolutionExplanation, SecretMaterialisationClass, SecretProvider,\n    SecretProviderDescriptor, SecretProviderRef, SecretProviderTier, SecretRequirement,\n    SecretRequirementRef, SecretValue, CREDENTIAL_RESOLUTION_VERSION,\n};""",
)

# Command surface.
replace_once(
    "crates/aikit-cli/src/cli.rs",
    """    /// Run the health checks.\n    Doctor(DoctorArgs),\n    /// Run an exported capability once.""",
    """    /// Run the health checks.\n    Doctor(DoctorArgs),\n    /// Inspect, bind and explicitly import credentials.\n    Credential(CredentialCmd),\n    /// Run an exported capability once.""",
)

replace_once(
    "crates/aikit-cli/src/cli.rs",
    """#[derive(Debug, Args)]\npub struct ExplainArgs {\n    /// The capability id, e.g. `skill/rust/code-review`.\n    #[arg(value_name = \"CAPABILITY\")]\n    pub capability: String,\n}""",
    """#[derive(Debug, Args)]\npub struct ExplainArgs {\n    /// The capability id, e.g. `skill/rust/code-review`.\n    #[arg(value_name = \"CAPABILITY\", required_unless_present = \"credential\")]\n    pub capability: Option<String>,\n    /// Explain resolution for this semantic CredentialRef instead of a capability.\n    #[arg(long, value_name = \"CREDENTIAL\", conflicts_with = \"capability\")]\n    pub credential: Option<String>,\n    /// Named shell/project environment source to make visible in the explanation.\n    #[arg(long, value_name = \"NAME\", requires = \"credential\")]\n    pub env_var: Option<String>,\n    /// Named project .env file. It is inspected only as part of this explicit credential flow.\n    #[arg(long, value_name = \"FILE\", requires = \"env_var\")]\n    pub project_env: Option<std::path::PathBuf>,\n    /// Explicitly permit the environment-import tier for this resolution.\n    #[arg(long, requires = \"env_var\")]\n    pub from_env: bool,\n    /// Explain the headless/CI resolution path.\n    #[arg(long, requires = \"credential\")]\n    pub headless: bool,\n}""",
)

replace_once(
    "crates/aikit-cli/src/cli.rs",
    """#[derive(Debug, Args)]\npub struct RunArgs {""",
    """#[derive(Debug, Args)]\npub struct CredentialCmd {\n    #[command(subcommand)]\n    pub command: CredentialSub,\n}\n\n#[derive(Debug, Subcommand)]\npub enum CredentialSub {\n    /// Resolve an existing binding or run the explicit initial-config flow.\n    Setup(CredentialSetupArgs),\n    /// Explain provider eligibility without materialising secret data.\n    Explain(CredentialExplainArgs),\n    /// List safe persisted binding metadata.\n    List(CredentialListArgs),\n}\n\n#[derive(Debug, Args)]\npub struct CredentialSetupArgs {\n    #[arg(value_name = \"CREDENTIAL\")]\n    pub credential: String,\n    #[arg(long, value_name = \"CONSUMER\", default_value = \"operator:aikit\")]\n    pub consumer: String,\n    #[arg(long, value_name = \"PURPOSE\", default_value = \"provider authentication\")]\n    pub purpose: String,\n    #[arg(long, value_name = \"NAME\")]\n    pub env_var: Option<String>,\n    #[arg(long, value_name = \"FILE\", requires = \"env_var\")]\n    pub project_env: Option<std::path::PathBuf>,\n    /// Explicitly choose the environment-import path. Never implied by variable presence.\n    #[arg(long, requires = \"env_var\")]\n    pub from_env: bool,\n    /// Never prompt. Existing binding or explicit --from-env must resolve.\n    #[arg(long)]\n    pub headless: bool,\n}\n\n#[derive(Debug, Args)]\npub struct CredentialExplainArgs {\n    #[arg(value_name = \"CREDENTIAL\")]\n    pub credential: String,\n    #[arg(long, value_name = \"CONSUMER\", default_value = \"operator:aikit\")]\n    pub consumer: String,\n    #[arg(long, value_name = \"PURPOSE\", default_value = \"credential resolution explanation\")]\n    pub purpose: String,\n    #[arg(long, value_name = \"NAME\")]\n    pub env_var: Option<String>,\n    #[arg(long, value_name = \"FILE\", requires = \"env_var\")]\n    pub project_env: Option<std::path::PathBuf>,\n    #[arg(long, requires = \"env_var\")]\n    pub from_env: bool,\n    #[arg(long)]\n    pub headless: bool,\n}\n\n#[derive(Debug, Args)]\npub struct CredentialListArgs {}\n\n#[derive(Debug, Args)]\npub struct RunArgs {""",
)

# Binary imports + dispatch.
replace_once(
    "crates/aikit-cli/src/main.rs",
    "use aikit_cli::{hook, multicall, run, ui};",
    "use aikit_cli::{credential, hook, multicall, run, ui};",
)
replace_once(
    "crates/aikit-cli/src/main.rs",
    """        Some(Command::Doctor(a)) => cmd_doctor(cwd, a),\n        Some(Command::Use(a)) => cmd_use(cwd, a),""",
    """        Some(Command::Doctor(a)) => cmd_doctor(cwd, a),\n        Some(Command::Credential(c)) => cmd_credential(cwd, c, json_mode),\n        Some(Command::Use(a)) => cmd_use(cwd, a),""",
)

old_explain = """fn cmd_explain(cwd: &std::path::Path, a: ExplainArgs) -> Result<Reply> {\n    let service = Service::discover(cwd)?;\n    let id = CapsuleId::parse(&a.capability)?;\n    let explanation = service.resolved().explain(&id).ok_or_else(|| {\n        AikitError::new(\n            \"resolution.unknown_capability\",\n            format!(\"{id} is not in the catalogue for this context\"),\n        )\n        .with(\"capability\", id.to_string())\n    })?;\n    let data = jval!({\n        \"id\": explanation.id.to_string(),\n        \"revision\": explanation.revision.as_ref().map(|revision| revision.as_str()),\n        \"active\": explanation.active,\n        \"declared_enabled\": explanation.declared_enabled,\n        \"selected_by\": explanation.selected_by,\n        \"required_by\": explanation.required_by,\n        \"dependencies\": explanation.dependencies,\n        \"exports\": explanation.exports,\n        \"skill_usage_overlays\": explanation.skill_usage_overlays,\n        \"unavailable\": explanation.unavailable.as_ref().map(|r| r.describe()),\n        \"render\": explanation.render(),\n    });\n    Ok(reply(&service, data, vec![]))\n}"""

new_explain = """fn cmd_explain(cwd: &std::path::Path, a: ExplainArgs) -> Result<Reply> {\n    let service = Service::discover(cwd)?;\n    if let Some(credential_ref) = a.credential {\n        let request = credential::CredentialRequest {\n            credential: aikit_core::credential::CredentialRef::new(credential_ref)?,\n            consumer_ref: \"operator:aikit-explain\".into(),\n            purpose: \"credential resolution explanation\".into(),\n            env_var: a.env_var,\n            project_env: a.project_env,\n            from_env: a.from_env,\n            headless: a.headless,\n        };\n        let inspection = credential::inspect(service.home(), &request)?;\n        return Ok(reply(\n            &service,\n            jval!({\n                \"credential\": request.credential.as_str(),\n                \"resolution\": inspection.resolution,\n                \"persisted_binding\": inspection.persisted_binding,\n                \"native_provider\": inspection.native_provider,\n                \"env_available\": inspection.env_available,\n            }),\n            vec![],\n        ));\n    }\n\n    let capability = a.capability.ok_or_else(|| {\n        AikitError::new(\"cli.usage\", \"pass a capability id or --credential CREDENTIAL\")\n    })?;\n    let id = CapsuleId::parse(&capability)?;\n    let explanation = service.resolved().explain(&id).ok_or_else(|| {\n        AikitError::new(\n            \"resolution.unknown_capability\",\n            format!(\"{id} is not in the catalogue for this context\"),\n        )\n        .with(\"capability\", id.to_string())\n    })?;\n    let data = jval!({\n        \"id\": explanation.id.to_string(),\n        \"revision\": explanation.revision.as_ref().map(|revision| revision.as_str()),\n        \"active\": explanation.active,\n        \"declared_enabled\": explanation.declared_enabled,\n        \"selected_by\": explanation.selected_by,\n        \"required_by\": explanation.required_by,\n        \"dependencies\": explanation.dependencies,\n        \"exports\": explanation.exports,\n        \"skill_usage_overlays\": explanation.skill_usage_overlays,\n        \"unavailable\": explanation.unavailable.as_ref().map(|r| r.describe()),\n        \"render\": explanation.render(),\n    });\n    Ok(reply(&service, data, vec![]))\n}\n\nfn cmd_credential(\n    cwd: &std::path::Path,\n    command: CredentialCmd,\n    json_mode: bool,\n) -> Result<Reply> {\n    let service = Service::discover(cwd)?;\n    match command.command {\n        CredentialSub::Setup(a) => {\n            let request = credential::CredentialRequest {\n                credential: aikit_core::credential::CredentialRef::new(a.credential)?,\n                consumer_ref: a.consumer,\n                purpose: a.purpose,\n                env_var: a.env_var,\n                project_env: a.project_env,\n                from_env: a.from_env,\n                headless: a.headless || json_mode,\n            };\n            let outcome = credential::setup(service.home(), &request)?;\n            Ok(reply(\n                &service,\n                jval!({\n                    \"credential\": request.credential.as_str(),\n                    \"newly_bound\": outcome.newly_bound,\n                    \"binding\": outcome.binding,\n                    \"resolution\": outcome.resolution,\n                }),\n                vec![],\n            ))\n        }\n        CredentialSub::Explain(a) => {\n            let request = credential::CredentialRequest {\n                credential: aikit_core::credential::CredentialRef::new(a.credential)?,\n                consumer_ref: a.consumer,\n                purpose: a.purpose,\n                env_var: a.env_var,\n                project_env: a.project_env,\n                from_env: a.from_env,\n                headless: a.headless || json_mode,\n            };\n            let inspection = credential::inspect(service.home(), &request)?;\n            Ok(reply(\n                &service,\n                jval!({\n                    \"credential\": request.credential.as_str(),\n                    \"resolution\": inspection.resolution,\n                    \"persisted_binding\": inspection.persisted_binding,\n                    \"native_provider\": inspection.native_provider,\n                    \"env_available\": inspection.env_available,\n                }),\n                vec![],\n            ))\n        }\n        CredentialSub::List(_) => {\n            let bindings = aikit_store::CredentialBindingStore::new(service.home()).list()?;\n            Ok(reply(\n                &service,\n                jval!({ \"bindings\": bindings, \"count\": bindings.len() }),\n                vec![],\n            ))\n        }\n    }\n}"""
replace_once("crates/aikit-cli/src/main.rs", old_explain, new_explain)
