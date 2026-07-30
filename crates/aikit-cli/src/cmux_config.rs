//! Lossless edits to cmux's JSON/JSONC configuration.
//!
//! Re-serialising a user's `cmux.json` would erase comments, reorder keys, and
//! normalize hand formatting. This small parser records byte spans and changes
//! only the command object AIKit owns (or the punctuation needed to add it).

use aikit_core::{AikitError, Result};

const AIKIT_NAME: &str = "AIKit";
const AIKIT_COMMAND: &str = "aikit ui";
const AIKIT_ENTRY: &str = "{ \"name\": \"AIKit\", \"description\": \"Open AIKit's unified palette and tree\", \"keywords\": [\"aikit\", \"skills\", \"capabilities\"], \"command\": \"aikit ui\", \"confirm\": false }";

#[derive(Debug, Clone)]
struct Node {
    start: usize,
    end: usize,
    kind: Kind,
}

#[derive(Debug, Clone)]
enum Kind {
    Object(Vec<Member>),
    Array(Vec<Node>),
    String(String),
    Scalar,
}

#[derive(Debug, Clone)]
struct Member {
    key: String,
    value: Node,
}

struct Parser<'a> {
    source: &'a [u8],
    cursor: usize,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source: source.as_bytes(),
            cursor: 0,
        }
    }

    fn parse(mut self) -> Result<Node> {
        self.skip_trivia()?;
        let root = self.value()?;
        self.skip_trivia()?;
        if self.cursor != self.source.len() {
            return Err(invalid("unexpected content after the root JSON value"));
        }
        Ok(root)
    }

    fn value(&mut self) -> Result<Node> {
        self.skip_trivia()?;
        let start = self.cursor;
        match self.peek() {
            Some(b'{') => self.object(start),
            Some(b'[') => self.array(start),
            Some(b'"') => {
                let value = self.string()?;
                Ok(Node {
                    start,
                    end: self.cursor,
                    kind: Kind::String(value),
                })
            }
            Some(_) => self.scalar(start),
            None => Err(invalid("expected a JSON value")),
        }
    }

    fn object(&mut self, start: usize) -> Result<Node> {
        self.expect(b'{')?;
        let mut members = Vec::new();
        loop {
            self.skip_trivia()?;
            if self.take(b'}') {
                return Ok(Node {
                    start,
                    end: self.cursor,
                    kind: Kind::Object(members),
                });
            }
            let key = self.string()?;
            self.skip_trivia()?;
            self.expect(b':')?;
            let value = self.value()?;
            members.push(Member { key, value });
            self.skip_trivia()?;
            if self.take(b',') {
                self.skip_trivia()?;
                if self.take(b'}') {
                    return Ok(Node {
                        start,
                        end: self.cursor,
                        kind: Kind::Object(members),
                    });
                }
                continue;
            }
            self.expect(b'}')?;
            return Ok(Node {
                start,
                end: self.cursor,
                kind: Kind::Object(members),
            });
        }
    }

    fn array(&mut self, start: usize) -> Result<Node> {
        self.expect(b'[')?;
        let mut values = Vec::new();
        loop {
            self.skip_trivia()?;
            if self.take(b']') {
                return Ok(Node {
                    start,
                    end: self.cursor,
                    kind: Kind::Array(values),
                });
            }
            values.push(self.value()?);
            self.skip_trivia()?;
            if self.take(b',') {
                self.skip_trivia()?;
                if self.take(b']') {
                    return Ok(Node {
                        start,
                        end: self.cursor,
                        kind: Kind::Array(values),
                    });
                }
                continue;
            }
            self.expect(b']')?;
            return Ok(Node {
                start,
                end: self.cursor,
                kind: Kind::Array(values),
            });
        }
    }

    fn scalar(&mut self, start: usize) -> Result<Node> {
        while let Some(byte) = self.peek() {
            if byte.is_ascii_whitespace() || matches!(byte, b',' | b']' | b'}') {
                break;
            }
            if byte == b'/' && matches!(self.source.get(self.cursor + 1), Some(b'/' | b'*')) {
                break;
            }
            self.cursor += 1;
        }
        if self.cursor == start {
            return Err(invalid("expected a JSON scalar"));
        }
        let raw = std::str::from_utf8(&self.source[start..self.cursor])
            .map_err(|_| invalid("cmux configuration is not UTF-8"))?;
        if !matches!(raw, "true" | "false" | "null")
            && serde_json::from_str::<serde_json::Number>(raw).is_err()
        {
            return Err(invalid(format!("invalid JSON scalar `{raw}`")));
        }
        Ok(Node {
            start,
            end: self.cursor,
            kind: Kind::Scalar,
        })
    }

    fn string(&mut self) -> Result<String> {
        let start = self.cursor;
        self.expect(b'"')?;
        let mut escaped = false;
        while let Some(byte) = self.peek() {
            self.cursor += 1;
            if escaped {
                escaped = false;
                continue;
            }
            match byte {
                b'\\' => escaped = true,
                b'"' => {
                    return serde_json::from_slice(&self.source[start..self.cursor])
                        .map_err(|error| invalid(format!("invalid JSON string: {error}")));
                }
                0x00..=0x1f => return Err(invalid("control character in JSON string")),
                _ => {}
            }
        }
        Err(invalid("unterminated JSON string"))
    }

    fn skip_trivia(&mut self) -> Result<()> {
        loop {
            while self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
                self.cursor += 1;
            }
            if self.peek() != Some(b'/') {
                return Ok(());
            }
            match self.source.get(self.cursor + 1).copied() {
                Some(b'/') => {
                    self.cursor += 2;
                    while self.peek().is_some_and(|byte| byte != b'\n') {
                        self.cursor += 1;
                    }
                }
                Some(b'*') => {
                    self.cursor += 2;
                    let mut closed = false;
                    while self.cursor + 1 < self.source.len() {
                        if self.source[self.cursor] == b'*' && self.source[self.cursor + 1] == b'/'
                        {
                            self.cursor += 2;
                            closed = true;
                            break;
                        }
                        self.cursor += 1;
                    }
                    if !closed {
                        return Err(invalid("unterminated JSON block comment"));
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    fn peek(&self) -> Option<u8> {
        self.source.get(self.cursor).copied()
    }

    fn take(&mut self, expected: u8) -> bool {
        if self.peek() == Some(expected) {
            self.cursor += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, expected: u8) -> Result<()> {
        if self.take(expected) {
            Ok(())
        } else {
            Err(invalid(format!(
                "expected `{}` at byte {}",
                expected as char, self.cursor
            )))
        }
    }
}

fn invalid(message: impl Into<String>) -> AikitError {
    AikitError::new("mux.cmux_config_invalid", message)
}

fn object_members(node: &Node) -> Result<&[Member]> {
    match &node.kind {
        Kind::Object(members) => Ok(members),
        _ => Err(invalid("cmux configuration root must be a JSON object")),
    }
}

fn unique_member<'a>(members: &'a [Member], key: &str) -> Result<Option<&'a Member>> {
    let mut found = members.iter().filter(|member| member.key == key);
    let first = found.next();
    if found.next().is_some() {
        return Err(invalid(format!(
            "cmux configuration contains duplicate `{key}` keys; their authority is ambiguous"
        )));
    }
    Ok(first)
}

fn string_member<'a>(node: &'a Node, key: &str) -> Option<&'a str> {
    let Kind::Object(members) = &node.kind else {
        return None;
    };
    members
        .iter()
        .find(|member| member.key == key)
        .and_then(|member| match &member.value.kind {
            Kind::String(value) => Some(value.as_str()),
            _ => None,
        })
}

fn validate_command_authority(entry: &Node) -> Result<()> {
    let Kind::Object(members) = &entry.kind else {
        return Ok(());
    };
    unique_member(members, "name")?;
    unique_member(members, "command")?;
    Ok(())
}

fn has_trailing_comma(source: &str, after: usize, close: usize) -> bool {
    let mut parser = Parser {
        source: source.as_bytes(),
        cursor: after,
    };
    if parser.skip_trivia().is_err() {
        return false;
    }
    parser.cursor < close && parser.take(b',')
}

fn insert_array_entry(source: &str, array: &Node) -> Result<String> {
    let Kind::Array(values) = &array.kind else {
        return Err(invalid("cmux `commands` must be an array"));
    };
    let close = array.end - 1;
    let insertion = if let Some(last) = values.last() {
        if has_trailing_comma(source, last.end, close) {
            format!("\n    {AIKIT_ENTRY},")
        } else {
            format!(",\n    {AIKIT_ENTRY}")
        }
    } else {
        format!("\n    {AIKIT_ENTRY}\n  ")
    };
    Ok(format!(
        "{}{}{}",
        &source[..close],
        insertion,
        &source[close..]
    ))
}

fn insert_commands_member(source: &str, root: &Node, members: &[Member]) -> String {
    let close = root.end - 1;
    let entry = format!("\"commands\": [\n    {AIKIT_ENTRY}\n  ]");
    let insertion = if let Some(last) = members.last() {
        if has_trailing_comma(source, last.value.end, close) {
            format!("\n  {entry},")
        } else {
            format!(",\n  {entry}")
        }
    } else {
        format!("\n  {entry}\n")
    };
    format!("{}{}{}", &source[..close], insertion, &source[close..])
}

/// Merge AIKit's command palette entry while preserving every unrelated byte.
pub fn merge_command(source: Option<&str>, replace: bool) -> Result<String> {
    let Some(source) = source else {
        return Ok(format!(
            "{{\n  \"commands\": [\n    {AIKIT_ENTRY}\n  ]\n}}\n"
        ));
    };
    let root = Parser::new(source).parse()?;
    let members = object_members(&root)?;
    let Some(commands) = unique_member(members, "commands")? else {
        return Ok(insert_commands_member(source, &root, members));
    };
    let Kind::Array(entries) = &commands.value.kind else {
        return Err(invalid("cmux `commands` must be an array"));
    };
    for entry in entries {
        validate_command_authority(entry)?;
    }
    let aikit_entries: Vec<&Node> = entries
        .iter()
        .filter(|entry| string_member(entry, "name") == Some(AIKIT_NAME))
        .collect();
    if aikit_entries.len() > 1 {
        return Err(invalid(
            "cmux configuration contains more than one command named `AIKit`",
        ));
    }
    if let Some(existing) = aikit_entries.first().copied() {
        if string_member(existing, "command") == Some(AIKIT_COMMAND) {
            return Ok(source.to_string());
        }
        if !replace {
            return Err(AikitError::new(
                "mux.key_conflict",
                "cmux already has a command named `AIKit`; AIKit did not replace it",
            )
            .with("key", AIKIT_NAME.to_string())
            .with(
                "binding",
                string_member(existing, "command")
                    .unwrap_or("<non-string command>")
                    .to_string(),
            )
            .with(
                "resolution",
                "rename the existing command or review and pass `--replace-key`".to_string(),
            ));
        }
        return Ok(format!(
            "{}{}{}",
            &source[..existing.start],
            AIKIT_ENTRY,
            &source[existing.end..]
        ));
    }
    insert_array_entry(source, &commands.value)
}

/// Verify that the installed file contains the exact executable command AIKit owns.
pub fn verify_command(source: &str) -> Result<bool> {
    let root = Parser::new(source).parse()?;
    let members = object_members(&root)?;
    let Some(commands) = unique_member(members, "commands")? else {
        return Ok(false);
    };
    let Kind::Array(entries) = &commands.value.kind else {
        return Err(invalid("cmux `commands` must be an array"));
    };
    for entry in entries {
        validate_command_authority(entry)?;
    }
    let aikit_entries: Vec<&Node> = entries
        .iter()
        .filter(|entry| string_member(entry, "name") == Some(AIKIT_NAME))
        .collect();
    if aikit_entries.len() > 1 {
        return Err(invalid(
            "cmux configuration contains more than one command named `AIKit`",
        ));
    }
    Ok(aikit_entries
        .first()
        .is_some_and(|entry| string_member(entry, "command") == Some(AIKIT_COMMAND)))
}
