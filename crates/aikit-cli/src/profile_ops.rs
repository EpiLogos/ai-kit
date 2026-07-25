//! Project-specific profile lenses.
//!
//! A fork is intentionally tiny: one project-local profile extending the base,
//! plus a reference from the project's declarations. The base is never copied,
//! so its future changes continue to flow through while the project's delta
//! remains legible.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Serialize;

use aikit_core::arg::Literal;
use aikit_core::catalog::Catalog;
use aikit_core::procedure::{Inverse, Plan, PlanDigest, Procedure, ProcedureKind, WorldEdit};
use aikit_core::profile::ProfileUse;
use aikit_core::{AikitError, ProfileId, Result};
use aikit_store::edit::ProfileDocument;

use crate::app::Service;

pub struct ProfileFork {
    pub procedure: Procedure,
    /// Identity of the exact base profile and project delta shown for review.
    pub review_digest: PlanDigest,
    pub base: ProfileId,
    pub fork: ProfileId,
    pub path: PathBuf,
}

#[derive(Serialize)]
struct ForkFile<'a> {
    schema: u32,
    id: &'a ProfileId,
    description: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    extends: Vec<&'a ProfileId>,
    #[serde(rename = "extends_use", skip_serializing_if = "Vec::is_empty")]
    extends_uses: Vec<ProfileUse>,
}

/// Build the diff-first Procedure for a project-local fork.
pub fn plan_fork(
    service: &Service,
    base: &ProfileId,
    requested_fork: Option<&str>,
    scope: &str,
    raw_bindings: &[String],
) -> Result<ProfileFork> {
    if scope != "project" {
        return Err(AikitError::new(
            "profile.unsupported_fork_scope",
            format!("profile forks are project lenses; `{scope}` is not supported"),
        )
        .with("scope", scope.to_string()));
    }
    let base_profile = Catalog::profile(service.snapshot(), base).ok_or_else(|| {
        AikitError::new(
            "resolution.unknown_profile",
            format!("{base} is not in any registry"),
        )
        .with("profile", base.to_string())
    })?;
    let bindings = parse_bindings(base_profile, raw_bindings)?;
    // Validate before writing the child. In particular, a required base
    // parameter may never become a fork that only fails later during `status`.
    base_profile.resolve_patch(&bindings)?;

    let project = service.descriptor().project_root.as_ref().ok_or_else(|| {
        AikitError::new(
            "profile.project_required",
            "profile fork needs a project with a `.aikit` marker",
        )
    })?;
    let fork = match requested_fork {
        Some(raw) => ProfileId::parse(raw)?,
        None => ProfileId::parse(&format!("profile/project/{}", profile_leaf(base)))?,
    };
    if &fork == base {
        return Err(AikitError::new(
            "profile.fork_is_base",
            "a fork needs its own id; otherwise it would extend itself",
        )
        .with("profile", base.to_string()));
    }

    let path = project
        .join(".aikit/profiles")
        .join(format!("{}.toml", fork.path()));
    if path.exists() || Catalog::profile(service.snapshot(), &fork).is_some() {
        return Err(AikitError::new(
            "profile.fork_exists",
            format!("refusing to overwrite the existing profile `{fork}`"),
        )
        .with("profile", fork.to_string())
        .with("path", path.display().to_string()));
    }

    let fork_text = toml::to_string_pretty(&ForkFile {
        schema: 1,
        id: &fork,
        description: format!("Project delta over {base}."),
        extends: if bindings.is_empty() {
            vec![base]
        } else {
            vec![]
        },
        extends_uses: if bindings.is_empty() {
            vec![]
        } else {
            vec![ProfileUse {
                profile: base.clone(),
                params: bindings,
            }]
        },
    })
    .map_err(|error| {
        AikitError::new(
            "profile.fork_encode_failed",
            format!("could not encode `{fork}`: {error}"),
        )
    })?;

    let project_profile = project.join(".aikit/profile.toml");
    let mut document = ProfileDocument::open(&project_profile)?;
    document.use_profile(&fork);
    let declarations = document.to_string();

    let plan = Plan::new()
        .with_note(format!(
            "create project lens {fork} over {base}; the fork contains only the project's delta"
        ))
        .with_edit(WorldEdit::WriteFile {
            path: path.clone(),
            contents: fork_text.into_bytes(),
            inverse: Inverse::Remove,
        })
        .with_edit(WorldEdit::WriteFile {
            path: project_profile.clone(),
            contents: declarations.into_bytes(),
            inverse: if project_profile.exists() {
                Inverse::Restore {
                    blob: aikit_core::procedure::BlobId::deferred(),
                }
            } else {
                Inverse::Remove
            },
        });
    let plan = aikit_store::procedure::bind_current_preconditions(plan)?;
    let base_source = service.profile_source(base).ok_or_else(|| {
        AikitError::new(
            "profile.source_missing",
            format!("could not locate the loaded declaration for base profile `{base}`"),
        )
    })?;
    let plan = aikit_store::procedure::bind_read_precondition(plan, &base_source)?;
    let base_fact = serde_json::to_string(base_profile).map_err(|error| {
        AikitError::new(
            "profile.fork_encode_failed",
            format!("could not bind the reviewed base profile `{base}`: {error}"),
        )
    })?;
    let review_digest = plan.review_digest(&[format!("base-profile|{base_fact}")]);
    let procedure = aikit_store::procedure::plan_procedure(
        service.home(),
        ProcedureKind::ProfileFork {
            base: base.clone(),
            fork: fork.clone(),
        },
        plan,
    )?;

    Ok(ProfileFork {
        procedure,
        review_digest,
        base: base.clone(),
        fork,
        path,
    })
}

/// The project-authored delta, without inherited contents repeated into it.
pub fn diff(service: &Service, id: &ProfileId) -> Result<serde_json::Value> {
    let profile = Catalog::profile(service.snapshot(), id).ok_or_else(|| {
        AikitError::new(
            "resolution.unknown_profile",
            format!("{id} is not in any registry"),
        )
        .with("profile", id.to_string())
    })?;
    let base = profile
        .extends
        .first()
        .or_else(|| profile.extends_uses.first().map(|parent| &parent.profile));
    let Some(base) = base else {
        return Err(AikitError::new(
            "profile.not_a_fork",
            format!("{id} does not extend a base profile"),
        )
        .with("profile", id.to_string()));
    };

    let config = serde_json::to_value(&profile.patch.config).map_err(|error| {
        AikitError::new(
            "profile.diff_encode_failed",
            format!("could not encode the config delta for {id}: {error}"),
        )
    })?;
    let base_params = profile
        .extends_uses
        .first()
        .map(|parent| serde_json::to_value(&parent.params))
        .transpose()
        .map_err(|error| {
            AikitError::new(
                "profile.diff_encode_failed",
                format!("could not encode the base bindings for {id}: {error}"),
            )
        })?
        .unwrap_or_else(|| serde_json::json!({}));
    Ok(serde_json::json!({
        "fork": id.to_string(),
        "base": base.to_string(),
        "base_params": base_params,
        "reason": profile.description,
        "enable": profile.patch.enable.iter().map(ToString::to_string).collect::<Vec<_>>(),
        "disable": profile.patch.disable.iter().map(ToString::to_string).collect::<Vec<_>>(),
        "profiles": profile.patch.profiles.iter().map(ToString::to_string).collect::<Vec<_>>(),
        "config": config,
        "params": profile.params.keys().cloned().collect::<Vec<_>>(),
    }))
}

fn parse_bindings(
    profile: &aikit_core::profile::Profile,
    raw: &[String],
) -> Result<BTreeMap<String, Literal>> {
    let mut bindings = BTreeMap::new();
    for binding in raw {
        let Some((name, value)) = binding.split_once('=') else {
            return Err(AikitError::new(
                "profile.invalid_binding",
                format!("`{binding}` is not KEY=VALUE"),
            )
            .with("binding", binding.clone()));
        };
        if name.is_empty() {
            return Err(AikitError::new(
                "profile.invalid_binding",
                "a profile parameter name may not be empty",
            ));
        }
        let declaration = profile.params.get(name).ok_or_else(|| {
            AikitError::new(
                "profile.unknown_parameter",
                format!("{} declares no parameter `{name}`", profile.id),
            )
            .with("profile", profile.id.to_string())
            .with("parameter", name.to_string())
        })?;
        let value = declaration.coerce_binding(name, value).map_err(|error| {
            AikitError::new(
                "profile.invalid_parameter",
                format!(
                    "{} parameter `{name}` is invalid: {}",
                    profile.id,
                    error.message()
                ),
            )
            .with("profile", profile.id.to_string())
            .with("parameter", name.to_string())
        })?;
        if bindings.insert(name.to_string(), value).is_some() {
            return Err(AikitError::new(
                "profile.duplicate_binding",
                format!("parameter `{name}` was supplied more than once"),
            )
            .with("parameter", name.to_string()));
        }
    }
    Ok(bindings)
}

fn profile_leaf(profile: &ProfileId) -> &str {
    profile.path().rsplit('/').next().unwrap_or("fork")
}
