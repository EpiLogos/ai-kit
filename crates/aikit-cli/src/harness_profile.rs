//! The public harness-profile intake: validate, inspect, register and list
//! `aikit.harness-profile/v1` documents.
//!
//! Before this surface existed, the declarative profile system was reachable
//! only from inside the source tree: documents were embedded at build time,
//! the grammar lived in Rust types, and an external author had no verb to
//! check a document against. The intake makes the published grammar real:
//! `validate` answers field-by-field, `show` prints a carried document
//! exactly as shipped, `register` installs an external document into the
//! AIKit home where the profile registry reads it, and `list` discloses
//! everything that resolves plus everything that failed to.

use std::path::Path;

use aikit_adapters::profiles;
use aikit_core::harness_profile::HarnessProfile;
use aikit_core::{AikitError, Result};
use serde::Serialize;

use crate::cli::{HarnessProfileSub, HarnessProfileValidateArgs};

use serde_json::json as jval;

#[derive(Serialize)]
struct ProfileSummary {
    slug: String,
    edition: serde_json::Value,
    source: &'static str,
    layers: Vec<LayerSummary>,
}

#[derive(Serialize)]
struct LayerSummary {
    layer: &'static str,
    posture: Option<String>,
}

fn summary(profile: &HarnessProfile, source: &'static str) -> ProfileSummary {
    // Layers carry their posture as a field named `posture`; read each one
    // through its serialized shape rather than matching every layer type.
    let layers: Vec<LayerSummary> = [
        ("presence", serde_json::to_value(&profile.presence).ok()),
        ("skills", serde_json::to_value(&profile.skills).ok()),
        ("guidance", serde_json::to_value(&profile.guidance).ok()),
        ("hooks", serde_json::to_value(&profile.hooks).ok()),
        ("tools", serde_json::to_value(&profile.tools).ok()),
        ("models", serde_json::to_value(&profile.models).ok()),
        ("sessions", serde_json::to_value(&profile.sessions).ok()),
        ("settings", serde_json::to_value(&profile.settings).ok()),
    ]
    .into_iter()
    .map(|(layer, value)| LayerSummary {
        layer,
        posture: value.as_ref().and_then(posture_of),
    })
    .collect();
    ProfileSummary {
        slug: profile.slug.clone(),
        edition: serde_json::to_value(profile.edition).unwrap_or(serde_json::Value::Null),
        source,
        layers,
    }
}

fn posture_of(value: &serde_json::Value) -> Option<String> {
    value
        .get("posture")
        .and_then(|posture| posture.as_str())
        .map(str::to_string)
}

fn parse_document(path: &Path) -> Result<HarnessProfile> {
    let text = std::fs::read_to_string(path).map_err(|error| {
        AikitError::new(
            "harness-profile.unreadable",
            format!("could not read {}: {error}", path.display()),
        )
        .with("path", path.display().to_string())
    })?;
    let profile: HarnessProfile = toml::from_str(&text).map_err(|error| {
        AikitError::new(
            "harness-profile.parse_failed",
            format!(
                "{} is not a readable harness-profile document: {error}",
                path.display()
            ),
        )
        .with("path", path.display().to_string())
    })?;
    profile.validate().map_err(|error| {
        AikitError::new(
            "harness-profile.invalid",
            format!(
                "{} failed harness-profile validation: {error}",
                path.display()
            ),
        )
        .with("path", path.display().to_string())
    })?;
    Ok(profile)
}

/// Run one intake verb.
pub fn run(cwd: &std::path::Path, command: HarnessProfileSub) -> Result<serde_json::Value> {
    match command {
        HarnessProfileSub::Validate(HarnessProfileValidateArgs { path }) => {
            // Resolved against the real filesystem, never the service scope:
            // a validator that refuses outside a project would be useless to
            // an author mid-work.
            let path = if path.is_absolute() {
                path
            } else {
                cwd.join(path)
            };
            let profile = parse_document(&path)?;
            Ok(jval!({
                "valid": true,
                "path": path.display().to_string(),
                "profile": serde_json::to_value(summary(&profile, "document")).ok(),
            }))
        }
        HarnessProfileSub::Show(args) => {
            if let Some(raw) = profiles::embedded_raw(&args.slug) {
                return Ok(jval!({
                    "slug": args.slug,
                    "source": "embedded",
                    "document": raw,
                }));
            }
            if let Some(profile) = profiles::external_profiles().get(&args.slug) {
                return Ok(jval!({
                    "slug": args.slug,
                    "source": "external",
                    "profile": serde_json::to_value(summary(profile, "external")).ok(),
                }));
            }
            Err(AikitError::new(
                "harness-profile.unknown",
                format!(
                    "no embedded or external profile is carried under slug `{}`; \
                     `aikit harness-profile list` shows what resolves",
                    args.slug
                ),
            )
            .with("slug", args.slug))
        }
        HarnessProfileSub::Register(args) => {
            let path = if args.path.is_absolute() {
                args.path.clone()
            } else {
                cwd.join(&args.path)
            };
            let profile = parse_document(&path)?;
            let slug = profile.slug.clone();
            if profiles::for_slug(&slug).is_some() {
                return Err(AikitError::new(
                    "harness-profile.embedded_conflict",
                    format!(
                        "slug `{slug}` already resolves (embedded profiles are never overridden \
                         by registration); pick the slug the document declares or retire it"
                    ),
                )
                .with("slug", slug));
            }
            let Some(dir) = profiles::external_dir() else {
                return Err(AikitError::new(
                    "harness-profile.no_home",
                    "cannot locate an AIKit home for harness-profiles (neither AIKIT_HOME nor HOME is set)",
                ));
            };
            let destination = dir.join(format!("{slug}.toml"));
            if destination.exists() && !args.force {
                return Err(AikitError::new(
                    "harness-profile.exists",
                    format!(
                        "{} already carries a registered document; pass --force to replace it",
                        destination.display()
                    ),
                )
                .with("path", destination.display().to_string()));
            }
            std::fs::create_dir_all(&dir).map_err(|error| {
                AikitError::new(
                    "harness-profile.register_failed",
                    format!("could not create {}: {error}", dir.display()),
                )
            })?;
            let contents = std::fs::read_to_string(&path).map_err(|error| {
                AikitError::new(
                    "harness-profile.unreadable",
                    format!("could not read {}: {error}", path.display()),
                )
            })?;
            std::fs::write(&destination, contents).map_err(|error| {
                AikitError::new(
                    "harness-profile.register_failed",
                    format!("could not write {}: {error}", destination.display()),
                )
            })?;
            Ok(jval!({
                "registered": true,
                "slug": slug,
                "path": destination.display().to_string(),
                "activation": "external profiles load at process start; commands started from \
                               now on resolve this document wherever a profile joins by slug",
            }))
        }
        HarnessProfileSub::List => {
            let embedded: Vec<serde_json::Value> = profiles::all()
                .map(|(_, profile)| {
                    serde_json::to_value(summary(profile, "embedded")).unwrap_or_default()
                })
                .collect();
            let external: Vec<serde_json::Value> = profiles::external_profiles()
                .values()
                .map(|profile| {
                    serde_json::to_value(summary(profile, "external")).unwrap_or_default()
                })
                .collect();
            let problems: Vec<serde_json::Value> = profiles::external_load_problems()
                .iter()
                .map(|(name, problem)| jval!({ "document": name, "problem": problem }))
                .collect();
            Ok(jval!({
                "external_dir": profiles::external_dir().map(|dir| dir.display().to_string()),
                "embedded": embedded,
                "external": external,
                "load_problems": problems,
            }))
        }
    }
}
