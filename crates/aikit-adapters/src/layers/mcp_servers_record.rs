//! The MCP-servers record grammar: config documents that spell tool servers
//! as a record map `{ <server name>: { command | url, args?, env?, cwd? } }`
//! under a declared key path (`mcpServers`, or `mcp.servers` where the
//! harness nests its collection).
//!
//! Records AIKit does not own are preserved untouched. A record at a managed
//! name is replaced wholesale, because the name is AIKit's and the contents
//! are what the resolved capability set says they are. An owned record whose
//! name no managed definition covers is swept, ownership being the marker
//! string matched in a record's `command`, `args` or `url` fields — the
//! record-map counterpart of the hook sweeps' dispatch-command identity.
//! Record values (env among them) reach the projected document and nothing
//! else: reports carry names, errors carry paths and problems.

use std::collections::BTreeMap;

use super::{LayerMergeError, MergeReport};

/// Merge a managed set of MCP server records into an existing document's
/// record map at `key_path`, preserving every entry AIKit does not own.
pub fn mcp_servers_record(
    existing: &serde_json::Value,
    key_path: &[&str],
    managed: &BTreeMap<String, serde_json::Value>,
    ownership: &str,
) -> Result<(serde_json::Value, MergeReport), LayerMergeError> {
    if ownership.trim().is_empty() {
        return Err(LayerMergeError::new(
            "layers.empty_ownership_identity",
            "the ownership identity that tells AIKit's server records apart from the \
             harness's own must not be empty; set the profile's ownership marker (for \
             example \"aikit\") before merging server records",
        ));
    }

    let path = key_path.join(".");
    let mut document = match existing {
        serde_json::Value::Null => serde_json::json!({}),
        serde_json::Value::Object(_) => existing.clone(),
        _ => {
            let mut error = LayerMergeError::new(
                "client.mcp_config_unreadable",
                "the existing document is not a JSON object; AIKit will not overwrite a \
                 file it cannot read",
            );
            if !path.is_empty() {
                error = error.with("path", path.clone());
            }
            return Err(error);
        }
    };

    let mut node = &mut document;
    for (depth, key) in key_path.iter().enumerate() {
        let is_final = depth + 1 == key_path.len();
        match node {
            serde_json::Value::Object(map) => {
                let value = map
                    .entry((*key).to_string())
                    .or_insert_with(|| serde_json::json!({}));
                if !value.is_object() {
                    return Err(record_map_error(
                        key_path,
                        is_final,
                        &key_path[..=depth].join("."),
                    ));
                }
                node = value;
            }
            // Unreachable: the document root was refused above when it was
            // not an object, and `node` only ever rebinds to a value the arm
            // above has checked to be an object.
            _ => {
                return Err(record_map_error(
                    key_path,
                    is_final,
                    &key_path[..=depth].join("."),
                ));
            }
        }
    }

    let collection = match node {
        serde_json::Value::Object(map) => map,
        _ => return Err(record_map_error(key_path, true, &path)),
    };

    let mut removed = Vec::new();
    let mut kept_foreign = Vec::new();
    for (name, record) in collection.iter() {
        if managed.contains_key(name) {
            continue;
        }
        if entry_carries_ownership(record, ownership) {
            removed.push(name.clone());
        } else {
            kept_foreign.push(name.clone());
        }
    }
    for name in &removed {
        collection.remove(name);
    }

    let mut added = Vec::new();
    let mut replaced = Vec::new();
    for (name, record) in managed {
        if collection.insert(name.clone(), record.clone()).is_some() {
            replaced.push(name.clone());
        } else {
            added.push(name.clone());
        }
    }

    removed.sort();
    kept_foreign.sort();
    Ok((
        document,
        MergeReport {
            added,
            replaced,
            removed,
            kept_foreign,
        },
    ))
}

/// Does a server record carry AIKit's ownership marker? Matched in the fields
/// AIKit projects into, mirroring how the hook sweeps recognise their entries
/// by dispatch command.
fn entry_carries_ownership(record: &serde_json::Value, ownership: &str) -> bool {
    ["command", "url"].into_iter().any(|field| {
        record
            .get(field)
            .and_then(serde_json::Value::as_str)
            .is_some_and(|value| value.contains(ownership))
    }) || record
        .get("args")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|args| {
            args.iter()
                .any(|arg| arg.as_str().is_some_and(|value| value.contains(ownership)))
        })
}

/// The refusal for a document whose declared key path cannot be traversed.
fn record_map_error(key_path: &[&str], is_final: bool, blocked_at: &str) -> LayerMergeError {
    let path = key_path.join(".");
    let message = if is_final {
        format!(
            "the existing `{path}` value is not an object; AIKit will not replace a server \
             record map it cannot read"
        )
    } else {
        format!(
            "the key path `{path}` cannot be created because `{blocked_at}` is not a JSON \
             object"
        )
    };
    LayerMergeError::new("client.mcp_config_unreadable", message).with("path", path)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use aikit_core::harness_profile::MergeGrammar;

    use crate::layers::{MergeArgs, apply_merge};

    use super::{MergeReport, mcp_servers_record};

    fn managed_bimba() -> serde_json::Value {
        serde_json::json!({
            "command": "/Users/admin/Central/Work/epi/bimba-portable/bimba-mcp.sh",
            "args": ["--port", "8080"],
            "env": { "BIMBA_TOKEN": "sk-super-secret" },
            "cwd": "/Users/admin/Central/Work/epi"
        })
    }

    fn foreign_fs_docs() -> serde_json::Value {
        serde_json::json!({
            "command": "npx",
            "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
        })
    }

    fn managed(entries: &[(&str, serde_json::Value)]) -> BTreeMap<String, serde_json::Value> {
        entries
            .iter()
            .map(|(name, record)| (name.to_string(), record.clone()))
            .collect()
    }

    fn merged_config(mcp_servers: serde_json::Value) -> serde_json::Value {
        serde_json::json!({ "mcpServers": mcp_servers })
    }

    #[test]
    fn a_fresh_document_gains_the_managed_records_under_the_declared_key_path() {
        let (merged, report) = mcp_servers_record(
            &serde_json::json!({}),
            &["mcpServers"],
            &managed(&[("bimba", managed_bimba())]),
            "aikit",
        )
        .unwrap();

        assert_eq!(
            merged,
            merged_config(serde_json::json!({ "bimba": managed_bimba() }))
        );
        assert_eq!(
            report,
            MergeReport {
                added: vec!["bimba".to_string()],
                replaced: vec![],
                removed: vec![],
                kept_foreign: vec![],
            }
        );
    }

    #[test]
    fn a_null_document_is_treated_as_fresh() {
        let (merged, report) = mcp_servers_record(
            &serde_json::Value::Null,
            &["mcpServers"],
            &managed(&[("bimba", managed_bimba())]),
            "aikit",
        )
        .unwrap();

        assert_eq!(
            merged,
            merged_config(serde_json::json!({ "bimba": managed_bimba() }))
        );
        assert_eq!(report.added, vec!["bimba".to_string()]);
    }

    #[test]
    fn foreign_records_survive_a_merge_byte_for_byte() {
        let existing = merged_config(serde_json::json!({
            "bimba": managed_bimba(),
            "fs-docs": foreign_fs_docs()
        }));
        let (merged, report) = mcp_servers_record(
            &existing,
            &["mcpServers"],
            &managed(&[("bimba", managed_bimba())]),
            "aikit",
        )
        .unwrap();

        assert_eq!(merged["mcpServers"]["fs-docs"], foreign_fs_docs());
        assert_eq!(
            report,
            MergeReport {
                added: vec![],
                replaced: vec!["bimba".to_string()],
                removed: vec![],
                kept_foreign: vec!["fs-docs".to_string()],
            }
        );
    }

    #[test]
    fn a_user_edited_managed_record_is_replaced_wholesale_by_name() {
        let existing = merged_config(serde_json::json!({
            "bimba": { "command": "my-own-bimba", "args": ["--elsewhere"] }
        }));
        let (merged, report) = mcp_servers_record(
            &existing,
            &["mcpServers"],
            &managed(&[("bimba", managed_bimba())]),
            "aikit",
        )
        .unwrap();

        assert_eq!(merged["mcpServers"]["bimba"], managed_bimba());
        assert_eq!(report.replaced, vec!["bimba".to_string()]);
        assert!(report.kept_foreign.is_empty());
    }

    #[test]
    fn owned_records_without_a_managed_definition_are_swept() {
        let existing = merged_config(serde_json::json!({
            "bimba": managed_bimba(),
            "retired-aikit": { "command": "/usr/local/bin/aikit", "args": ["mcp", "serve", "retired"] },
            "url-owned": { "url": "https://aikit.internal/sse" },
            "args-owned": { "command": "python", "args": ["-m", "aikit_mcp"] },
            "fs-docs": foreign_fs_docs()
        }));
        let (merged, report) = mcp_servers_record(
            &existing,
            &["mcpServers"],
            &managed(&[("bimba", managed_bimba())]),
            "aikit",
        )
        .unwrap();

        assert_eq!(merged["mcpServers"]["fs-docs"], foreign_fs_docs());
        assert!(merged["mcpServers"].get("retired-aikit").is_none());
        assert!(merged["mcpServers"].get("url-owned").is_none());
        assert!(merged["mcpServers"].get("args-owned").is_none());
        assert_eq!(
            report,
            MergeReport {
                added: vec![],
                replaced: vec!["bimba".to_string()],
                removed: vec![
                    "args-owned".to_string(),
                    "retired-aikit".to_string(),
                    "url-owned".to_string()
                ],
                kept_foreign: vec!["fs-docs".to_string()],
            }
        );
    }

    #[test]
    fn a_nested_key_path_is_created_when_missing() {
        let (merged, report) = mcp_servers_record(
            &serde_json::json!({ "mcp": {} }),
            &["mcp", "servers"],
            &managed(&[("bimba", managed_bimba())]),
            "aikit",
        )
        .unwrap();

        assert_eq!(
            merged,
            serde_json::json!({ "mcp": { "servers": { "bimba": managed_bimba() } } })
        );
        assert_eq!(report.added, vec!["bimba".to_string()]);
    }

    #[test]
    fn an_intermediate_key_that_is_not_an_object_refuses_naming_the_path() {
        let error = mcp_servers_record(
            &serde_json::json!({ "mcp": 5 }),
            &["mcp", "servers"],
            &managed(&[("bimba", managed_bimba())]),
            "aikit",
        )
        .unwrap_err();

        assert_eq!(error.code(), "client.mcp_config_unreadable");
        assert!(
            error.message().contains("`mcp`") && error.message().contains("not a JSON object"),
            "the refusal must name the key that is in the way: {error}"
        );
        assert_eq!(
            error.details().get("path").map(String::as_str),
            Some("mcp.servers")
        );
    }

    #[test]
    fn a_collection_that_is_not_an_object_refuses_naming_the_path() {
        let error = mcp_servers_record(
            &serde_json::json!({ "mcpServers": [] }),
            &["mcpServers"],
            &managed(&[("bimba", managed_bimba())]),
            "aikit",
        )
        .unwrap_err();

        assert_eq!(error.code(), "client.mcp_config_unreadable");
        assert_eq!(
            error.details().get("path").map(String::as_str),
            Some("mcpServers")
        );
    }

    #[test]
    fn a_document_root_that_is_not_an_object_refuses() {
        let error = mcp_servers_record(
            &serde_json::json!([1, 2]),
            &["mcpServers"],
            &managed(&[("bimba", managed_bimba())]),
            "aikit",
        )
        .unwrap_err();

        assert_eq!(error.code(), "client.mcp_config_unreadable");
        assert!(
            error.message().contains("not a JSON object"),
            "the refusal must say what is wrong: {error}"
        );
    }

    #[test]
    fn no_report_and_no_error_ever_carries_a_record_value() {
        // The env value is the secret-bearing part of a record; it reaches the
        // projected file and nothing else.
        let (merged, _) = mcp_servers_record(
            &serde_json::json!({}),
            &["mcpServers"],
            &managed(&[("bimba", managed_bimba())]),
            "aikit",
        )
        .unwrap();
        assert_eq!(
            merged["mcpServers"]["bimba"]["env"]["BIMBA_TOKEN"], "sk-super-secret",
            "the projected document carries the record whole"
        );

        let error = mcp_servers_record(
            &serde_json::json!({ "mcpServers": 5 }),
            &["mcpServers"],
            &managed(&[("bimba", managed_bimba())]),
            "aikit",
        )
        .unwrap_err();
        let rendered = format!("{error}{error:?}");
        assert!(
            !rendered.contains("sk-super-secret"),
            "an error may name paths and problems, never record values: {rendered}"
        );
    }

    #[test]
    fn an_empty_ownership_marker_refuses_rather_than_sweeping_every_foreign_entry() {
        let error = mcp_servers_record(
            &merged_config(serde_json::json!({ "fs-docs": foreign_fs_docs() })),
            &["mcpServers"],
            &managed(&[("bimba", managed_bimba())]),
            "  ",
        )
        .unwrap_err();

        assert_eq!(error.code(), "layers.empty_ownership_identity");
    }

    #[test]
    fn apply_merge_dispatches_the_record_grammar_from_the_profile_declaration() {
        let (merged, _) = apply_merge(
            MergeGrammar::McpServersRecord,
            &serde_json::json!({}),
            MergeArgs::McpServersRecord {
                key_path: vec!["mcpServers".to_string()],
                managed: managed(&[("bimba", managed_bimba())]),
                ownership: "aikit".to_string(),
            },
        )
        .unwrap();

        assert_eq!(
            merged,
            merged_config(serde_json::json!({ "bimba": managed_bimba() }))
        );
    }

    #[test]
    fn a_grammar_dispatched_with_another_grammars_arguments_refuses() {
        let error = apply_merge(
            MergeGrammar::McpServersRecord,
            &serde_json::json!({}),
            MergeArgs::ZcodeHookWrapper {
                events: vec![],
                client: "zcode".to_string(),
            },
        )
        .unwrap_err();

        assert_eq!(error.code(), "layers.grammar_arguments_mismatch");
    }
}
