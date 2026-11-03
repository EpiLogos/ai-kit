//! A Method written from a song's clip (O-I #844; DLW manual §19.1 Songs: "A song that has worked, run after run, can earn a
//! name. Select its clip and capture it on the Skillset rack's Capture page (§19.7): the rack writes a Method from it and sends it
//! to you"; §25.4: "A Method is a song that earned its name").
//!
//! The clip is Software Factory's render of a song (`factory.song-clip/v1`, `factory workflow export`): the units of the song in
//! its score, each on the row of the agent or agent set that takes it, what it waits on, what it returns and when it is done, and
//! the joins where stacked units meet. This module reads that document and nothing else — no Factory state, no model — and writes
//! the Method's text from it: one step per unit, in the order the song runs them. The text then travels the one capture road
//! (`aikit capture`, the inbox's scanner, its resemblance and its proposal), so the Method is a proposal that waits for the
//! person's acceptance and is never written into their ground by itself. A Method is a Skill whose description starts with
//! `METHOD:` (aikit_core::method), so the capture's title carries that prefix and the promotion keeps it as the description.
use std::collections::{BTreeMap, BTreeSet};

use aikit_core::method::METHOD_DESCRIPTION_PREFIX;
use aikit_core::{AikitError, Result};
use serde_json::Value;

/// Software Factory's song clip, the only document a song capture reads.
pub const SONG_CLIP: &str = "factory.song-clip/v1";

/// The Method a song's clip gives: its title (`METHOD:` first, the Skill's description once promoted), its text, where it came
/// from, and the steps in the order the song runs them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SongMethod {
    pub title: String,
    pub body: String,
    pub source_ref: Option<String>,
    pub steps: Vec<String>,
}

fn refused(message: impl Into<String>) -> AikitError {
    AikitError::new("capture.song_invalid", message)
}

fn text(value: &Value) -> Option<&str> {
    value.as_str().map(str::trim).filter(|s| !s.is_empty())
}

fn texts(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| items.iter().filter_map(text).map(str::to_owned).collect())
        .unwrap_or_default()
}

/// The units in the order the song runs them: a unit comes after every unit it waits on, directly or through a join that
/// releases it; units free at the same step keep the song's own order. A cycle is no song.
fn order(units: &[&Value], joins: &[Value]) -> Result<Vec<usize>> {
    let keys: Vec<String> = units
        .iter()
        .map(|u| text(&u["key"]).unwrap_or_default().to_owned())
        .collect();
    let index: BTreeMap<&str, usize> = keys
        .iter()
        .enumerate()
        .map(|(i, k)| (k.as_str(), i))
        .collect();
    let mut waits: Vec<BTreeSet<usize>> = units
        .iter()
        .map(|u| {
            texts(&u["dependencies"])
                .iter()
                .filter_map(|d| index.get(d.as_str()).copied())
                .collect()
        })
        .collect();
    for join in joins {
        let before: Vec<usize> = texts(&join["waitsFor"])
            .iter()
            .filter_map(|k| index.get(k.as_str()).copied())
            .collect();
        for released in texts(&join["releases"]) {
            if let Some(&at) = index.get(released.as_str()) {
                waits[at].extend(before.iter().copied());
            }
        }
    }
    let mut done = BTreeSet::new();
    let mut out = Vec::with_capacity(units.len());
    while out.len() < units.len() {
        let free: Vec<usize> = (0..units.len())
            .filter(|i| !done.contains(i) && waits[*i].iter().all(|w| done.contains(w)))
            .collect();
        if free.is_empty() {
            return Err(refused(
                "The clip's units wait on each other in a circle, so they run in no order: it is no song to capture.",
            ));
        }
        for i in free {
            done.insert(i);
            out.push(i);
        }
    }
    Ok(out)
}

fn slug(title: &str) -> String {
    let mut out = String::new();
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_owned()
}

fn quoted(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The Method the clip gives, titled `title` (the song's own key when none is named). Refused, in words, for anything that is not
/// a song's clip with at least one unit.
pub fn method_from_song(clip: &Value, title: Option<&str>) -> Result<SongMethod> {
    if clip["contract"].as_str() != Some(SONG_CLIP) {
        return Err(refused(format!(
            "A song capture reads a Software Factory song's clip ({SONG_CLIP}, `factory workflow export`); this document is {}.",
            clip["contract"].as_str().map_or("no contract".to_owned(), |c| format!("a {c}"))
        )));
    }
    let score = &clip["score"];
    let units: Vec<&Value> = score["units"]
        .as_array()
        .map(|a| a.iter().collect())
        .unwrap_or_default();
    if units.is_empty() {
        return Err(refused(
            "The clip holds no unit, so there is no step to capture.",
        ));
    }
    if units.iter().any(|u| text(&u["key"]).is_none()) {
        return Err(refused(
            "A unit of the clip has no key, so its steps cannot be told apart.",
        ));
    }
    let joins = score["joins"].as_array().cloned().unwrap_or_default();
    let song = text(&score["workflowKey"])
        .or_else(|| text(&clip["name"]))
        .unwrap_or("the song")
        .to_owned();
    let clip_ref = text(&clip["clipRef"]).map(str::to_owned);
    let named = title
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .unwrap_or(&song)
        .to_owned();
    let bare = named
        .strip_prefix(METHOD_DESCRIPTION_PREFIX)
        .map(str::trim)
        .unwrap_or(&named)
        .to_owned();
    let row_kind: BTreeMap<String, String> = score["rows"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|r| {
                    Some((
                        text(&r["ref"])?.to_owned(),
                        text(&r["kind"]).unwrap_or("agent").to_owned(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    let who = |row: Option<&str>| match row {
        None => "no row named".to_owned(),
        Some(r) => match row_kind.get(r).map(String::as_str) {
            Some("agent-set") => format!("the agent set {r}"),
            Some("agency") => format!("the agency {r}"),
            _ => format!("the agent {r}"),
        },
    };
    let ordered = order(&units, &joins)?;
    let mut steps = Vec::with_capacity(units.len());
    for (n, &at) in ordered.iter().enumerate() {
        let unit = units[at];
        let key = text(&unit["key"]).unwrap_or_default();
        let mut step = format!(
            "{}. **{key}** — {}",
            n + 1,
            text(&unit["concern"]).unwrap_or("(the song names no concern)")
        );
        step.push_str(&format!(". Taken by {}", who(text(&unit["row"]))));
        let deps = texts(&unit["dependencies"]);
        if !deps.is_empty() {
            step.push_str(&format!("; after {}", deps.join(", ")));
        }
        if let Some(returns) = text(&unit["returns"]) {
            step.push_str(&format!(". Returns: {returns}"));
        }
        let success = texts(&unit["success"]);
        if !success.is_empty() {
            step.push_str(&format!(". Done when: {}", success.join("; ")));
        }
        if let Some(stop) = text(&unit["stop"]) {
            step.push_str(&format!(". {stop}"));
        }
        if text(&unit["songClip"]).is_some() || unit["song"].is_object() {
            step.push_str(&format!(
                ". It is a song of its own ({}), run whole",
                text(&unit["songClip"]).unwrap_or("its clip")
            ));
        }
        step.push('.');
        steps.push(step);
    }
    let rows: Vec<String> = score["rows"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|r| text(&r["ref"]).map(|r| format!("- {}", who(Some(r)))))
                .collect()
        })
        .unwrap_or_default();
    let join_lines: Vec<String> = joins
        .iter()
        .map(|j| {
            format!(
                "- {}: once {} have returned, {} go on.",
                text(&j["key"]).unwrap_or("a join"),
                texts(&j["waitsFor"]).join(", "),
                texts(&j["releases"]).join(", ")
            )
        })
        .collect();
    let set = &clip["set"];
    let from = match (text(&set["entry"]), text(&set["byteRevision"])) {
        (Some(entry), Some(revision)) => format!(", rendered from `{entry}` at `{revision}`"),
        (Some(entry), None) => format!(", rendered from `{entry}`"),
        _ => String::new(),
    };
    let description = format!(
        "{METHOD_DESCRIPTION_PREFIX} {bare}. The song {song}, captured from its clip{}: {} step{} on {} row{}, from claim to return.",
        clip_ref.as_deref().map(|r| format!(" {r}")).unwrap_or_default(),
        steps.len(),
        if steps.len() == 1 { "" } else { "s" },
        rows.len(),
        if rows.len() == 1 { "" } else { "s" },
    );
    let mut body = String::new();
    body.push_str("---\n");
    body.push_str(&format!("name: {}\n", slug(&bare)));
    body.push_str(&format!("description: {}\n", quoted(&description)));
    body.push_str("---\n\n");
    body.push_str(&format!("# {bare}\n\n"));
    body.push_str(&format!(
        "A Method captured from the song {song} (Software Factory's clip{}{from}). Each step is a unit of the song, in the order it runs; the clip stays with the Method, so a reader can see where it came from.\n\n",
        clip_ref.as_deref().map(|r| format!(" `{r}`")).unwrap_or_default()
    ));
    body.push_str("## Steps\n\n");
    for step in &steps {
        body.push_str(step);
        body.push('\n');
    }
    if !join_lines.is_empty() {
        body.push_str("\n## Where stacked steps join\n\n");
        for line in &join_lines {
            body.push_str(line);
            body.push('\n');
        }
    }
    if !rows.is_empty() {
        body.push_str("\n## Who takes the steps\n\n");
        for line in &rows {
            body.push_str(line);
            body.push('\n');
        }
    }
    Ok(SongMethod {
        title: format!("{METHOD_DESCRIPTION_PREFIX} {bare}"),
        body,
        source_ref: clip_ref,
        steps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn clip() -> Value {
        json!({
            "contract": SONG_CLIP, "clipRef": "factory-song:o-i/release-check", "name": "release-check",
            "set": {"entry": "songs/release-check.workflow.ts", "byteRevision": "blake3:abc"},
            "score": {"workflowKey": "release-check",
                "rows": [{"ref": "agent/field", "kind": "agent"}, {"ref": "agent-set/guardians", "kind": "agent-set"}],
                "units": [
                    {"key": "report", "row": "agent/field", "dependencies": [], "concern": "Report what the guardians found", "returns": "The report"},
                    {"key": "gather", "row": "agent/field", "dependencies": [], "concern": "Gather the release", "success": ["every product named"]},
                    {"key": "check", "row": "agent-set/guardians", "dependencies": ["gather"], "concern": "Check each product"}],
                "joins": [{"key": "agree", "waitsFor": ["check"], "releases": ["report"]}]}
        })
    }

    #[test]
    fn steps_follow_the_order_the_song_runs_its_units() {
        let method = method_from_song(&clip(), Some("Release check")).unwrap();
        assert!(
            method.steps[0].starts_with("1. **gather**"),
            "{:?}",
            method.steps
        );
        assert!(
            method.steps[1].starts_with("2. **check**")
                && method.steps[1].contains("the agent set agent-set/guardians")
        );
        assert!(
            method.steps[2].starts_with("3. **report**"),
            "report waits on the join"
        );
        assert_eq!(method.title, "METHOD: Release check");
        assert_eq!(
            method.source_ref.as_deref(),
            Some("factory-song:o-i/release-check")
        );
        assert!(method
            .body
            .starts_with("---\nname: release-check\ndescription: \"METHOD: Release check."));
        assert!(method
            .body
            .contains("- agree: once check have returned, report go on."));
    }

    #[test]
    fn anything_but_a_song_clip_is_refused_in_words() {
        let error =
            method_from_song(&json!({"contract": "factory.workflow-score/v1"}), None).unwrap_err();
        assert!(
            error.to_string().contains("factory.song-clip/v1"),
            "{error}"
        );
        let mut circle = clip();
        circle["score"]["units"][1]["dependencies"] = json!(["check"]);
        assert!(method_from_song(&circle, None)
            .unwrap_err()
            .to_string()
            .contains("circle"));
    }
}
