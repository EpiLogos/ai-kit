//! Instantiating a `template` capsule into a project.
//!
//! `Kind::Template` has always been in the domain model — "available to
//! materialize into a project or task" — and nothing ever materialized one. This
//! module is that step, and the decision that shapes it is small and consequential:
//!
//! > **Instantiating a template writes into the user's project, which is outside
//! > `~/.aikit/state/`, so it is a Procedure.**
//!
//! STANDARDS §6 admits no exception, and taking it seriously is what buys
//! instantiation its whole safety story for free: a plan computed before anything
//! is written, a reviewable diff, an inverse per edit, staging on a branch when the
//! project is a repository, and a working undo. A bespoke copy loop would have had
//! to reinvent every one of those, worse.
//!
//! ## Substitution
//!
//! `{{param}}` placeholders are substituted in three places — the destination, each
//! payload path, and each payload body — because a service scaffold needs its own
//! name in the directory, the filename and the file. Substitution is literal and
//! non-recursive: a value that happens to contain `{{other}}` is inserted as text
//! and never re-expanded, so a parameter value cannot reach into the template.
//!
//! ## What it refuses
//!
//! An occupied destination. A template drop-in that silently overwrote a
//! hand-written file would be the single worst thing this feature could do, and the
//! fact that a Procedure could technically undo it is not the same as consent.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use aikit_core::arg::ArgSpec;
use aikit_core::capsule::{Capsule, TemplateSection};
use aikit_core::procedure::{Inverse, Plan, Procedure, ProcedureKind, WorldEdit};
use aikit_core::{AikitError, Result};

use crate::home::AikitHome;
use crate::procedure::plan_procedure;

/// The values a user supplied for a template's parameters.
pub type ParamValues = BTreeMap<String, String>;

/// Resolve every declared parameter against the supplied values, applying
/// defaults and refusing a missing required one.
///
/// Refusal happens **before planning**, so a template that cannot be instantiated
/// produces an error rather than a half-formed plan a user might review and run.
pub fn resolve_params(section: &TemplateSection, supplied: &ParamValues) -> Result<ParamValues> {
    let mut resolved = ParamValues::new();
    for spec in &section.params {
        match supplied.get(&spec.name) {
            Some(value) => {
                // Coercion validates the value against the spec's own rules
                // (choices, ranges, patterns) — the same rules a script argument
                // gets, because it is the same type.
                spec.coerce(value)?;
                resolved.insert(spec.name.clone(), value.clone());
            }
            None => match default_for(spec) {
                Some(default) => {
                    resolved.insert(spec.name.clone(), default);
                }
                None if spec.is_required() => {
                    return Err(AikitError::new(
                        "template.missing_parameter",
                        format!(
                            "`{}` is required by this template and was not supplied",
                            spec.name
                        ),
                    )
                    .with("parameter", spec.name.clone()));
                }
                None => {}
            },
        }
    }

    // A value for a parameter the template never declared is a typo, and silently
    // ignoring it would leave the user wondering why nothing changed.
    for name in supplied.keys() {
        if !section.params.iter().any(|s| &s.name == name) {
            return Err(AikitError::new(
                "template.unknown_parameter",
                format!("this template declares no parameter `{name}`"),
            )
            .with("parameter", name.clone()));
        }
    }
    Ok(resolved)
}

fn default_for(spec: &ArgSpec) -> Option<String> {
    spec.default.as_ref().map(|literal| literal.to_string())
}

/// Substitute `{{name}}` placeholders literally and non-recursively.
///
/// Walking the input once and consuming each placeholder whole means a substituted
/// value is never rescanned, so a value containing `{{service_name}}` is inserted
/// as those characters rather than expanded again.
pub fn substitute(input: &str, values: &ParamValues) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            // An unclosed placeholder is text, not an error: a template may
            // legitimately contain `{{` in prose.
            out.push_str(&rest[start..]);
            return out;
        };
        let name = after[..end].trim();
        match values.get(name) {
            Some(value) => out.push_str(value),
            // An unknown placeholder is left verbatim rather than blanked, so the
            // result shows what was not substituted instead of hiding it.
            None => out.push_str(&rest[start..start + 2 + end + 2]),
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

/// Plan the instantiation of `capsule` into `project`.
///
/// Returns a [`Procedure`]: nothing is written, the isolation is chosen by the
/// ordinary rule (a branch when the project is a repository, a shadow tree
/// otherwise), and every edit carries the inverse that removes it.
pub fn plan_instantiation(
    home: &AikitHome,
    capsule: &Capsule,
    project: &Path,
    supplied: &ParamValues,
) -> Result<Procedure> {
    let section = capsule.template().ok_or_else(|| {
        AikitError::new(
            "template.not_a_template",
            format!("{} is not a template capsule", capsule.id),
        )
        .with("capability", capsule.id.to_string())
    })?;
    let root = capsule.root.as_ref().ok_or_else(|| {
        AikitError::new(
            "template.no_payload",
            format!(
                "{} has no directory on disk, so there is nothing to instantiate",
                capsule.id
            ),
        )
        .with("capability", capsule.id.to_string())
    })?;

    let values = resolve_params(section, supplied)?;

    let payload_root = root.join(&section.root);
    if !payload_root.is_dir() {
        return Err(AikitError::new(
            "template.no_payload",
            format!(
                "{} declares its payload at `{}`, which is not a directory",
                capsule.id, section.root
            ),
        )
        .with("capability", capsule.id.to_string())
        .with("path", payload_root.display().to_string()));
    }

    let destination = match &section.destination {
        Some(raw) => project.join(substitute(raw, &values)),
        None => project.to_path_buf(),
    };

    let mut plan = Plan::new().with_note(format!(
        "instantiate {} into {}",
        capsule.id,
        destination.display()
    ));

    for relative in payload_files(&payload_root)? {
        let source = payload_root.join(&relative);
        let contents = std::fs::read(&source)
            .map_err(|e| crate::home::io_error("template.unreadable_payload", &source, &e))?;

        // Both the path and the body carry parameters.
        let target_relative = substitute(&relative.to_string_lossy(), &values);
        let target = destination.join(&target_relative);

        if target.exists() {
            return Err(AikitError::new(
                "template.destination_occupied",
                format!(
                    "{} already exists; instantiating {} would overwrite work that is not \
                     AIKit's. Choose another destination, or move the existing file first.",
                    target.display(),
                    capsule.id
                ),
            )
            .with("path", target.display().to_string())
            .with("capability", capsule.id.to_string()));
        }

        // Text payloads get substitution; anything that is not valid UTF-8 is
        // copied byte for byte, because a binary asset in a template is a real
        // case and mangling it would be worse than not substituting it.
        let rendered = match String::from_utf8(contents.clone()) {
            Ok(text) => substitute(&text, &values).into_bytes(),
            Err(_) => contents,
        };

        plan = plan.with_edit(WorldEdit::WriteFile {
            path: target,
            contents: rendered,
            // The file did not exist — the inverse of creating it is removing it.
            inverse: Inverse::Remove,
        });
    }

    if plan.is_empty() {
        return Err(AikitError::new(
            "template.no_payload",
            format!("{} has an empty payload directory", capsule.id),
        )
        .with("capability", capsule.id.to_string()));
    }

    plan_procedure(
        home,
        ProcedureKind::Custom {
            capsule: capsule.id.clone(),
        },
        plan,
    )
}

/// Every file under a payload root, relative and sorted, so a plan is deterministic.
fn payload_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .sort_by_file_name()
        .into_iter()
        .filter_map(std::result::Result::ok)
    {
        if entry.file_type().is_file() {
            let relative = entry.path().strip_prefix(root).unwrap_or(entry.path());
            files.push(relative.to_path_buf());
        }
    }
    Ok(files)
}
