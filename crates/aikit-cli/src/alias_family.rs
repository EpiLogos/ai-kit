//! Alias families (ADR 0005 Stage 1): user-owned manifests of named commands
//! composed from {harness, model route, fixed args}, installed as an ordinary
//! command family.
//!
//! Families are versioned *data* (`aikit.alias-family/v1` documents under
//! `<AIKIT home>/alias-families/`), never code baked into a product. A
//! manifest cites the harness registry and the model catalogue by reference —
//! unknown harness slugs, unknown model refs and ill-shaped values are
//! refused at validation — and it never carries a binary path or key
//! material. `install` emits thin launcher scripts as generated data the
//! owner places on PATH by hand, exactly like the Stage 0 flagship the ADR
//! records (`examples/alias-families/agents/`); nothing here adds itself to
//! PATH or writes outside the AIKit home.
//!
//! Coverage honesty carries through: an entry whose harness profile records
//! no model dispatch (or no selector surface) validates as refused, naming the
//! profile's declared reason — the portal does not sell commands it cannot
//! run.

use std::collections::BTreeMap;
use std::path::PathBuf;

use aikit_adapters::profiles;
use aikit_adapters::runner::CommandRunner;
use aikit_core::harness_profile::ModelDispatchPosture;
use aikit_core::resource::canonical_model_ref;
use aikit_core::resource::ProviderRef;
use aikit_store::model_catalogue::resolved_catalogue;
use aikit_store::AikitHome;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::route_launch;

/// The exact schema key an alias-family manifest must declare.
pub const ALIAS_FAMILY_SCHEMA: &str = "aikit.alias-family/v1";

/// Where owner-owned family manifests live, relative to the AIKit home.
pub const ALIAS_FAMILIES_DIR: &str = "alias-families";

/// Where `alias install` emits generated launcher scripts, relative to the
/// families directory. Generated data, never automatically on PATH.
pub const INSTALLED_DIR: &str = "installed";

/// One user-owned command: a harness, the canonical Model to run it against,
/// an optional provider pin, and fixed arguments passed through on every run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AliasEntry {
    /// A harness the client registry knows (its catalog slug, registry name
    /// or a registered alias).
    pub harness: String,
    /// The canonical `model:<stable-id>` ref.
    pub model: String,
    /// Pin the route's provider. A pin constrains the route; it never changes
    /// which Model was selected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Fixed arguments appended on every launch, before the caller's own.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
}

/// One `aikit.alias-family/v1` document. Entries are keyed by command name;
/// the name becomes the installed command's file name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AliasFamilyManifest {
    pub schema: String,
    /// The family's identity, kebab-case: it names the manifest's stanza in
    /// every verb (`aikit alias install <family>`).
    pub family: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub entries: BTreeMap<String, AliasEntry>,
}

fn error(message: impl std::fmt::Display) -> aikit_core::AikitError {
    aikit_core::AikitError::new("alias_family", message.to_string())
}

fn error_for(path: &std::path::Path, message: impl std::fmt::Display) -> aikit_core::AikitError {
    error(format!("{}: {message}", path.display()))
}

/// Read one manifest file. A wrong schema version is refused naming expected
/// and found, never reinterpreted.
pub fn read_manifest(path: &std::path::Path) -> aikit_core::Result<AliasFamilyManifest> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| error_for(path, format!("could not be read: {e}")))?;
    let manifest: AliasFamilyManifest = toml::from_str(&text).map_err(|e| {
        error_for(
            path,
            format!("is not a valid {ALIAS_FAMILY_SCHEMA} document: {e}"),
        )
    })?;
    if manifest.schema != ALIAS_FAMILY_SCHEMA {
        return Err(error_for(
            path,
            format!(
                "declares schema {:?}; expected exactly {ALIAS_FAMILY_SCHEMA}",
                manifest.schema
            ),
        ));
    }
    if manifest.family.is_empty() || manifest.family != manifest.family.trim() {
        return Err(error_for(path, "declares an empty or padded `family`"));
    }
    Ok(manifest)
}

/// Every manifest under `<home>/alias-families/*.toml`, sorted, with an honest
/// problem line per unreadable file. A missing directory is the normal
/// not-authored-yet state.
pub fn load_families(home: &AikitHome) -> (Vec<(PathBuf, AliasFamilyManifest)>, Vec<String>) {
    let dir = home.root().join(ALIAS_FAMILIES_DIR);
    let mut manifests = Vec::new();
    let mut problems = Vec::new();
    let read = match std::fs::read_dir(&dir) {
        Ok(read) => read,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (manifests, problems),
        Err(e) => {
            problems.push(format!("{}: {e}", dir.display()));
            return (manifests, problems);
        }
    };
    let mut paths: Vec<PathBuf> = read
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    paths.sort();
    for path in paths {
        match read_manifest(&path) {
            Ok(manifest) => manifests.push((path, manifest)),
            Err(e) => problems.push(e.to_string()),
        }
    }
    (manifests, problems)
}

fn valid_command_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !name.starts_with('-')
}

/// How one entry validated. `launchable` means the profile's model dispatch
/// can carry the model choice at spawn and the catalogue knows the model (and
/// the pinned provider, when given) — whether a route is *observed today* and
/// whether a key is bound is live state, disclosed by `list`, refused loudly
/// by the launcher itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryVerdict {
    Launchable {
        harness_slug: String,
        model: String,
        provider_pin: Option<String>,
    },
    Refused {
        reason: String,
    },
    Invalid {
        reason: String,
    },
}

impl EntryVerdict {
    pub fn is_launchable(&self) -> bool {
        matches!(self, EntryVerdict::Launchable { .. })
    }
}

/// Validate one entry against the harness registry, its profile facts and the
/// model catalogue. No process spawns and no credential is read.
pub fn validate_entry(
    home: &AikitHome,
    _family: &str,
    name: &str,
    entry: &AliasEntry,
) -> EntryVerdict {
    let refuse = |reason: String| EntryVerdict::Refused { reason };
    let invalid = |reason: String| EntryVerdict::Invalid { reason };
    if !valid_command_name(name) {
        return invalid(format!(
            "command name {name:?} is not a lawful command name; use kebab-case \
             ([a-z0-9-], no leading dash)"
        ));
    }
    let Some(slug) = crate::client::catalog_slug_for(&entry.harness) else {
        return invalid(format!(
            "harness {entry:?} is not in the client registry; a manifest cites the registry, \
             never a binary",
            entry = entry.harness
        ));
    };
    let Some(profile) = profiles::for_slug(slug) else {
        return invalid(format!(
            "no harness profile is carried for {slug}; the portal composes from profile facts \
             only and invents none"
        ));
    };
    let Some(models) = &profile.models else {
        return refuse(format!(
            "the {slug} profile declares no models layer, so no model choice can reach it"
        ));
    };
    // Canonical identity first: a manifest that misspells the ModelRef is
    // invalid, not merely unusable.
    let model = match canonical_model_ref(&entry.model) {
        Ok(model) => model,
        Err(e) => return invalid(format!("model ref {entry:?}: {e}", entry = entry.model)),
    };
    let pin = match &entry.provider {
        Some(raw) => match ProviderRef::parse(raw) {
            Ok(pin) => Some(pin),
            Err(e) => return invalid(format!("provider pin {raw:?}: {}", e.message())),
        },
        None => None,
    };
    let (catalogue, _) = resolved_catalogue(home);
    let Some(catalogue_entry) = catalogue.get(&model) else {
        return invalid(format!(
            "model {model} is absent from the canonical catalogue; author an owner entry \
             under <AIKIT home>/model-catalogue/ or refresh a Provider Source"
        ));
    };
    if let Some(pin) = &pin {
        if !catalogue_entry
            .routes
            .iter()
            .any(|route| &route.provider == pin)
        {
            return invalid(format!(
                "the catalogue declares no route to {model} via {pin}; declared providers: {}",
                catalogue_entry
                    .routes
                    .iter()
                    .map(|route| route.provider.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    // Profile dispatch truth: the same gate the launcher applies, applied at
    // validation so a family that cannot run is named now, not at 3am.
    match &models.dispatch {
        ModelDispatchPosture::None { reason } => refuse(format!(
            "the {slug} profile records no model dispatch, so no foreign model can be \
             route-launched into it; the profile's declared reason: {reason}"
        )),
        ModelDispatchPosture::ProviderPlural => {
            if models.argv_selectors.is_none() {
                refuse(format!(
                    "the {slug} profile declares provider-plural dispatch but records no \
                     observed per-invocation argv selectors; no selector was invented"
                ))
            } else {
                EntryVerdict::Launchable {
                    harness_slug: slug.to_string(),
                    model: model.to_string(),
                    provider_pin: pin.map(|p| p.to_string()),
                }
            }
        }
        ModelDispatchPosture::NativeProviderBinding {
            provider_ref,
            selector_kind,
            ..
        } => {
            if let Some(pin) = &pin {
                if pin.as_str() != provider_ref {
                    return refuse(format!(
                        "the {slug} profile natively binds {provider_ref}; the pinned {pin} \
                         cannot serve it"
                    ));
                }
            }
            if selector_kind != "argv-flag" {
                refuse(format!(
                    "the {slug} profile binds {provider_ref} through a {selector_kind:?} \
                     selector, and AIKit's launch path has no one-shot {selector_kind:?} \
                     surface for it"
                ))
            } else {
                EntryVerdict::Launchable {
                    harness_slug: slug.to_string(),
                    model: model.to_string(),
                    provider_pin: pin.map(|p| p.to_string()),
                }
            }
        }
    }
}

/// Validate a whole family: every entry, plus the family-level findings.
pub fn validate_family(
    home: &AikitHome,
    path: &std::path::Path,
    manifest: &AliasFamilyManifest,
) -> Vec<(String, EntryVerdict)> {
    let _ = path;
    let _ = family_check(manifest);
    manifest
        .entries
        .iter()
        .map(|(name, entry)| {
            (
                name.clone(),
                validate_entry(home, &manifest.family, name, entry),
            )
        })
        .collect()
}

fn family_check(manifest: &AliasFamilyManifest) -> Vec<String> {
    let mut findings = Vec::new();
    if !valid_command_name(&manifest.family) {
        findings.push(format!(
            "family name {:?} is not kebab-case ([a-z0-9-], no leading dash)",
            manifest.family
        ));
    }
    if manifest.entries.is_empty() {
        findings.push("the family declares no entries; delete the manifest or add one".into());
    }
    findings
}

/// One row of `alias list`/`alias check`: what the entry cites, whether it
/// can launch at all, and — when live evidence was consulted — what would
/// happen if it ran right now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EntryReading {
    pub family: String,
    pub entry: String,
    pub harness: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// `launchable`, `refused` or `invalid`.
    pub verdict: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live: Option<serde_json::Value>,
}

/// Read every family on the machine. When `runner` is given, each launchable
/// entry gains its live route state (observation + credential presence, never
/// material) exactly as the launcher would see it.
pub fn read_all(
    home: &AikitHome,
    runner: Option<&dyn CommandRunner>,
) -> (Vec<EntryReading>, Vec<String>) {
    let (manifests, mut problems) = load_families(home);
    let mut readings = Vec::new();
    let mut joined: BTreeMap<String, aikit_core::resource::ModelRouteSet> = BTreeMap::new();
    for (path, manifest) in manifests {
        for finding in family_check(&manifest) {
            problems.push(format!("{}: {finding}", path.display()));
        }
        for (name, entry) in &manifest.entries {
            let verdict = validate_entry(home, &manifest.family, name, entry);
            let live = if let (
                true,
                Some(runner),
                EntryVerdict::Launchable {
                    harness_slug,
                    model,
                    provider_pin,
                },
            ) = (runner.is_some(), runner, &verdict)
            {
                Some(live_state(
                    home,
                    runner,
                    harness_slug,
                    model,
                    provider_pin.as_deref(),
                    &mut joined,
                ))
            } else {
                None
            };
            let (verdict_word, reason) = match &verdict {
                EntryVerdict::Launchable { .. } => ("launchable", None),
                EntryVerdict::Refused { reason } => ("refused", Some(reason.clone())),
                EntryVerdict::Invalid { reason } => ("invalid", Some(reason.clone())),
            };
            readings.push(EntryReading {
                family: manifest.family.clone(),
                entry: name.clone(),
                harness: entry.harness.clone(),
                model: entry.model.clone(),
                provider: entry.provider.clone(),
                args: entry.args.clone(),
                verdict: verdict_word.into(),
                reason,
                live,
            });
        }
    }
    (readings, problems)
}

/// The live route state one entry would meet if launched now: the selected
/// route (or the refusal the launcher would raise), with credential presence
/// from the binding store. Presence only — nothing is materialised.
fn live_state(
    home: &AikitHome,
    runner: &dyn CommandRunner,
    harness_slug: &str,
    model_ref: &str,
    provider_pin: Option<&str>,
    joined: &mut BTreeMap<String, aikit_core::resource::ModelRouteSet>,
) -> serde_json::Value {
    let pin = provider_pin.and_then(|p| ProviderRef::parse(p).ok());
    let set = match joined.get(model_ref) {
        Some(set) => set.clone(),
        None => {
            let model = match canonical_model_ref(model_ref) {
                Ok(model) => model,
                Err(e) => return json!({"state": "refuses", "reason": e.to_string()}),
            };
            match route_launch::joined_routes(runner, home, &model) {
                Ok((set, _notes)) => {
                    joined.insert(model_ref.to_string(), set.clone());
                    set
                }
                Err(e) => return json!({"state": "refuses", "reason": e.to_string()}),
            }
        }
    };
    match route_launch::select_route(&set, pin.as_ref()) {
        route_launch::RouteSelection::Selected(route) => json!({
            "state": "ready",
            "provider": route.provider.as_str(),
            "provider_native_id": route.provider_native_id,
            "route_kind": route.kind.as_str(),
            "credential": match &route.credential {
                aikit_core::resource::CredentialCondition::NotRequired =>
                    "not required".to_string(),
                aikit_core::resource::CredentialCondition::Satisfied { binding_ref, .. } =>
                    format!("bound ({binding_ref})"),
                aikit_core::resource::CredentialCondition::Required { hint } =>
                    format!("unbound ({hint})"),
            },
        }),
        route_launch::RouteSelection::Ambiguous(providers) => json!({
            "state": "refuses",
            "reason": format!(
                "several usable routes ({}); pin --provider",
                providers.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
            ),
        }),
        route_launch::RouteSelection::NoneUsable(reasons)
        | route_launch::RouteSelection::NoneViable(reasons) => json!({
            "state": "refuses",
            "reason": reasons.join("; "),
            "harness": harness_slug,
        }),
    }
}

/// `aikit alias install <family>`: emit the generated launcher scripts.
/// The family must validate clean — one refusable entry refuses the whole
/// install, loudly, because a partially installed family would silently
/// diverge from its manifest. Output lands under
/// `<home>/alias-families/installed/<family>/` (or `--out`), as executable
/// *data* the owner places on PATH by hand, exactly as the Stage 0 flagship
/// documents; nothing here touches PATH.
pub fn install(
    home: &AikitHome,
    family: &str,
    out_dir: Option<std::path::PathBuf>,
) -> aikit_core::Result<serde_json::Value> {
    let manifest_path = home
        .root()
        .join(ALIAS_FAMILIES_DIR)
        .join(format!("{family}.toml"));
    let manifest = read_manifest(&manifest_path)?;
    let verdicts = validate_family(home, &manifest_path, &manifest);
    let mut problems = family_check(&manifest);
    for (name, verdict) in &verdicts {
        if let EntryVerdict::Refused { reason } | EntryVerdict::Invalid { reason } = verdict {
            problems.push(format!("entry {name:?}: {reason}"));
        }
    }
    if !problems.is_empty() {
        return Err(error(format!(
            "`{family}` does not validate; nothing was installed:\n  - {}",
            problems.join("\n  - ")
        )));
    }
    let out = out_dir.unwrap_or_else(|| {
        home.root()
            .join(ALIAS_FAMILIES_DIR)
            .join(INSTALLED_DIR)
            .join(&manifest.family)
    });
    std::fs::create_dir_all(&out)
        .map_err(|e| error(format!("could not create {}: {e}", out.display())))?;
    let mut installed = Vec::new();
    for (name, entry) in &manifest.entries {
        let script = shim(&manifest_path, &manifest.family, name, entry);
        let path = out.join(name);
        std::fs::write(&path, script)
            .map_err(|e| error(format!("could not write {}: {e}", path.display())))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| error(format!("could not mark {} executable: {e}", path.display())))?;
        }
        installed.push(json!({
            "entry": name,
            "path": path.display().to_string(),
            "runs": shim_command_line(entry),
        }));
    }
    Ok(json!({
        "family": manifest.family,
        "manifest": manifest_path.display().to_string(),
        "installed": installed,
        "out": out.display().to_string(),
        "next": format!(
            "the launchers are generated data: place them on PATH by hand (for example \
             cp {out}/* ~/.local/bin/), exactly like the Stage 0 example \
             (examples/alias-families/agents/); they cite the manifest, carry no key \
             material, and refuse honestly when a route or credential is missing",
            out = out.display()
        ),
    }))
}

/// The command line a shim execs — the route launcher with the entry's
/// fixed composition.
fn shim_command_line(entry: &AliasEntry) -> String {
    let mut line = format!(
        "aikit harness run --harness {} --model {}",
        entry.harness, entry.model
    );
    if let Some(provider) = &entry.provider {
        line.push_str(&format!(" --provider {provider}"));
    }
    line.push_str(" --");
    for arg in &entry.args {
        line.push_str(&format!(" {arg}"));
    }
    line.push_str(" \"$@\"");
    line
}

/// The generated launcher script. Deliberately thin: it cites the manifest it
/// came from and execs the route launcher; keys travel only through AIKit's
/// credential seam, never through this file.
fn shim(path: &std::path::Path, family: &str, name: &str, entry: &AliasEntry) -> String {
    format!(
        "#!/bin/sh
# {name} — generated by `aikit alias install {family}` from {path} ({ALIAS_FAMILY_SCHEMA}).
# A route portal launcher (ADR 0005): it cites the harness registry and a model
# route by reference and carries no key material; credentials confirm and
# deliver through AIKit's seam at run time. Generated data — install it on
# PATH by hand, exactly like the Stage 0 flagship
# (examples/alias-families/agents/).
exec {command}
",
        path = path.display(),
        command = shim_command_line(entry),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_store::AikitHome;

    fn home() -> (tempfile::TempDir, AikitHome) {
        let dir = tempfile::tempdir().unwrap();
        let home = AikitHome::at(dir.path().to_path_buf());
        (dir, home)
    }

    fn write_manifest(home: &AikitHome, body: &str) -> std::path::PathBuf {
        let dir = home.root().join(ALIAS_FAMILIES_DIR);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("dev.toml");
        std::fs::write(&path, body).unwrap();
        path
    }

    const MANIFEST: &str = r#"
schema = "aikit.alias-family/v1"
family = "dev"
description = "route-launched coding harnesses"

[entries.llama-pi]
harness = "pi"
model = "model:llama3.2"

[entries.claude-opus]
harness = "claude"
model = "model:claude-opus-5"
"#;

    #[test]
    fn a_manifest_parses_and_round_trips_its_entries() {
        let (_dir, home) = home();
        let path = write_manifest(&home, MANIFEST);
        let manifest = read_manifest(&path).unwrap();
        assert_eq!(manifest.family, "dev");
        assert_eq!(manifest.entries.len(), 2);
        assert_eq!(manifest.entries["llama-pi"].harness, "pi");
        assert_eq!(manifest.entries["llama-pi"].model, "model:llama3.2");
        assert!(manifest.entries["claude-opus"].args.is_empty());
    }

    #[test]
    fn a_wrong_schema_is_refused_naming_expected_and_found() {
        let (_dir, home) = home();
        let path = write_manifest(&home, &MANIFEST.replace("/v1", "/v2"));
        let error = read_manifest(&path).unwrap_err();
        assert!(error.to_string().contains(ALIAS_FAMILY_SCHEMA), "{error}");
        assert!(error.to_string().contains("v2"), "{error}");
    }

    #[test]
    fn an_unknown_model_ref_is_invalid_and_names_the_catalogue_remediation() {
        let (_dir, home) = home();
        write_manifest(&home, MANIFEST);
        let (readings, problems) = read_all(&home, None);
        assert!(problems.is_empty(), "{problems:?}");
        let llama = readings
            .iter()
            .find(|r| r.entry == "llama-pi")
            .expect("entry row");
        assert_eq!(llama.verdict, "launchable", "{:?}", llama.reason);
        let opus = readings
            .iter()
            .find(|r| r.entry == "claude-opus")
            .expect("entry row");
        assert_eq!(
            opus.verdict, "refused",
            "claude's config-key selector cannot carry a model at spawn"
        );
        assert!(
            opus.reason
                .as_deref()
                .unwrap_or_default()
                .contains("config-key"),
            "the refusal names the selector kind: {:?}",
            opus.reason
        );
    }

    #[test]
    fn an_unknown_harness_slug_is_invalid() {
        let (_dir, home) = home();
        write_manifest(
            &home,
            r#"
schema = "aikit.alias-family/v1"
family = "dev"
[entries.ghost]
harness = "no-such-harness"
model = "model:llama3.2"
"#,
        );
        let (readings, _) = read_all(&home, None);
        assert_eq!(readings[0].verdict, "invalid");
        assert!(readings[0]
            .reason
            .as_deref()
            .unwrap()
            .contains("client registry"));
    }

    #[test]
    fn a_none_dispatch_harness_refuses_with_the_profile_reason() {
        let (_dir, home) = home();
        write_manifest(
            &home,
            r#"
schema = "aikit.alias-family/v1"
family = "dev"
[entries.zed]
harness = "zcode"
model = "model:claude-opus-5"
"#,
        );
        let (readings, _) = read_all(&home, None);
        assert_eq!(readings[0].verdict, "refused");
        let reason = readings[0].reason.as_deref().unwrap();
        assert!(
            reason.contains("declares no native provider binding"),
            "the refusal carries the profile's declared reason: {reason}"
        );
    }

    #[test]
    fn install_refuses_a_family_with_any_non_launchable_entry() {
        let (_dir, home) = home();
        write_manifest(&home, MANIFEST);
        let error = install(&home, "dev", None).unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("claude-opus"),
            "the refusal names the offending entry: {message}"
        );
        assert!(
            !home
                .root()
                .join(ALIAS_FAMILIES_DIR)
                .join(INSTALLED_DIR)
                .exists(),
            "nothing was written"
        );
    }

    #[test]
    fn install_emits_executable_data_citing_the_manifest_and_no_keys() {
        let (_dir, home) = home();
        write_manifest(
            &home,
            r#"
schema = "aikit.alias-family/v1"
family = "dev"
[entries.llama-pi]
harness = "pi"
model = "model:llama3.2"
args = ["--no-session-persistence"]
"#,
        );
        let data = install(&home, "dev", None).unwrap();
        let out = data["out"].as_str().unwrap();
        let script_path = std::path::PathBuf::from(out).join("llama-pi");
        let script = std::fs::read_to_string(&script_path).unwrap();
        assert!(script.starts_with("#!/bin/sh"));
        assert!(script.contains("aikit.alias-family/v1"));
        assert!(script.contains("model:llama3.2"));
        assert!(script.contains("--no-session-persistence"));
        assert!(script.contains("carries no key material"));
        assert!(!script.contains("sk-") && !script.contains("API_KEY="));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&script_path)
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o111, 0o111, "the shim is executable data");
        }
        assert!(
            !data["installed"][0]["runs"]
                .as_str()
                .unwrap()
                .contains("--provider"),
            "no provider pin was invented"
        );
    }
}
