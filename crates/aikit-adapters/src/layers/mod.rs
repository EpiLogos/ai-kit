//! The one layer merge engine: every native-config merge grammar AIKit
//! implements lives here, behind [`crate::layers::apply_merge`], dispatched on
//! the [`MergeGrammar`](aikit_core::harness_profile::MergeGrammar) a harness
//! profile declares.
//!
//! The invariant this module owns is *ownership truth in foreign documents*.
//! Whatever a grammar writes into, three laws hold everywhere: entries AIKit
//! does not own are preserved byte-for-byte; entries AIKit owns are identified
//! by their ownership identity (the dispatch command prefix for hooks, an
//! ownership marker matched in command/args/url fields for tool records) and
//! replaced rather than joined, because a stale entry beside a fresh one would
//! fire the whole chain twice; and owned entries whose capability source
//! disappeared are swept. Every merge reports what it did — [`MergeReport`] —
//! and no error or report ever carries a value from inside a projected record:
//! names and paths only, because config files carry secrets.

mod claude_hook_map;
mod mcp_servers_record;
mod pi_extensions_record;
mod zcode_hook_wrapper;

pub use claude_hook_map::claude_hook_map;
pub use mcp_servers_record::mcp_servers_record;
pub use pi_extensions_record::pi_extensions_record;
pub use zcode_hook_wrapper::zcode_hook_wrapper;

use std::collections::{BTreeMap, BTreeSet};

use aikit_core::harness_profile::MergeGrammar;
use aikit_core::hooks::HookEventKind;
use aikit_core::{AikitError, Result};

/// Whether tool-carrying events carry a `matcher` in an AIKit dispatch entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatcherPolicy {
    /// claude-code matches tool names against a glob; `*` is its match-all.
    StarForTools,
    /// codex's hooks.json and zcode's configuration events match by regex
    /// (or exact name) and an omitted matcher matches everything; a matcher
    /// AIKit invented would narrow what the dispatcher sees.
    Omitted,
}

impl MatcherPolicy {
    /// The matcher an AIKit entry carries for one event, if any.
    fn matcher_for(&self, event: &HookEventKind) -> Option<&'static str> {
        match self {
            MatcherPolicy::StarForTools if event.carries_tool_name() => Some("*"),
            _ => None,
        }
    }
}

/// What one merge did, for receipts and disclosure. Only entry and event
/// names ever appear here — never record contents, because native config
/// files carry secrets.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
pub struct MergeReport {
    /// Entries AIKit inserted: no AIKit entry existed there before.
    pub added: Vec<String>,
    /// Entries where a previous AIKit entry was replaced by the fresh one.
    pub replaced: Vec<String>,
    /// Names removed from the document: event keys emptied by the ownership
    /// sweep, and owned records swept because no managed definition covers
    /// them anymore.
    pub removed: Vec<String>,
    /// Entries AIKit found, did not own, and preserved untouched.
    pub kept_foreign: Vec<String>,
}

/// A layer merge refusal. The `code` is stable machine surface; the message
/// names what failed and what would fix it. Converts into `AikitError`
/// unchanged, so the adapter seams keep their error contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerMergeError {
    code: &'static str,
    message: String,
    details: BTreeMap<String, String>,
}

impl LayerMergeError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn with(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.details.insert(key.into(), value.into());
        self
    }

    pub fn code(&self) -> &'static str {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn details(&self) -> &BTreeMap<String, String> {
        &self.details
    }
}

impl std::fmt::Display for LayerMergeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)?;
        if !self.details.is_empty() {
            let rendered: Vec<String> = self
                .details
                .iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect();
            write!(f, " ({})", rendered.join(", "))?;
        }
        Ok(())
    }
}

impl std::error::Error for LayerMergeError {}

impl From<LayerMergeError> for AikitError {
    fn from(error: LayerMergeError) -> Self {
        let mut aikit_error = AikitError::new(error.code, error.message);
        for (key, value) in error.details {
            aikit_error = aikit_error.with(key, value);
        }
        aikit_error
    }
}

/// The per-grammar arguments [`apply_merge`] needs, one variant per grammar.
/// The grammar/variant pairing is checked at dispatch: a grammar handed
/// another grammar's arguments is a refusal, never a guess.
pub enum MergeArgs {
    ClaudeHookMap {
        events: Vec<(HookEventKind, String)>,
        client: String,
        matchers: MatcherPolicy,
    },
    ZcodeHookWrapper {
        events: Vec<(HookEventKind, String)>,
        client: String,
    },
    McpServersRecord {
        key_path: Vec<String>,
        managed: BTreeMap<String, serde_json::Value>,
        ownership: String,
    },
    PiExtensionsRecord {
        key_path: Vec<String>,
        /// The projected carrier's absolute path when the carrier is active;
        /// `None` is a sweep-only merge — every owned entry removed.
        managed: Option<String>,
        ownership: String,
    },
}

/// The one merge entrypoint: dispatch the grammar a harness profile declares
/// onto an existing native-config document.
pub fn apply_merge(
    grammar: MergeGrammar,
    existing: &serde_json::Value,
    args: MergeArgs,
) -> std::result::Result<(serde_json::Value, MergeReport), LayerMergeError> {
    match (grammar, args) {
        (
            MergeGrammar::ClaudeHookMap,
            MergeArgs::ClaudeHookMap {
                events,
                client,
                matchers,
            },
        ) => claude_hook_map(existing, &events, matchers, &client),
        (MergeGrammar::ZcodeHookWrapper, MergeArgs::ZcodeHookWrapper { events, client }) => {
            zcode_hook_wrapper(existing, &events, &client)
        }
        (
            MergeGrammar::McpServersRecord,
            MergeArgs::McpServersRecord {
                key_path,
                managed,
                ownership,
            },
        ) => {
            let key_path: Vec<&str> = key_path.iter().map(String::as_str).collect();
            mcp_servers_record(existing, &key_path, &managed, &ownership)
        }
        (
            MergeGrammar::PiExtensionsRecord,
            MergeArgs::PiExtensionsRecord {
                key_path,
                managed,
                ownership,
            },
        ) => {
            let key_path: Vec<&str> = key_path.iter().map(String::as_str).collect();
            pi_extensions_record(existing, &key_path, managed.as_deref(), &ownership)
        }
        (grammar, _) => Err(LayerMergeError::new(
            "layers.grammar_arguments_mismatch",
            format!(
                "the {grammar:?} merge grammar was handed another grammar's arguments; pass \
                 the `MergeArgs` variant matching the grammar the profile declares"
            ),
        )),
    }
}

// ---------------------------------------------------------------------------
// The shared hook-record machinery
// ---------------------------------------------------------------------------

/// The command AIKit installs for one event, on every hook-carrying harness.
pub fn dispatch_command(client: &str, event: &HookEventKind) -> String {
    format!("aikit hook dispatch {client} {event}")
}

/// Is this an AIKit dispatcher entry — including a stale one from an older
/// install that spelled the event differently?
fn is_aikit_entry(client: &str, command: &str) -> bool {
    command
        .trim()
        .starts_with(&format!("aikit hook dispatch {client}"))
}

/// Clone an existing document for merging, treating a missing file's `Null`
/// as an empty one. A document of any other shape is refused with the
/// grammar's own wording, because AIKit will not overwrite what it cannot read.
fn document_for_merging(
    existing: &serde_json::Value,
    not_an_object: &'static str,
) -> std::result::Result<serde_json::Value, LayerMergeError> {
    match existing {
        serde_json::Value::Null => Ok(serde_json::json!({})),
        serde_json::Value::Object(_) => Ok(existing.clone()),
        _ => Err(LayerMergeError::new(
            "client.settings_unreadable",
            not_an_object,
        )),
    }
}

/// Navigate to (or create) a hook document's `hooks` block. Both hook grammars
/// spell this level the same way and refuse the same way when the user's file
/// has something else there.
fn hooks_block_of(
    document: &mut serde_json::Value,
) -> std::result::Result<&mut serde_json::Map<String, serde_json::Value>, LayerMergeError> {
    document
        .as_object_mut()
        .and_then(|object| {
            object
                .entry("hooks")
                .or_insert_with(|| serde_json::json!({}))
                .as_object_mut()
        })
        .ok_or_else(|| {
            LayerMergeError::new(
                "client.settings_unreadable",
                "the existing `hooks` value is not an object",
            )
        })
}

/// One AIKit dispatch entry, exactly as it lands in the native document.
fn dispatch_entry(client: &str, event: &HookEventKind, matcher: Option<&str>) -> serde_json::Value {
    let mut entry = serde_json::Map::new();
    if let Some(matcher) = matcher {
        entry.insert("matcher".to_string(), serde_json::json!(matcher));
    }
    entry.insert(
        "hooks".to_string(),
        serde_json::json!([{
            "type": "command",
            "command": dispatch_command(client, event),
        }]),
    );
    serde_json::Value::Object(entry)
}

/// Remove every event key emptied by the sweep, so an old install does not
/// leave `"Stop": []` behind forever. Returns the names that vanished.
fn prune_emptied_event_keys(
    events: &mut serde_json::Map<String, serde_json::Value>,
) -> Vec<String> {
    let before: Vec<String> = events.keys().cloned().collect();
    events.retain(|_, entries| entries.as_array().is_none_or(|a| !a.is_empty()));
    before
        .into_iter()
        .filter(|key| !events.contains_key(key))
        .collect()
}

/// Where AIKit entries stood before the merge, and where foreign hooks remain.
struct OwnershipSweep {
    owned_found: BTreeSet<String>,
    foreign_present: BTreeSet<String>,
}

/// Remove every AIKit dispatch entry from each event's matcher list, keeping
/// everything that is not ours. A previous install may have written an entry
/// under an event AIKit no longer dispatches, or under a misspelling, so the
/// sweep runs everywhere, before any fresh entry is written.
fn sweep_owned_entries(
    events: &mut serde_json::Map<String, serde_json::Value>,
    client: &str,
) -> OwnershipSweep {
    let mut sweep = OwnershipSweep {
        owned_found: BTreeSet::new(),
        foreign_present: BTreeSet::new(),
    };
    for (name, entries) in events.iter_mut() {
        let Some(matcher_entries) = entries.as_array_mut() else {
            continue;
        };
        let mut owned_here = false;
        let mut foreign_here = false;
        for matcher in matcher_entries.iter_mut() {
            match matcher
                .get_mut("hooks")
                .and_then(|hooks| hooks.as_array_mut())
            {
                Some(list) => {
                    for hook in list.iter() {
                        if hook
                            .get("command")
                            .and_then(|command| command.as_str())
                            .is_some_and(|command| is_aikit_entry(client, command))
                        {
                            owned_here = true;
                        } else {
                            foreign_here = true;
                        }
                    }
                    list.retain(|hook| {
                        !hook
                            .get("command")
                            .and_then(|command| command.as_str())
                            .is_some_and(|command| is_aikit_entry(client, command))
                    });
                }
                // A shape AIKit did not write; preserved untouched.
                None => foreign_here = true,
            }
        }
        matcher_entries.retain(|matcher| {
            matcher
                .get("hooks")
                .and_then(|hooks| hooks.as_array())
                .is_none_or(|list| !list.is_empty())
        });
        if owned_here {
            sweep.owned_found.insert(name.clone());
        }
        if foreign_here {
            sweep.foreign_present.insert(name.clone());
        }
    }
    sweep
}

/// The one hook-merge body, shared by every hook grammar: sweep owned entries
/// everywhere, write one fresh entry per dispatched event, prune the keys the
/// sweep emptied, and report what happened.
///
/// `container` is the events map the grammar navigated to (`hooks` for the
/// Claude grammar, `hooks.events` for the zcode wrapper); `container_label`
/// is how a refusal spells that path. `matchers` decides whether an AIKit
/// entry carries a matcher — claude-code's `*` on tool events, nothing where
/// a matcher AIKit invented would narrow what the dispatcher sees.
fn merge_hook_entries(
    container: &mut serde_json::Map<String, serde_json::Value>,
    events: &[(HookEventKind, String)],
    client: &str,
    container_label: &str,
    matchers: MatcherPolicy,
) -> std::result::Result<MergeReport, LayerMergeError> {
    let sweep = sweep_owned_entries(container, client);

    let mut added = BTreeSet::new();
    let mut replaced = BTreeSet::new();
    for (event, native_name) in events {
        let list = container
            .entry(native_name.clone())
            .or_insert_with(|| serde_json::json!([]));
        let array = list.as_array_mut().ok_or_else(|| {
            LayerMergeError::new(
                "client.settings_unreadable",
                format!("the existing `{container_label}.{event}` value is not an array"),
            )
        })?;
        array.push(dispatch_entry(client, event, matchers.matcher_for(event)));
        if sweep.owned_found.contains(native_name) {
            replaced.insert(native_name.clone());
        } else {
            added.insert(native_name.clone());
        }
    }

    Ok(MergeReport {
        added: added.into_iter().collect(),
        replaced: replaced.into_iter().collect(),
        removed: prune_emptied_event_keys(container),
        kept_foreign: sweep.foreign_present.into_iter().collect(),
    })
}

// ---------------------------------------------------------------------------
// The string seam the file-reading adapters call
// ---------------------------------------------------------------------------

/// Parse an existing native-config file for a delegate seam: a missing or
/// whitespace-only file is an empty document; anything unparseable is refused
/// with the caller's own wording, naming what will not happen.
pub(crate) fn parse_existing_document(
    existing: Option<&str>,
    unreadable: impl FnOnce(String) -> AikitError,
) -> Result<serde_json::Value> {
    match existing {
        None => Ok(serde_json::json!({})),
        Some(raw) if raw.trim().is_empty() => Ok(serde_json::json!({})),
        Some(raw) => serde_json::from_str(raw).map_err(|e| unreadable(e.to_string())),
    }
}

/// Render a merged document the way every native config is written: pretty,
/// with a trailing newline.
pub(crate) fn render_document(
    document: &serde_json::Value,
    unrenderable: impl FnOnce(String) -> AikitError,
) -> Result<String> {
    let mut rendered =
        serde_json::to_string_pretty(document).map_err(|e| unrenderable(e.to_string()))?;
    rendered.push('\n');
    Ok(rendered)
}

#[cfg(test)]
mod golden_tests {
    use aikit_core::harness_profile::MergeGrammar;
    use aikit_core::hooks::HookEventKind;

    use crate::clients::hook_map::{merge_hook_map_entries, MatcherPolicy};
    use crate::clients::zcode::merge_dispatcher_entries;
    use crate::layers::{apply_merge, MergeArgs, MergeReport};

    fn claude_events(pairs: &[(&str, &str)]) -> Vec<(HookEventKind, String)> {
        pairs
            .iter()
            .map(|(event, native)| (HookEventKind::parse(event), native.to_string()))
            .collect()
    }

    fn zcode_events(pairs: &[(&str, &str)]) -> Vec<(HookEventKind, String)> {
        claude_events(pairs)
    }

    // -----------------------------------------------------------------------
    // Goldens: claude_hook_map, captured from `merge_hook_map_entries`.
    // -----------------------------------------------------------------------

    #[test]
    fn golden_a_fresh_claude_settings_file_gains_one_entry_per_event_with_matchers_on_tool_events_only(
    ) {
        let merged = merge_hook_map_entries(
            None,
            &claude_events(&[
                ("pre-tool-use", "PreToolUse"),
                ("stop", "Stop"),
                ("session-start", "SessionStart"),
            ]),
            "claude",
            MatcherPolicy::StarForTools,
        )
        .unwrap();

        assert_eq!(
            merged,
            r#"{
  "hooks": {
    "PreToolUse": [
      {
        "hooks": [
          {
            "command": "aikit hook dispatch claude PreToolUse",
            "type": "command"
          }
        ],
        "matcher": "*"
      }
    ],
    "SessionStart": [
      {
        "hooks": [
          {
            "command": "aikit hook dispatch claude SessionStart",
            "type": "command"
          }
        ]
      }
    ],
    "Stop": [
      {
        "hooks": [
          {
            "command": "aikit hook dispatch claude Stop",
            "type": "command"
          }
        ]
      }
    ]
  }
}
"#
        );
    }

    #[test]
    fn golden_claude_foreign_hooks_and_unrelated_keys_survive_a_merge() {
        let existing = r#"{
  "model": "opus",
  "hooks": {
    "PreToolUse": [
      { "matcher": "Bash", "hooks": [ { "type": "command", "command": "my-own-guard" } ] }
    ],
    "PreCompact": [
      { "hooks": [ { "type": "command", "command": "my-own-compactor" } ] }
    ]
  }
}"#;
        let merged = merge_hook_map_entries(
            Some(existing),
            &claude_events(&[
                ("pre-tool-use", "PreToolUse"),
                ("session-start", "SessionStart"),
            ]),
            "claude",
            MatcherPolicy::StarForTools,
        )
        .unwrap();

        assert_eq!(
            merged,
            r#"{
  "hooks": {
    "PreCompact": [
      {
        "hooks": [
          {
            "command": "my-own-compactor",
            "type": "command"
          }
        ]
      }
    ],
    "PreToolUse": [
      {
        "hooks": [
          {
            "command": "my-own-guard",
            "type": "command"
          }
        ],
        "matcher": "Bash"
      },
      {
        "hooks": [
          {
            "command": "aikit hook dispatch claude PreToolUse",
            "type": "command"
          }
        ],
        "matcher": "*"
      }
    ],
    "SessionStart": [
      {
        "hooks": [
          {
            "command": "aikit hook dispatch claude SessionStart",
            "type": "command"
          }
        ]
      }
    ]
  },
  "model": "opus"
}
"#
        );
    }

    #[test]
    fn golden_claude_a_previous_entry_is_replaced_and_a_stale_spelling_is_swept() {
        let existing = r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"aikit hook dispatch claude Stopp"}]}],"SessionStart":[{"hooks":[{"type":"command","command":"aikit hook dispatch claude SessionStart"}]}]}}"#;
        let merged = merge_hook_map_entries(
            Some(existing),
            &claude_events(&[("stop", "Stop")]),
            "claude",
            MatcherPolicy::StarForTools,
        )
        .unwrap();

        assert_eq!(
            merged,
            r#"{
  "hooks": {
    "Stop": [
      {
        "hooks": [
          {
            "command": "aikit hook dispatch claude Stop",
            "type": "command"
          }
        ]
      }
    ]
  }
}
"#
        );
    }

    #[test]
    fn golden_claude_event_keys_emptied_by_the_sweep_are_pruned() {
        let existing = r#"{"hooks":{"PreCompact":[],"Chores":[{"hooks":[{"type":"command","command":"mine"}]}],"Stop":[{"hooks":[{"type":"command","command":"aikit hook dispatch claude Old"}]}]}}"#;
        let merged = merge_hook_map_entries(
            Some(existing),
            &claude_events(&[]),
            "claude",
            MatcherPolicy::StarForTools,
        )
        .unwrap();

        assert_eq!(
            merged,
            r#"{
  "hooks": {
    "Chores": [
      {
        "hooks": [
          {
            "command": "mine",
            "type": "command"
          }
        ]
      }
    ]
  }
}
"#
        );
    }

    #[test]
    fn golden_claude_a_settings_file_that_is_not_json_is_refused_rather_than_overwritten() {
        let error = merge_hook_map_entries(
            Some("{ this is not json"),
            &claude_events(&[("stop", "Stop")]),
            "claude",
            MatcherPolicy::StarForTools,
        )
        .unwrap_err();

        assert_eq!(error.code(), "client.settings_unreadable");
        assert!(
            error
                .message()
                .starts_with("the existing settings are not valid JSON"),
            "the refusal must say what it will not do: {error}"
        );
    }

    // -----------------------------------------------------------------------
    // Goldens: zcode_hook_wrapper, captured from `merge_dispatcher_entries`.
    // -----------------------------------------------------------------------

    #[test]
    fn golden_a_fresh_zcode_configuration_gains_the_enabled_wrapper_without_matchers() {
        let merged = merge_dispatcher_entries(
            None,
            &zcode_events(&[("session-start", "SessionStart"), ("stop", "Stop")]),
        )
        .unwrap();

        assert_eq!(
            merged,
            r#"{
  "hooks": {
    "enabled": true,
    "events": {
      "SessionStart": [
        {
          "hooks": [
            {
              "command": "aikit hook dispatch zcode SessionStart",
              "type": "command"
            }
          ]
        }
      ],
      "Stop": [
        {
          "hooks": [
            {
              "command": "aikit hook dispatch zcode Stop",
              "type": "command"
            }
          ]
        }
      ]
    }
  }
}
"#
        );
    }

    #[test]
    fn golden_zcode_foreign_events_survive_and_a_flagsless_hooks_block_gains_enabled() {
        let existing = r#"{
  "model": "sonnet",
  "hooks": {
    "events": {
      "Stop": [ { "hooks": [ { "type": "command", "command": "my-own-stopper" } ] } ]
    }
  }
}"#;
        let merged = merge_dispatcher_entries(
            Some(existing),
            &zcode_events(&[("session-start", "SessionStart"), ("stop", "Stop")]),
        )
        .unwrap();

        assert_eq!(
            merged,
            r#"{
  "hooks": {
    "enabled": true,
    "events": {
      "SessionStart": [
        {
          "hooks": [
            {
              "command": "aikit hook dispatch zcode SessionStart",
              "type": "command"
            }
          ]
        }
      ],
      "Stop": [
        {
          "hooks": [
            {
              "command": "my-own-stopper",
              "type": "command"
            }
          ]
        },
        {
          "hooks": [
            {
              "command": "aikit hook dispatch zcode Stop",
              "type": "command"
            }
          ]
        }
      ]
    }
  },
  "model": "sonnet"
}
"#
        );
    }

    #[test]
    fn golden_zcode_a_previous_entry_is_replaced_and_a_stale_spelling_is_swept() {
        let existing = r#"{"hooks":{"enabled":true,"events":{"Stop":[{"hooks":[{"type":"command","command":"aikit hook dispatch zcode Stopp"}]}]}}}"#;
        let merged =
            merge_dispatcher_entries(Some(existing), &zcode_events(&[("stop", "Stop")])).unwrap();

        assert_eq!(
            merged,
            r#"{
  "hooks": {
    "enabled": true,
    "events": {
      "Stop": [
        {
          "hooks": [
            {
              "command": "aikit hook dispatch zcode Stop",
              "type": "command"
            }
          ]
        }
      ]
    }
  }
}
"#
        );
    }

    #[test]
    fn golden_zcode_events_emptied_by_the_sweep_are_pruned_while_foreign_events_remain() {
        let existing = r#"{"hooks":{"enabled":true,"events":{"Stop":[{"hooks":[{"type":"command","command":"aikit hook dispatch zcode Stopp"}]}],"SessionStart":[{"hooks":[{"type":"command","command":"mine"}]}]}}}"#;
        let merged = merge_dispatcher_entries(
            Some(existing),
            &zcode_events(&[("session-start", "SessionStart")]),
        )
        .unwrap();

        assert_eq!(
            merged,
            r#"{
  "hooks": {
    "enabled": true,
    "events": {
      "SessionStart": [
        {
          "hooks": [
            {
              "command": "mine",
              "type": "command"
            }
          ]
        },
        {
          "hooks": [
            {
              "command": "aikit hook dispatch zcode SessionStart",
              "type": "command"
            }
          ]
        }
      ]
    }
  }
}
"#
        );
    }

    #[test]
    fn golden_zcode_an_explicitly_disabled_hooks_block_refuses_with_the_stable_error() {
        let existing = r#"{"hooks":{"enabled":false,"events":{"Stop":[{"hooks":[{"type":"command","command":"mine"}]}]}}}"#;
        let error = merge_dispatcher_entries(Some(existing), &zcode_events(&[("stop", "Stop")]))
            .unwrap_err();

        assert_eq!(error.code(), "client.hooks_disabled_by_user");
        assert_eq!(
            error.message(),
            "zcode's configuration-file hooks are explicitly disabled \
             (`hooks.enabled: false`); enabling the runner would also activate hooks the \
             user kept disabled, so AIKit refuses instead of flipping the flag"
        );
    }

    #[test]
    fn golden_zcode_a_configuration_that_is_not_json_is_refused_rather_than_overwritten() {
        let error =
            merge_dispatcher_entries(Some("not json {"), &zcode_events(&[("stop", "Stop")]))
                .unwrap_err();

        assert_eq!(error.code(), "client.settings_unreadable");
        assert!(
            error
                .message()
                .starts_with("the existing zcode configuration is not valid JSON"),
            "the refusal must say what it will not do: {error}"
        );
    }

    // -----------------------------------------------------------------------
    // Parity: the one engine, dispatched through `apply_merge`, must produce
    // exactly what the string seams produce for the same inputs.
    // -----------------------------------------------------------------------

    fn rendered(document: &serde_json::Value) -> String {
        let mut rendered = serde_json::to_string_pretty(document).unwrap();
        rendered.push('\n');
        rendered
    }

    #[test]
    fn apply_merge_renders_the_claude_grammar_byte_identically_to_the_string_seam() {
        let existing = r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"aikit hook dispatch claude Stopp"}]}],"PreCompact":[{"hooks":[{"type":"command","command":"my-own-compactor"}]}]}}"#;

        let seam = merge_hook_map_entries(
            Some(existing),
            &claude_events(&[("stop", "Stop"), ("user-prompt-submit", "UserPromptSubmit")]),
            "claude",
            MatcherPolicy::StarForTools,
        )
        .unwrap();
        let (document, report) = apply_merge(
            MergeGrammar::ClaudeHookMap,
            &serde_json::from_str::<serde_json::Value>(existing).unwrap(),
            MergeArgs::ClaudeHookMap {
                events: claude_events(&[
                    ("stop", "Stop"),
                    ("user-prompt-submit", "UserPromptSubmit"),
                ]),
                client: "claude".to_string(),
                matchers: MatcherPolicy::StarForTools,
            },
        )
        .unwrap();

        assert_eq!(rendered(&document), seam);
        assert_eq!(
            report,
            MergeReport {
                added: vec!["UserPromptSubmit".to_string()],
                replaced: vec!["Stop".to_string()],
                removed: vec![],
                kept_foreign: vec!["PreCompact".to_string()],
            }
        );
    }

    #[test]
    fn apply_merge_renders_the_zcode_grammar_byte_identically_to_the_string_seam() {
        let existing = r#"{"model":"sonnet","hooks":{"enabled":true,"events":{"Stop":[{"hooks":[{"type":"command","command":"my-own-stopper"}]}]}}}"#;

        let seam = merge_dispatcher_entries(
            Some(existing),
            &zcode_events(&[("session-start", "SessionStart"), ("stop", "Stop")]),
        )
        .unwrap();
        let (document, _) = apply_merge(
            MergeGrammar::ZcodeHookWrapper,
            &serde_json::from_str::<serde_json::Value>(existing).unwrap(),
            MergeArgs::ZcodeHookWrapper {
                events: zcode_events(&[("session-start", "SessionStart"), ("stop", "Stop")]),
                client: "zcode".to_string(),
            },
        )
        .unwrap();

        assert_eq!(rendered(&document), seam);
    }
}
