//! "Used" records: which page an agent's reply took a passage from.
//!
//! The encounter journal keeps a fetch call and its output, and the reply, but nothing tied a passage of the reply to the page it
//! came from. When a turn ends, the owner records a `source-passage-used` event for each page the turn's reply cites, and only
//! where it cites one — never by matching the reply's words against the page:
//!
//! - **the harness cites it**: an ACP `session/update` `agent_message_chunk` whose content is a `resource` (its `uri` and the
//!   `text` the reply took from it) or a `resource_link` (its `uri`), the uri exactly a fetch call's url (the adapter keeps these
//!   whole as a provider update: agent_connection.rs `cites_resource`). A resource's text is the quote; where it stands
//!   verbatim in the page as the fetch returned it, its spans are recorded, line by line; where it does not, the quote is kept
//!   and `located` says false.
//! - **the reply cites it**: the turn's reply text carries a fetched url verbatim, as a link or bare. That ties the page to the
//!   reply, not a passage: no quote, no spans.
//!
//! A fetch is the tape's own rule (O-I desktop/cradle/src/agent/tape/model.ts): an ACP tool call of kind `fetch`, or a tool named
//! `fetch` or `web_fetch` (Pi RPC); its url is the call's own input (`rawInput.url`, `args.url`); its page is the call's output
//! text (a string, a content list's text parts joined by newlines). A page cited by no fetch call ties to nothing and is not
//! recorded. The records are journal events like the owner's other receipts, so every journal reader sees them; `read` lists
//! them for one session.
use aikit_core::{ResourceRef, Result};
use aikit_store::{encounter::EncounterStore, AikitHome};
use serde_json::{json, Map, Value};

pub const USED_KIND: &str = "source-passage-used";
/// A turn whose citations could not be read: the reason, so an absent record is never silent.
pub const UNRECORDED_KIND: &str = "source-passage-used-unrecorded";
pub const RECORD_SCHEMA: &str = "aikit.encounter-source-used/v1";
pub const READING_SCHEMA: &str = "aikit.encounter-used-reading/v1";
/// The harness events that feed the records, named in every reading.
pub const FED_BY: [&str; 2] = [
    "ACP session/update agent_message_chunk whose content is a resource (uri and text) or a resource_link (uri), the uri a fetch call's url",
    "the turn's reply text carrying a fetch call's url verbatim",
];

/// Whether a turn's reply may cite a page, watched as its signals arrive so a turn that cites nothing costs no journal read at
/// its end: a reply chunk naming a resource, or reply text that carries "http" (watched across chunk boundaries).
#[derive(Debug, Default)]
pub struct Watch {
    cited: bool,
    tail: String,
}

impl Watch {
    /// Reply text as it arrives.
    pub fn text(&mut self, text: &str) {
        let joined = format!("{}{text}", self.tail);
        if joined.contains("http") {
            self.cited = true;
        }
        let keep = joined.char_indices().rev().nth(3).map_or(0, |(at, _)| at);
        self.tail = joined[keep..].to_owned();
    }
    /// A reply chunk the provider sent whole (agent_connection.rs keeps a resource or resource link so).
    pub fn update(&mut self, session_update: &str, payload: &Value) {
        if session_update == "agent_message_chunk"
            && matches!(
                payload.pointer("/content/type").and_then(Value::as_str),
                Some("resource" | "resource_link")
            )
        {
            self.cited = true;
        }
    }
    /// At the turn's end: whether it may cite a page; the watch starts again for the next turn.
    pub fn take(&mut self) -> bool {
        std::mem::take(self).cited
    }
}

fn str_at<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

/// A call's output as text (O-I railBindings.ts `outputLines`): a string as it is, a list's text parts joined by newlines, an
/// object's `text`, or its `content` read the same way.
pub fn output_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .map(output_text)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(map) => match (map.get("text"), map.get("content")) {
            (Some(Value::String(text)), _) => text.clone(),
            (_, Some(content)) => output_text(content),
            _ => String::new(),
        },
        _ => String::new(),
    }
}

#[derive(Debug)]
struct Fetch {
    id: Option<String>,
    url: Option<String>,
    first_cursor: u64,
    page: Option<String>,
}

/// One tool signal's facts, from the two shapes the journal carries (ACP tool_call / tool_call_update; Pi RPC
/// tool_execution_*), as the tape reads them: the call's id, whether it opens the call, whether it is a fetch, its url, its output.
struct CallFacts {
    id: Option<String>,
    start: bool,
    fetch: bool,
    url: Option<String>,
    output: Option<Value>,
}

fn is_fetch_name(name: &str) -> bool {
    name.eq_ignore_ascii_case("fetch") || name.eq_ignore_ascii_case("web_fetch")
}

fn call_facts(signal_kind: &str, payload: &Value) -> CallFacts {
    let id = str_at(payload, "toolCallId")
        .or_else(|| str_at(payload, "tool_call_id"))
        .or_else(|| str_at(payload, "id"))
        .map(ToOwned::to_owned);
    let pi_type = str_at(payload, "type");
    if pi_type.is_some_and(|kind| kind.starts_with("tool_execution"))
        || str_at(payload, "toolName").is_some()
    {
        let ended = signal_kind == "tool-result" || pi_type == Some("tool_execution_end");
        return CallFacts {
            id,
            start: pi_type == Some("tool_execution_start"),
            fetch: str_at(payload, "toolName").is_some_and(is_fetch_name),
            url: payload
                .pointer("/args/url")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            output: if ended {
                payload.get("result").cloned()
            } else {
                None
            },
        };
    }
    let update = str_at(payload, "sessionUpdate");
    let url = payload
        .pointer("/rawInput/url")
        .or_else(|| payload.pointer("/rawInput/uri"))
        .and_then(Value::as_str)
        .or_else(|| {
            payload
                .pointer("/locations/0/path")
                .and_then(Value::as_str)
                .filter(|path| path.starts_with("http://") || path.starts_with("https://"))
        })
        .map(ToOwned::to_owned);
    let output = if update == Some("tool_call_update") || signal_kind == "tool-result" {
        payload
            .get("content")
            .filter(|content| !content.is_null())
            .or_else(|| payload.get("rawOutput"))
            .cloned()
    } else {
        None
    };
    CallFacts {
        id,
        start: update == Some("tool_call") || (update.is_none() && signal_kind == "tool-call"),
        fetch: str_at(payload, "kind") == Some("fetch"),
        url,
        output,
    }
}

/// A quote's places in the page, as spans `{line, from, to}` of the page split at newlines, counted in characters: every place it
/// stands verbatim, a quote across lines giving one span per line.
fn spans_of(page: &str, quote: &str) -> Vec<Value> {
    if quote.is_empty() {
        return Vec::new();
    }
    let mut starts = Vec::new();
    let mut offset = 0;
    for line in page.split('\n') {
        starts.push(offset);
        offset += line.len() + 1;
    }
    let lines: Vec<&str> = page.split('\n').collect();
    let mut spans = Vec::new();
    for (start, matched) in page.match_indices(quote) {
        let end = start + matched.len();
        for (index, line) in lines.iter().enumerate() {
            let line_start = starts[index];
            let (from, to) = (start.max(line_start), end.min(line_start + line.len()));
            if to <= from {
                continue;
            }
            let (from, to) = (from - line_start, to - line_start);
            spans.push(json!({
                "line": index,
                "from": line[..from].chars().count(),
                "to": line[..to].chars().count(),
            }));
        }
    }
    spans
}

/// Whether `url` stands at `at` in `text` as a whole url: no letter or digit before it, and after it the text's end, a space, a
/// closing bracket or quote, or a sentence's punctuation followed by one of those.
fn whole_url_at(text: &str, at: usize, url: &str) -> bool {
    if text[..at]
        .chars()
        .next_back()
        .is_some_and(char::is_alphanumeric)
    {
        return false;
    }
    let closing =
        |c: char| c.is_whitespace() || matches!(c, ')' | ']' | '>' | '"' | '\'' | '`' | '*' | '_');
    let mut rest = text[at + url.len()..].chars();
    match rest.next() {
        None => true,
        Some(c) if closing(c) => true,
        Some('.' | ',' | ';' | ':' | '!' | '?') => rest.next().is_none_or(closing),
        Some(_) => false,
    }
}

/// The used records one finished turn supports: `events` is the session's journal in cursor order, through the turn's end. A
/// citation counts only when it stands in the last turn (after the turn before it ended); the fetch it cites may be earlier.
pub fn derive(agent_session: &ResourceRef, events: &[(u64, Value)]) -> Vec<Value> {
    let ended: Vec<usize> = events
        .iter()
        .enumerate()
        .filter(|(_, (_, event))| event.pointer("/event/TurnEnded").is_some())
        .map(|(index, _)| index)
        .collect();
    let turn_from = match ended.as_slice() {
        [.., before, _] => events[*before].0,
        _ => 0,
    };
    let mut fetches: Vec<Fetch> = Vec::new();
    let mut harness: Vec<(u64, Value)> = Vec::new();
    let mut reply = String::new();
    let mut reply_cursors: Vec<(usize, u64)> = Vec::new();
    for (cursor, event) in events {
        if event["kind"] != "provider" {
            continue;
        }
        let Some(kind) = event.pointer("/event/Signal/kind") else {
            continue;
        };
        let signal = str_at(kind, "kind").unwrap_or_default();
        match signal {
            "tool-call" | "tool-result" => {
                let facts = call_facts(signal, kind.get("payload").unwrap_or(&Value::Null));
                let known = facts.id.as_deref().and_then(|id| {
                    fetches
                        .iter()
                        .rposition(|fetch| fetch.id.as_deref() == Some(id))
                });
                match known {
                    Some(index) if !facts.start => {
                        let fetch = &mut fetches[index];
                        if fetch.url.is_none() {
                            fetch.url = facts.url;
                        }
                        if let Some(output) = facts.output {
                            fetch.page = Some(output_text(&output));
                        }
                    }
                    _ if facts.fetch => fetches.push(Fetch {
                        id: facts.id,
                        url: facts.url,
                        first_cursor: *cursor,
                        page: facts.output.as_ref().map(output_text),
                    }),
                    _ => {}
                }
            }
            "agent-message-chunk" | "agent-message-segment" if *cursor > turn_from => {
                reply.push_str(str_at(kind, "text").unwrap_or_default());
                reply_cursors.push((reply.len(), *cursor));
            }
            "provider-update"
                if *cursor > turn_from
                    && str_at(kind, "session_update") == Some("agent_message_chunk") =>
            {
                if let Some(content) = kind.pointer("/payload/content") {
                    harness.push((*cursor, content.clone()));
                }
            }
            _ => {}
        }
    }
    let fetched = |url: &str, before: u64| {
        fetches
            .iter()
            .rev()
            .find(|fetch| fetch.url.as_deref() == Some(url) && fetch.first_cursor < before)
    };
    let record = |fetch: &Fetch, cited_by: &str, citation: &str, citing: u64| {
        let mut record = Map::new();
        record.insert("kind".into(), json!(USED_KIND));
        record.insert("schema".into(), json!(RECORD_SCHEMA));
        record.insert("agent_session".into(), json!(agent_session));
        record.insert("tool_call_id".into(), json!(fetch.id));
        record.insert("url".into(), json!(fetch.url));
        record.insert("fetch_cursor".into(), json!(fetch.first_cursor));
        record.insert("cited_by".into(), json!(cited_by));
        record.insert("citation".into(), json!(citation));
        record.insert("citing_cursor".into(), json!(citing));
        record
    };
    let mut records: Vec<Value> = Vec::new();
    let mut cited_calls: Vec<u64> = Vec::new();
    for (cursor, content) in &harness {
        let (form, uri, quote) = match str_at(content, "type") {
            Some("resource") => (
                "resource",
                content.pointer("/resource/uri").and_then(Value::as_str),
                content.pointer("/resource/text").and_then(Value::as_str),
            ),
            Some("resource_link") => ("resource-link", str_at(content, "uri"), None),
            _ => continue,
        };
        let Some(fetch) = uri.and_then(|uri| fetched(uri, *cursor)) else {
            continue;
        };
        let mut used = record(fetch, "harness", form, *cursor);
        if let Some(quote) = quote.filter(|quote| !quote.trim().is_empty()) {
            let spans = fetch
                .page
                .as_deref()
                .map(|page| spans_of(page, quote))
                .unwrap_or_default();
            used.insert("located".into(), json!(!spans.is_empty()));
            used.insert("quote".into(), json!(quote));
            used.insert("spans".into(), json!(spans));
        }
        cited_calls.push(fetch.first_cursor);
        records.push(Value::Object(used));
    }
    // The reply's own links: each fetched url once, tied to the latest fetch of it the reply mentions after, where the reply
    // carries it whole.
    let mut seen: Vec<&str> = Vec::new();
    for fetch in fetches.iter().rev() {
        let Some(url) = fetch.url.as_deref() else {
            continue;
        };
        if seen.contains(&url) || cited_calls.contains(&fetch.first_cursor) {
            continue;
        }
        let citing = reply
            .match_indices(url)
            .filter(|(at, _)| whole_url_at(&reply, *at, url))
            .filter_map(|(at, _)| {
                reply_cursors
                    .iter()
                    .find(|(upto, _)| *upto >= at + url.len())
                    .map(|(_, cursor)| *cursor)
            })
            .find(|citing| *citing > fetch.first_cursor);
        if let Some(citing) = citing {
            seen.push(url);
            records.push(Value::Object(record(fetch, "reply", "link", citing)));
        }
    }
    records.sort_by_key(|record| record["citing_cursor"].as_u64().unwrap_or_default());
    records
}

/// The session's whole journal, in cursor order.
fn journal(store: &EncounterStore, session: &ResourceRef) -> Result<Vec<(u64, Value)>> {
    let mut after = 0;
    let mut out = Vec::new();
    loop {
        let page = store.events(session, after, 256)?;
        out.extend(
            page.events
                .into_iter()
                .map(|event| (event.cursor, event.event)),
        );
        if !page.more {
            return Ok(out);
        }
        after = page.next_cursor;
    }
}

/// At a turn's end: append the used records its reply supports. A failure to read the journal is recorded as such, so the
/// absence of a record is never silent; it never fails the turn's own append.
pub fn record_turn(store: &EncounterStore, session: &ResourceRef) -> Result<usize> {
    match journal(store, session) {
        Ok(events) => {
            let records = derive(session, &events);
            for record in &records {
                store.append(session, record)?;
            }
            Ok(records.len())
        }
        Err(error) => {
            store.append(
                session,
                &json!({"kind": UNRECORDED_KIND, "agent_session": session, "reason": error.to_string()}),
            )?;
            Ok(0)
        }
    }
}

/// `aikit session-space encounter-used`: the used records of one session, each with its journal cursor, and the turns whose
/// citations could not be read.
pub fn read(home: &AikitHome, session: &ResourceRef) -> Result<Value> {
    let store = EncounterStore::open(home)?;
    let events = journal(&store, session)?;
    let pick = |kind: &str| -> Vec<Value> {
        events
            .iter()
            .filter(|(_, event)| event["kind"] == kind)
            .map(|(cursor, event)| {
                let mut event = event.clone();
                event["cursor"] = json!(cursor);
                event
            })
            .collect()
    };
    Ok(json!({
        "schema": READING_SCHEMA,
        "agent_session": session,
        "used": pick(USED_KIND),
        "unrecorded": pick(UNRECORDED_KIND),
        "fed_by": FED_BY,
        "standing": "recorded where the harness or the reply cites a fetched page; never inferred from the reply's words",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> ResourceRef {
        ResourceRef::parse("agent-session/reader").unwrap()
    }
    fn provider(cursor: u64, kind: Value) -> (u64, Value) {
        (
            cursor,
            json!({"kind": "provider", "event": {"Signal": {"sequence": cursor, "kind": kind, "provenance": []}}}),
        )
    }
    fn turn_ended(cursor: u64) -> (u64, Value) {
        (
            cursor,
            json!({"kind": "provider", "event": {"TurnEnded": {"signals": 1}}}),
        )
    }
    fn user(cursor: u64) -> (u64, Value) {
        (
            cursor,
            json!({"kind": "user-message", "text": "read the page"}),
        )
    }
    fn chunk(cursor: u64, text: &str) -> (u64, Value) {
        provider(cursor, json!({"kind": "agent-message-chunk", "text": text}))
    }
    fn cites(cursor: u64, content: Value) -> (u64, Value) {
        provider(
            cursor,
            json!({"kind": "provider-update", "session_update": "agent_message_chunk",
                   "payload": {"sessionUpdate": "agent_message_chunk", "content": content}}),
        )
    }
    /// An ACP fetch: its call, then its output.
    fn acp_fetch(cursor: u64, id: &str, url: &str, page: &str) -> Vec<(u64, Value)> {
        vec![
            provider(
                cursor,
                json!({"kind": "tool-call", "payload": {"sessionUpdate": "tool_call", "toolCallId": id, "kind": "fetch",
                       "title": format!("Fetch {url}"), "rawInput": {"url": url}, "status": "pending"}}),
            ),
            provider(
                cursor + 1,
                json!({"kind": "tool-call", "payload": {"sessionUpdate": "tool_call_update", "toolCallId": id, "status": "completed",
                       "content": [{"type": "content", "content": {"type": "text", "text": page}}]}}),
            ),
        ]
    }
    const PAGE: &str = "Max for Live devices\n\nLFO and Shaper are modulators: each maps one\nparameter at a time.";

    #[test]
    fn a_harness_citation_with_the_text_it_took_records_the_passage_where_it_stands_on_the_page() {
        let mut events = vec![user(1)];
        events.extend(acp_fetch(2, "call-1", "https://example.org/m4l", PAGE));
        events.push(chunk(4, "They map one parameter at a time."));
        events.push(cites(
            5,
            json!({"type": "resource", "resource": {"uri": "https://example.org/m4l",
            "text": "each maps one\nparameter at a time"}}),
        ));
        events.push(turn_ended(6));
        let records = derive(&session(), &events);
        assert_eq!(records.len(), 1, "{records:?}");
        let used = &records[0];
        assert_eq!(used["kind"], USED_KIND);
        assert_eq!(used["schema"], RECORD_SCHEMA);
        assert_eq!(used["agent_session"], "agent-session/reader");
        assert_eq!(used["tool_call_id"], "call-1");
        assert_eq!(used["url"], "https://example.org/m4l");
        assert_eq!(used["fetch_cursor"], 2);
        assert_eq!(used["cited_by"], "harness");
        assert_eq!(used["citation"], "resource");
        assert_eq!(used["citing_cursor"], 5);
        assert_eq!(used["located"], true);
        assert_eq!(
            used["spans"],
            json!([{"line": 2, "from": 31, "to": 44}, {"line": 3, "from": 0, "to": 19}]),
            "a quote across two lines is one span on each"
        );
    }

    #[test]
    fn a_link_names_the_page_alone_and_a_quote_the_page_does_not_hold_is_kept_unlocated() {
        let mut events = vec![user(1)];
        events.extend(acp_fetch(2, "call-1", "https://example.org/m4l", PAGE));
        events.extend(acp_fetch(
            4,
            "call-2",
            "https://example.org/other",
            "Other page",
        ));
        events.push(cites(
            6,
            json!({"type": "resource_link", "uri": "https://example.org/m4l", "name": "m4l"}),
        ));
        events.push(cites(7, json!({"type": "resource", "resource": {"uri": "https://example.org/other", "text": "words it never said"}})));
        // A page no fetch call read ties to nothing.
        events.push(cites(
            8,
            json!({"type": "resource_link", "uri": "https://example.org/unread"}),
        ));
        events.push(turn_ended(9));
        let records = derive(&session(), &events);
        assert_eq!(records.len(), 2, "{records:?}");
        assert_eq!(records[0]["citation"], "resource-link");
        assert_eq!(records[0]["tool_call_id"], "call-1");
        assert!(records[0].get("quote").is_none() && records[0].get("spans").is_none());
        assert_eq!(records[1]["tool_call_id"], "call-2");
        assert_eq!(records[1]["quote"], "words it never said");
        assert_eq!(records[1]["located"], false);
        assert_eq!(records[1]["spans"], json!([]));
    }

    #[test]
    fn the_reply_cites_a_page_only_by_carrying_its_url_whole_never_by_sharing_its_words() {
        let mut events = vec![user(1)];
        events.extend(acp_fetch(2, "call-1", "https://example.org/a", PAGE));
        events.extend(acp_fetch(
            4,
            "call-2",
            "https://example.org/b",
            "LFO and Shaper are modulators",
        ));
        // The url split across chunks, closed by a sentence's full stop; the second page's words quoted with no url.
        events.push(chunk(6, "Per [the manual](https://exam"));
        events.push(chunk(
            7,
            "ple.org/a). \"LFO and Shaper are modulators\" says so too.",
        ));
        events.push(turn_ended(8));
        let records = derive(&session(), &events);
        assert_eq!(records.len(), 1, "only the url ties a page: {records:?}");
        assert_eq!(records[0]["tool_call_id"], "call-1");
        assert_eq!(records[0]["cited_by"], "reply");
        assert_eq!(records[0]["citation"], "link");
        assert_eq!(
            records[0]["citing_cursor"], 7,
            "the chunk where the url ends"
        );

        // A url that only begins the one fetched is no citation of it.
        let mut longer = vec![user(1)];
        longer.extend(acp_fetch(2, "call-1", "https://example.org/a", PAGE));
        longer.push(chunk(4, "See https://example.org/ab for more."));
        longer.push(turn_ended(5));
        assert!(derive(&session(), &longer).is_empty());
    }

    #[test]
    fn only_the_last_turn_cites_and_a_pi_fetch_counts_as_the_tape_counts_it() {
        let mut events = vec![user(1)];
        events.push(provider(
            2,
            json!({"kind": "tool-call", "payload": {"type": "tool_execution_start", "toolCallId": "pi-1", "toolName": "web_fetch",
                   "args": {"url": "https://example.org/pi"}}}),
        ));
        events.push(provider(
            3,
            json!({"kind": "tool-result", "payload": {"type": "tool_execution_end", "toolCallId": "pi-1", "toolName": "web_fetch",
                   "result": {"content": [{"type": "text", "text": "A pi page"}]}}}),
        ));
        events.push(chunk(4, "From https://example.org/pi"));
        events.push(turn_ended(5));
        let first = derive(&session(), &events);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0]["tool_call_id"], "pi-1");
        // The next turn says nothing of the page: the citation of the turn before is not recorded again.
        events.push(user(6));
        events.push(chunk(7, "Done."));
        events.push(turn_ended(8));
        assert!(derive(&session(), &events).is_empty());
    }

    #[test]
    fn the_watch_reads_a_turn_again_only_where_its_reply_may_cite() {
        let mut watch = Watch::default();
        watch.text("no link ht");
        watch.text("tps but split");
        assert!(watch.take(), "http across chunks");
        watch.text("plain words");
        assert!(!watch.take());
        watch.update(
            "agent_message_chunk",
            &json!({"content": {"type": "resource_link", "uri": "x"}}),
        );
        assert!(watch.take());
    }

    #[test]
    fn records_are_kept_in_the_journal_and_read_back_for_the_session() {
        let home_dir = tempfile::TempDir::new().unwrap();
        let home = AikitHome::at(home_dir.path().join("home"));
        let store = EncounterStore::open(&home).unwrap();
        let agent = session();
        let mut events = vec![user(0)];
        events.extend(acp_fetch(0, "call-1", "https://example.org/m4l", PAGE));
        events.push(chunk(0, "See https://example.org/m4l."));
        events.push(turn_ended(0));
        for (_, event) in &events {
            store.append(&agent, event).unwrap();
        }
        assert_eq!(record_turn(&store, &agent).unwrap(), 1);
        drop(store);
        let reading = read(&home, &agent).unwrap();
        assert_eq!(reading["schema"], READING_SCHEMA);
        let used = reading["used"].as_array().unwrap();
        assert_eq!(used.len(), 1);
        assert_eq!(used[0]["tool_call_id"], "call-1");
        assert!(used[0]["cursor"].as_u64().unwrap() > 0);
        assert_eq!(reading["fed_by"].as_array().unwrap().len(), 2);
    }
}
