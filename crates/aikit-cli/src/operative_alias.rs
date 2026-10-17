//! Operative aliases as saved operations: `@name = expression`.
//!
//! The grammar owner (`aikit_core::resource::operative`) defines what an alias
//! *is* and how it expands. This module is the other half: where one lives.
//!
//! It lives in the capsule envelope, as an `alias` with `syntax = "operative"` -
//! identity, scope, provenance and discovery are the envelope's, so a saved
//! search alias is found by `aikit search kind:alias`, reviewed like any capsule
//! and sits in the personal registry beside the scripts. There is no second
//! registry. What differs is the runtime: a shell alias is run by a shim, an
//! operative alias is expanded inside a query and never reaches a shell (so it
//! has no shim, needs no review to be used, and `aikit run` refuses it).
//!
//! Expansion reads the whole catalogue, active or not. An alias is a name a
//! person gave to a query; switching it "on" would be a second step that buys
//! nothing, and an alias that vanishes from search because a profile changed
//! would be a surprise.

use aikit_core::catalog::Catalog;
use aikit_core::resource::{
    parse_alias_definition, validate_alias_definition, AliasDefinition, AliasTable,
};
use aikit_core::{AikitError, CapsuleId, Result};
use aikit_store::events::Timestamp;
use aikit_store::AikitHome;

/// The operative aliases the catalogue holds: name (without `@`) to expression.
pub fn table_from_catalog(catalog: &dyn Catalog) -> AliasTable {
    catalog
        .capsules()
        .into_iter()
        .filter(|capsule| capsule.is_operative_alias())
        .filter_map(|capsule| {
            capsule
                .alias()
                .map(|alias| (alias.name.clone(), alias.body.clone()))
        })
        .collect()
}

/// What defining an alias wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Defined {
    pub id: CapsuleId,
    pub definition: AliasDefinition,
    pub manifest: std::path::PathBuf,
    /// True when an earlier definition of the same name was replaced.
    pub replaced: bool,
}

/// Define `@name = expression` in the personal registry.
///
/// Checked before anything is written: the name is a valid alias name, the body
/// is an operative expression once its gaps are filled, and following it through
/// the aliases that already exist never leads back to itself. A name already used
/// by a shell alias is refused (the two share the id `alias/personal/<name>`), and
/// redefining one's own alias needs `replace`.
pub fn define(
    home: &AikitHome,
    catalog: &dyn Catalog,
    raw: &str,
    description: Option<&str>,
    by: Option<&str>,
    replace: bool,
) -> Result<Defined> {
    let definition = parse_alias_definition(raw)?;
    let id = CapsuleId::parse(&format!("alias/personal/{}", definition.name))?;

    let mut table = table_from_catalog(catalog);
    let existing = catalog.get(&id);
    let mut replaced = false;
    if let Some(existing) = existing {
        if !existing.is_operative_alias() {
            return Err(AikitError::new(
                "alias.name_taken",
                format!(
                    "`{id}` is already a shell alias; an operative alias of the same name would \
                     share its id. Choose another name."
                ),
            )
            .with("capability", id.to_string()));
        }
        if !replace {
            return Err(AikitError::new(
                "alias.exists",
                format!(
                    "`@{}` is already defined; pass --replace to redefine it",
                    definition.name
                ),
            )
            .with("capability", id.to_string()));
        }
        // The old body must not count while the new one is checked: redefining
        // `@a` in terms of what `@a` used to be is not a cycle.
        table.remove(&definition.name);
        replaced = true;
    }
    validate_alias_definition(&definition, &table)?;

    let registry = home.registry("personal");
    let manifest_path = registry.join(id.registry_path()).join("manifest.toml");
    if replaced {
        let source = existing.and_then(|capsule| capsule.source.as_ref());
        if source.is_some_and(|source| source.as_str() != "personal") {
            return Err(AikitError::new(
                "alias.foreign",
                format!(
                    "`{id}` comes from the `{}` registry, not yours; define a different name",
                    source.map_or("", |s| s.as_str())
                ),
            ));
        }
    }

    let manifest = render(&id, &definition, description, by);
    // Parse before writing: a definition that would not load never reaches disk.
    aikit_core::Capsule::from_toml_str(&manifest)?;
    if let Some(parent) = manifest_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            AikitError::new(
                "alias.write_failed",
                format!("could not create {}: {e}", parent.display()),
            )
        })?;
    }
    let staged = manifest_path.with_extension("toml.new");
    std::fs::write(&staged, manifest.as_bytes())
        .and_then(|()| std::fs::rename(&staged, &manifest_path))
        .map_err(|e| {
            AikitError::new(
                "alias.write_failed",
                format!("could not write {}: {e}", manifest_path.display()),
            )
        })?;

    Ok(Defined {
        id,
        definition,
        manifest: manifest_path,
        replaced,
    })
}

/// TOML basic-string quoting; JSON's string escapes are a subset of it.
fn quote(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

fn render(
    id: &CapsuleId,
    definition: &AliasDefinition,
    description: Option<&str>,
    by: Option<&str>,
) -> String {
    let description = description
        .map(str::to_string)
        .unwrap_or_else(|| format!("Search alias for `{}`.", definition.body));
    let mut out = String::new();
    out.push_str(
        "# Generated by `aikit alias define`. Edit freely — this is now an ordinary capsule.\n",
    );
    out.push_str("schema = 1\n");
    out.push_str(&format!("id = {}\n", quote(&id.to_string())));
    out.push_str("kind = \"alias\"\n");
    out.push_str(&format!(
        "name = {}\n",
        quote(&format!("@{}", definition.name))
    ));
    out.push_str(&format!("description = {}\n", quote(&description)));
    out.push_str("maturity = \"stable\"\n");
    out.push_str("tags = [\"operative\", \"search\"]\n");
    out.push_str("\n[provenance]\nsource = \"authored\"\n");
    out.push_str(&format!(
        "created_at = {}\n",
        quote(&Timestamp::now().to_string())
    ));
    if let Some(by) = by {
        out.push_str(&format!("author = {}\n", quote(by)));
    }
    out.push_str("\n[alias]\n");
    out.push_str(&format!("name = {}\n", quote(&definition.name)));
    out.push_str("syntax = \"operative\"\n");
    out.push_str(&format!("body = {}\n", quote(&definition.body)));
    out
}
