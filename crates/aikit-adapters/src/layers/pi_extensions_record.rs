//! The pi extensions-record grammar: settings documents that spell extension
//! modules as a string array under a declared key path (`extensions` in pi's
//! global `~/.pi/agent/settings.json`), each entry a path to a TypeScript
//! module pi loads through jiti.
//!
//! Entries AIKit does not own are preserved untouched. The one managed entry
//! is the extension carrier's projected path, identified by the ownership
//! marker matched in the entry's file name — the array counterpart of the
//! hook sweeps' dispatch-command identity and the record map's marker match.
//! Because the carrier file is content-addressed (`aikit-hook-carrier-<hash
//! .ts>`), a re-projection of a new revision means the previous owned entry
//! is swept and the fresh one added, never accumulated: two carrier entries
//! would load two carriers and fire every hook twice. A sweep-only merge
//! (`managed = None`) removes every owned entry and adds nothing — the
//! projection of "this carrier is not active" — while foreign entries, and
//! anything in the array that is not a path string, survive untouched.
//! Reports carry entry file names only; errors carry paths and problems,
//! because native config files carry secrets.

use super::{LayerMergeError, MergeReport};

/// Merge the managed carrier entry into an existing document's extension
/// array at `key_path`, preserving every entry AIKit does not own. `managed`
/// is `Some(projected carrier path)` when the carrier is active and `None`
/// for a sweep-only merge.
pub fn pi_extensions_record(
    existing: &serde_json::Value,
    key_path: &[&str],
    managed: Option<&str>,
    ownership: &str,
) -> Result<(serde_json::Value, MergeReport), LayerMergeError> {
    if ownership.trim().is_empty() {
        return Err(LayerMergeError::new(
            "layers.empty_ownership_identity",
            "the ownership identity that tells AIKit's extension entries apart from the \
             harness's own must not be empty; set the profile's ownership marker (for \
             example \"aikit-hook-carrier\") before merging extension records",
        ));
    }

    let path = key_path.join(".");
    let mut document = match existing {
        serde_json::Value::Null => serde_json::json!({}),
        serde_json::Value::Object(_) => existing.clone(),
        _ => {
            return Err(LayerMergeError::new(
                "client.settings_unreadable",
                "the existing document is not a JSON object; AIKit will not overwrite a \
                 file it cannot read",
            )
            .with("path", path.clone()))
        }
    };

    // Navigate to (or create) the declared key path; every intermediate node
    // must be an object the navigation can extend.
    let mut node = &mut document;
    for (depth, key) in key_path.iter().enumerate() {
        let is_final = depth + 1 == key_path.len();
        match node {
            serde_json::Value::Object(map) => {
                let value = map.entry((*key).to_string()).or_insert_with(|| {
                    if is_final {
                        serde_json::json!([])
                    } else {
                        serde_json::json!({})
                    }
                });
                if !is_final && !value.is_object() {
                    return Err(array_error(key_path, false, &key_path[..=depth].join(".")));
                }
                node = value;
            }
            // Unreachable: the document root was refused above when it was
            // not an object, and `node` only ever rebinds to a value the arm
            // above has checked to be an object.
            _ => return Err(array_error(key_path, is_final, &path)),
        }
    }

    let entries = match node {
        serde_json::Value::Array(entries) => entries,
        serde_json::Value::Null => {
            // An explicit `null` where the array belongs is treated as absent,
            // the same courtesy the record grammar extends a null document.
            *node = serde_json::json!([]);
            node.as_array_mut().expect("just replaced with an array")
        }
        _ => return Err(array_error(key_path, true, &path)),
    };

    let mut removed = Vec::new();
    let mut kept_foreign = Vec::new();
    entries.retain(|entry| match entry.as_str() {
        Some(value) => {
            if carries_ownership(value, ownership) && Some(value) != managed {
                removed.push(file_name_of(value));
                false
            } else {
                if Some(value) != managed {
                    kept_foreign.push(file_name_of(value));
                }
                true
            }
        }
        // A shape AIKit did not write; preserved untouched.
        None => {
            kept_foreign.push("<non-string entry>".to_string());
            true
        }
    });

    let mut added = Vec::new();
    let mut replaced = Vec::new();
    if let Some(managed_path) = managed {
        let fresh = !entries
            .iter()
            .any(|entry| entry.as_str() == Some(managed_path));
        if fresh {
            entries.push(serde_json::Value::String(managed_path.to_string()));
            if removed.is_empty() {
                added.push(file_name_of(managed_path));
            } else {
                replaced.push(file_name_of(managed_path));
            }
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

/// Does an extension entry carry AIKit's ownership marker? Matched in the
/// entry's file name, mirroring how the hook sweeps recognise their entries
/// by dispatch command: a foreign path that merely contains the marker in a
/// parent directory is not AIKit's.
fn carries_ownership(entry: &str, ownership: &str) -> bool {
    file_name_of(entry).starts_with(ownership)
}

/// The file name of a path entry, the only part reports ever carry.
fn file_name_of(entry: &str) -> String {
    entry
        .rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(entry)
        .to_string()
}

/// The refusal for a document whose declared key path cannot be traversed.
fn array_error(key_path: &[&str], is_final: bool, blocked_at: &str) -> LayerMergeError {
    let path = key_path.join(".");
    let message = if is_final {
        format!(
            "the existing `{path}` value is not an array; AIKit will not replace an \
             extension list it cannot read"
        )
    } else {
        format!(
            "the key path `{path}` cannot be created because `{blocked_at}` is not a JSON \
             object"
        )
    };
    LayerMergeError::new("client.settings_unreadable", message).with("path", path)
}

#[cfg(test)]
mod tests {
    use aikit_core::harness_profile::MergeGrammar;

    use crate::layers::{apply_merge, MergeArgs};

    use super::{pi_extensions_record, MergeReport};

    const OWNERSHIP: &str = "aikit-hook-carrier";

    fn fresh_carrier() -> String {
        "/Users/admin/.aikit/ctx/projections/pi/aikit-hook-carrier-aaa111.ts".to_string()
    }

    fn stale_carrier() -> String {
        "/Users/admin/.aikit/ctx/projections/pi/aikit-hook-carrier-old999.ts".to_string()
    }

    fn foreign_extension() -> String {
        "/Users/admin/my-extensions/grammars.ts".to_string()
    }

    #[test]
    fn a_fresh_document_gains_the_managed_carrier_under_the_declared_key_path() {
        let (merged, report) = pi_extensions_record(
            &serde_json::json!({}),
            &["extensions"],
            Some(&fresh_carrier()),
            OWNERSHIP,
        )
        .unwrap();

        assert_eq!(
            merged,
            serde_json::json!({ "extensions": [fresh_carrier()] })
        );
        assert_eq!(
            report,
            MergeReport {
                added: vec!["aikit-hook-carrier-aaa111.ts".to_string()],
                replaced: vec![],
                removed: vec![],
                kept_foreign: vec![],
            }
        );
    }

    #[test]
    fn a_null_document_is_treated_as_fresh() {
        let (merged, report) = pi_extensions_record(
            &serde_json::Value::Null,
            &["extensions"],
            Some(&fresh_carrier()),
            OWNERSHIP,
        )
        .unwrap();

        assert_eq!(merged["extensions"], serde_json::json!([fresh_carrier()]));
        assert_eq!(
            report.added,
            vec!["aikit-hook-carrier-aaa111.ts".to_string()]
        );
    }

    #[test]
    fn foreign_extension_entries_survive_a_merge_untouched() {
        let existing = serde_json::json!({
            "theme": "dark",
            "extensions": [foreign_extension()]
        });
        let (merged, report) = pi_extensions_record(
            &existing,
            &["extensions"],
            Some(&fresh_carrier()),
            OWNERSHIP,
        )
        .unwrap();

        assert_eq!(
            merged,
            serde_json::json!({
                "theme": "dark",
                "extensions": [foreign_extension(), fresh_carrier()]
            })
        );
        assert_eq!(
            report,
            MergeReport {
                added: vec!["aikit-hook-carrier-aaa111.ts".to_string()],
                replaced: vec![],
                removed: vec![],
                kept_foreign: vec!["grammars.ts".to_string()],
            }
        );
    }

    #[test]
    fn a_previous_carrier_revision_is_swept_and_replaced_not_accumulated() {
        let existing = serde_json::json!({ "extensions": [stale_carrier(), foreign_extension()] });
        let (merged, report) = pi_extensions_record(
            &existing,
            &["extensions"],
            Some(&fresh_carrier()),
            OWNERSHIP,
        )
        .unwrap();

        assert_eq!(
            merged,
            serde_json::json!({ "extensions": [foreign_extension(), fresh_carrier()] })
        );
        assert_eq!(
            report.removed,
            vec!["aikit-hook-carrier-old999.ts".to_string()]
        );
        assert_eq!(
            report.replaced,
            vec!["aikit-hook-carrier-aaa111.ts".to_string()]
        );
        assert!(report.added.is_empty());
        assert_eq!(report.kept_foreign, vec!["grammars.ts".to_string()]);
    }

    #[test]
    fn an_already_current_carrier_is_left_in_place_and_reported_as_no_change() {
        let existing = serde_json::json!({ "extensions": [foreign_extension(), fresh_carrier()] });
        let (merged, report) = pi_extensions_record(
            &existing,
            &["extensions"],
            Some(&fresh_carrier()),
            OWNERSHIP,
        )
        .unwrap();

        assert_eq!(
            merged,
            serde_json::json!({ "extensions": [foreign_extension(), fresh_carrier()] })
        );
        assert_eq!(
            report,
            MergeReport {
                added: vec![],
                replaced: vec![],
                removed: vec![],
                kept_foreign: vec!["grammars.ts".to_string()],
            }
        );
    }

    #[test]
    fn a_sweep_only_merge_removes_every_owned_entry_and_nothing_else() {
        let existing = serde_json::json!({
            "extensions": [fresh_carrier(), stale_carrier(), foreign_extension()]
        });
        let (merged, report) =
            pi_extensions_record(&existing, &["extensions"], None, OWNERSHIP).unwrap();

        assert_eq!(
            merged,
            serde_json::json!({ "extensions": [foreign_extension()] })
        );
        assert_eq!(
            report.removed,
            vec![
                "aikit-hook-carrier-aaa111.ts".to_string(),
                "aikit-hook-carrier-old999.ts".to_string()
            ]
        );
        assert_eq!(report.kept_foreign, vec!["grammars.ts".to_string()]);
    }

    #[test]
    fn a_foreign_path_merely_containing_the_marker_in_a_parent_directory_is_not_owned() {
        let tricky = "/Users/admin/aikit-hook-carrier-notes/foreign.ts".to_string();
        let existing = serde_json::json!({ "extensions": [tricky] });
        let (merged, report) =
            pi_extensions_record(&existing, &["extensions"], None, OWNERSHIP).unwrap();

        assert_eq!(merged["extensions"], serde_json::json!([tricky]));
        assert_eq!(report.kept_foreign, vec!["foreign.ts".to_string()]);
    }

    #[test]
    fn a_non_string_entry_is_preserved_untouched_and_disclosed() {
        let existing = serde_json::json!({ "extensions": [foreign_extension(), { "path": 1 }] });
        let (merged, report) =
            pi_extensions_record(&existing, &["extensions"], None, OWNERSHIP).unwrap();

        assert_eq!(
            merged["extensions"],
            serde_json::json!([foreign_extension(), { "path": 1 }])
        );
        assert_eq!(
            report.kept_foreign,
            vec!["<non-string entry>".to_string(), "grammars.ts".to_string()]
        );
    }

    #[test]
    fn a_missing_extensions_key_is_created_as_a_fresh_array() {
        let (merged, report) = pi_extensions_record(
            &serde_json::json!({ "theme": "dark" }),
            &["extensions"],
            Some(&fresh_carrier()),
            OWNERSHIP,
        )
        .unwrap();

        assert_eq!(
            merged,
            serde_json::json!({ "theme": "dark", "extensions": [fresh_carrier()] })
        );
        assert_eq!(report.added.len(), 1);
    }

    #[test]
    fn an_extensions_value_that_is_not_an_array_refuses_naming_the_path() {
        let error = pi_extensions_record(
            &serde_json::json!({ "extensions": {} }),
            &["extensions"],
            Some(&fresh_carrier()),
            OWNERSHIP,
        )
        .unwrap_err();

        assert_eq!(error.code(), "client.settings_unreadable");
        assert!(error.message().contains("not an array"), "{error}");
    }

    #[test]
    fn a_document_root_that_is_not_an_object_refuses() {
        let error = pi_extensions_record(
            &serde_json::json!([1, 2]),
            &["extensions"],
            Some(&fresh_carrier()),
            OWNERSHIP,
        )
        .unwrap_err();

        assert_eq!(error.code(), "client.settings_unreadable");
        assert!(error.message().contains("not a JSON object"), "{error}");
    }

    #[test]
    fn an_intermediate_key_that_is_not_an_object_refuses_naming_the_block() {
        let error = pi_extensions_record(
            &serde_json::json!({ "pi": 5 }),
            &["pi", "extensions"],
            Some(&fresh_carrier()),
            OWNERSHIP,
        )
        .unwrap_err();

        assert_eq!(error.code(), "client.settings_unreadable");
        assert!(error.message().contains("`pi`"), "{error}");
    }

    #[test]
    fn no_report_and_no_error_ever_carries_an_entry_path() {
        let (_, report) = pi_extensions_record(
            &serde_json::json!({}),
            &["extensions"],
            Some(&fresh_carrier()),
            OWNERSHIP,
        )
        .unwrap();
        let rendered = format!("{report:?}");
        assert!(
            !rendered.contains("/Users/admin"),
            "a report names file entries, never their directories: {rendered}"
        );

        let error = pi_extensions_record(
            &serde_json::json!({ "extensions": 5 }),
            &["extensions"],
            Some(&fresh_carrier()),
            OWNERSHIP,
        )
        .unwrap_err();
        let rendered = format!("{error}{error:?}");
        assert!(
            !rendered.contains("/Users/admin"),
            "an error may name paths and problems, never entry values: {rendered}"
        );
    }

    #[test]
    fn an_empty_ownership_marker_refuses_rather_than_sweeping_every_foreign_entry() {
        let error = pi_extensions_record(
            &serde_json::json!({ "extensions": [foreign_extension()] }),
            &["extensions"],
            None,
            "  ",
        )
        .unwrap_err();

        assert_eq!(error.code(), "layers.empty_ownership_identity");
    }

    #[test]
    fn apply_merge_dispatches_the_extensions_grammar_from_the_profile_declaration() {
        let (merged, _) = apply_merge(
            MergeGrammar::PiExtensionsRecord,
            &serde_json::json!({}),
            MergeArgs::PiExtensionsRecord {
                key_path: vec!["extensions".to_string()],
                managed: Some(fresh_carrier()),
                ownership: OWNERSHIP.to_string(),
            },
        )
        .unwrap();

        assert_eq!(merged["extensions"], serde_json::json!([fresh_carrier()]));
    }
}
