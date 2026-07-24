//! Per-section config merge algebra.
//!
//! `[config.*]` for most capsules is key/value data that a higher scope should be
//! able to tweak one field of without restating the whole table — a **deep
//! merge**, which is the default. But a section that represents one *replaceable
//! record* (an MCP server entry, a command spec) must be **replaced wholesale**,
//! the way Claude MCP, mise `[tasks]` and flox all do it: a higher scope's record
//! is the record, not a field-level overlay onto the lower one.
//!
//! PRIOR-ART-ACTIONS #27: getting this explicit is the single most common fix for
//! "why isn't my config taking effect". A capsule declares `config_merge` in its
//! manifest and the resolver honours it when it folds `[config.*]` across scopes.

mod common;
use common::*;

use std::collections::BTreeMap;

use aikit_core::profile::PoolPatch;
use aikit_core::scope::{LayerOrigin, ScopeKind, ScopeLayer};

/// A layer that both enables `id` and sets a `[config.<id>]` table parsed from
/// `body`.
fn config_layer(kind: ScopeKind, id: &str, body: &str) -> ScopeLayer {
    let table: toml::value::Table = toml::from_str(body).expect("config body parses");
    let mut config = BTreeMap::new();
    config.insert(cid(id), table);
    ScopeLayer {
        kind,
        depth: 0,
        origin: LayerOrigin::new(format!("test:{}", kind.as_str())),
        patch: PoolPatch {
            profiles: vec![],
            enable: vec![cid(id)],
            disable: vec![],
            config,
        },
    }
}

#[test]
fn a_deep_merge_section_keeps_keys_a_higher_scope_did_not_restate() {
    // No `config_merge` declared → the default, deep merge. A session changes one
    // field and the project's other fields survive.
    let cap = script_with("script/test/kv", "");
    let view = Fixture::new(vec![cap])
        .with_layers(vec![
            config_layer(
                ScopeKind::Project,
                "script/test/kv",
                "profile = \"ci\"\ntimeout = \"90s\"",
            ),
            config_layer(ScopeKind::Session, "script/test/kv", "profile = \"local\""),
        ])
        .resolve()
        .expect("resolves");

    let config = &view.active[&cid("script/test/kv")].config;
    assert_eq!(
        config.get("profile").and_then(|v| v.as_str()),
        Some("local"),
        "the higher scope wins the field it restated"
    );
    assert_eq!(
        config.get("timeout").and_then(|v| v.as_str()),
        Some("90s"),
        "a deep-merge section keeps the field the higher scope left alone"
    );
}

#[test]
fn a_replace_section_drops_keys_a_higher_scope_did_not_restate() {
    // A capsule whose config is one replaceable record declares replacement. The
    // session's record IS the record; the project's `args` do not bleed through.
    let cap = script_with("script/test/mcp", "config_merge = \"replace\"");
    let view = Fixture::new(vec![cap])
        .with_layers(vec![
            config_layer(
                ScopeKind::Project,
                "script/test/mcp",
                "command = \"old\"\nargs = [\"a\", \"b\"]",
            ),
            config_layer(ScopeKind::Session, "script/test/mcp", "command = \"new\""),
        ])
        .resolve()
        .expect("resolves");

    let config = &view.active[&cid("script/test/mcp")].config;
    assert_eq!(
        config.get("command").and_then(|v| v.as_str()),
        Some("new"),
        "the higher scope's record wins"
    );
    assert!(
        config.get("args").is_none(),
        "whole-record replacement drops the record the higher scope did not restate; \
         got {config:?}"
    );
}

#[test]
fn config_merge_defaults_to_deep_and_parses_replace_from_the_manifest() {
    use aikit_core::profile::ConfigMerge;
    let deep = script_with("script/test/kv", "");
    assert_eq!(deep.config_merge, ConfigMerge::Deep);

    let replace = script_with("script/test/mcp", "config_merge = \"replace\"");
    assert_eq!(replace.config_merge, ConfigMerge::Replace);
}
