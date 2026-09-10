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
    activate, decide_injection, dedup_hash, render_header, render_rules, KnowledgeDomain,
};
use aikit_core::hooks::HookEvent;
use aikit_core::pressure::Block;
use aikit_store::index::Index;

/// Load the domain declarations in force, personal register first, then the
/// project's own.
///
/// Domains were project-layer data only, and that had a consequence nobody
/// intended: a convention could be declared inside one project and be
/// unreachable everywhere else — including at the root register, where the root
/// wiki and all cross-project work live. A rule about how to write the wiki was
/// armed in whichever project happened to hold the file.
///
/// Two registers, same schema, same grammar:
///
/// * `<aikit home>/domains` — personal scope, in force wherever this person
///   works, including outside any project;
/// * `<project>/.aikit/domains` — the project's own, and the more specific of
///   the two.
///
/// Precedence is by domain id: a project declaration with the same id as a
/// personal one **replaces** it rather than merging with it. Merging two
/// rule lists that were authored separately would produce guidance neither
/// author wrote, and the more specific declaration is the one whose author knew
/// about the project.
pub fn load_domains_in(
    home_domains: Option<&Path>,
    project_root: Option<&Path>,
) -> (Vec<KnowledgeDomain>, Vec<String>) {
    let mut domains: Vec<KnowledgeDomain> = Vec::new();
    let mut warnings = Vec::new();
    for dir in [
        home_domains.map(Path::to_path_buf),
        project_root.map(|root| root.join(".aikit/domains")),
    ]
    .into_iter()
    .flatten()
    {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue, // no domains declared here: honest absence
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
                Ok(domain) => {
                    // Later register wins by id; the replacement is visible
                    // rather than silent, because an operator debugging which
                    // guidance arrived needs to know one declaration shadowed
                    // another.
                    if let Some(existing) = domains.iter().position(|held| held.id == domain.id) {
                        warnings.push(format!(
                            "domain {} declared in the project layer replaces the personal declaration of the same id",
                            domain.id
                        ));
                        domains[existing] = domain;
                    } else {
                        domains.push(domain);
                    }
                }
                Err(error) => warnings.push(format!(
                    "domain declaration {} refused: {error}",
                    path.display()
                )),
            }
        }
    }
    (domains, warnings)
}

/// The project-layer-only load, kept for callers that have no home to consult.
pub fn load_domains(project_root: &Path) -> (Vec<KnowledgeDomain>, Vec<String>) {
    load_domains_in(None, Some(project_root))
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
) -> (Vec<Block>, Vec<String>) {
    let mut blocks = Vec::new();
    let mut warnings = Vec::new();
    let Some(prompt) = prompt else {
        return (blocks, warnings);
    };
    for activation in activate(domains, prompt) {
        let (ordinary, standing) = render_rules(&activation);
        let hash = dedup_hash(&activation.domain.id, &ordinary);
        let seen = match index.injection_seen(scope, &hash) {
            Ok(seen) => seen,
            Err(error) => {
                warnings.push(format!(
                    "continuity/domain-activation dedup check unavailable: {error}"
                ));
                false
            }
        };
        // What the ledger's answer *means* is the shared law's to say, not
        // this reaction's.
        let decision = decide_injection(ordinary, standing, seen);
        if decision.suppressed {
            continue;
        }
        if decision.record {
            if let Err(error) = index.record_injection(scope, &hash) {
                warnings.push(format!(
                    "continuity/domain-activation ledger unavailable: {error}"
                ));
            }
        }
        blocks.push(Block {
            header: render_header(&activation, decision.deduped, decision.has_standing),
            standing: decision.standing,
            ordinary: decision.ordinary,
        });
    }
    (blocks, warnings)
}
