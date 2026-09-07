//! Domain activation (W1/CASE 03): the composed engine reaction that makes
//! declared KnowledgeDomains operative.
//!
//! Domains are declared data in the project layer: `<project>/.aikit/domains/*.toml`
//! (`aikit.knowledge-domain/v1`). They load only when the active composition
//! selected `hook/continuity/domain-activation` — descope law, never
//! ambient. Activation is deterministic case-insensitive trigger matching
//! (see [`aikit_core::domain`]); injection is deduped on rendered content
//! through the store's injection ledger; standing rules are exempt by their
//! explicit classification, and the exemption is visible in the explanation.
//!
//! Fail-open: unreadable or invalid declarations are disclosed as warnings;
//! the turn proceeds.

use std::path::Path;

use aikit_core::domain::{
    activate, dedup_hash, render_header, render_rules, KnowledgeDomain,
};
use aikit_core::hooks::HookEvent;
use aikit_store::index::Index;

/// Load the domain declarations declared in the project layer.
pub fn load_domains(project_root: &Path) -> (Vec<KnowledgeDomain>, Vec<String>) {
    let mut domains = Vec::new();
    let mut warnings = Vec::new();
    let dir = project_root.join(".aikit/domains");
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(_) => return (domains, warnings), // no domains declared: honest absence
    };
    let mut paths: Vec<_> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("toml"))
        .collect();
    paths.sort();
    for path in paths {
        match std::fs::read_to_string(&path)
            .map_err(|error| error.to_string())
            .and_then(|text| KnowledgeDomain::from_toml_str(&text))
        {
            Ok(domain) => domains.push(domain),
            Err(error) => warnings.push(format!(
                "domain declaration {} refused: {error}",
                path.display()
            )),
        }
    }
    (domains, warnings)
}

/// The prompt text a submit event carries, if any.
pub fn prompt_of(event: &HookEvent) -> Option<String> {
    ["prompt", "user_prompt", "input"]
        .iter()
        .find_map(|key| event.payload.get(*key))
        .and_then(|value| value.as_str())
        .map(str::to_owned)
}

/// The dedup scope for an event: the session id when the client declared
/// one, else the project root path — stable across dispatch processes,
/// because the engine's context id is minted per invocation.
pub fn dedup_scope(event: &HookEvent, project_root: Option<&Path>) -> Option<String> {
    event
        .payload
        .get("session_id")
        .or_else(|| event.payload.get("sessionId"))
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| project_root.map(|root| root.to_string_lossy().into_owned()))
}

/// Run the reaction: activate domains on the prompt, render blocks, dedup
/// through the ledger. Returns the blocks to inject plus warnings.
///
/// The dedup law, as behaviour:
/// * ordinary payload unchanged since its last injection → not re-injected;
/// * standing rules → always reasserted, exemption visible in the block;
/// * deduped ordinary + no standing → nothing arrives at all (true dedup;
///   inspection answers "what was deduped" through `aikit context`).
pub fn run(
    index: &Index,
    scope: &str,
    domains: &[KnowledgeDomain],
    prompt: Option<&str>,
) -> (Vec<String>, Vec<String>) {
    let mut blocks = Vec::new();
    let mut warnings = Vec::new();
    let Some(prompt) = prompt else {
        return (blocks, warnings);
    };
    for activation in activate(domains, prompt) {
        let (ordinary, standing) = render_rules(&activation);
        let hash = dedup_hash(&activation.domain.id, &ordinary);
        let deduped = !ordinary.is_empty()
            && match index.injection_seen(scope, &hash) {
                Ok(seen) => seen,
                Err(error) => {
                    warnings.push(format!(
                        "continuity/domain-activation dedup check unavailable: {error}"
                    ));
                    false
                }
            };
        if deduped && standing.is_empty() {
            continue;
        }
        if !deduped && !ordinary.is_empty() {
            if let Err(error) = index.record_injection(scope, &hash) {
                warnings.push(format!(
                    "continuity/domain-activation ledger unavailable: {error}"
                ));
            }
        }
        // Deduped ordinary lines stay out of the block entirely; only
        // standing rules reassert.
        let lines: Vec<&String> = if deduped {
            standing.iter().collect()
        } else {
            standing.iter().chain(ordinary.iter()).collect()
        };
        let mut block = render_header(&activation, deduped, !standing.is_empty());
        for line in lines {
            block.push('\n');
            block.push_str(line);
        }
        blocks.push(block);
    }
    (blocks, warnings)
}
