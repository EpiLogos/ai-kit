//! Read-only compatibility projection for `aikit tree`.
//!
//! The V2 application relation field lives in `TuiState` and projects List / Tree /
//! Graph from one `RelationReadModel`. This module is deliberately narrower: it
//! preserves the public `aikit tree` diagnostic command as a textual projection of
//! package/catalogue state. It owns no selection, staging, mutation, activation or
//! store effects and is not used by the V2 ApplicationSurface.

use std::collections::BTreeSet;

use aikit_core::id::CapsuleId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeKind {
    Root(Root),
    Set { name: String, observed: bool },
    Group { label: String },
    Capability { id: CapsuleId },
    HookStep {
        capsule: CapsuleId,
        phase: String,
        position: usize,
    },
    Entry { label: String, detail: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Root {
    Sets,
    Kinds,
    Hooks,
    Contexts,
    Registries,
    Inbox,
}

impl Root {
    pub const ALL: [Root; 6] = [
        Root::Sets,
        Root::Kinds,
        Root::Hooks,
        Root::Contexts,
        Root::Registries,
        Root::Inbox,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Root::Sets => "sets",
            Root::Kinds => "kinds",
            Root::Hooks => "hooks",
            Root::Contexts => "contexts",
            Root::Registries => "registries",
            Root::Inbox => "inbox",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub kind: NodeKind,
    pub depth: usize,
    pub summary: String,
    pub expandable: bool,
    pub children: Vec<Node>,
}

impl Node {
    pub fn leaf(kind: NodeKind, summary: impl Into<String>) -> Self {
        Self {
            kind,
            depth: 0,
            summary: summary.into(),
            expandable: false,
            children: Vec::new(),
        }
    }

    pub fn branch(kind: NodeKind, summary: impl Into<String>, children: Vec<Node>) -> Self {
        Self {
            kind,
            depth: 0,
            summary: summary.into(),
            expandable: true,
            children,
        }
    }

    pub fn path(&self, parent: &str) -> String {
        let own = match &self.kind {
            NodeKind::Root(root) => root.as_str().to_string(),
            NodeKind::Set { name, .. } => name.clone(),
            NodeKind::Group { label } => label.clone(),
            NodeKind::Capability { id } => id.to_string(),
            NodeKind::HookStep { capsule, .. } => capsule.to_string(),
            NodeKind::Entry { label, .. } => label.clone(),
        };
        if parent.is_empty() {
            own
        } else {
            format!("{parent}/{own}")
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub path: String,
    pub node: Node,
    pub depth: usize,
    pub expanded: bool,
}

/// Read-only formatting state for the public diagnostic command.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TreeState {
    pub roots: Vec<Node>,
    pub expanded: BTreeSet<String>,
    pub filter: String,
}

impl TreeState {
    pub fn new(roots: Vec<Node>) -> Self {
        Self {
            roots,
            ..Default::default()
        }
    }

    pub fn rows(&self) -> Vec<Row> {
        let mut out = Vec::new();
        for node in &self.roots {
            self.flatten(node, "", 0, &mut out);
        }
        out
    }

    fn flatten(&self, node: &Node, parent: &str, depth: usize, out: &mut Vec<Row>) {
        let path = node.path(parent);
        if !self.matches_filter(node, &path) {
            return;
        }
        let expanded = self.expanded.contains(&path);
        out.push(Row {
            path: path.clone(),
            node: node.clone(),
            depth,
            expanded,
        });
        if expanded {
            for child in &node.children {
                self.flatten(child, &path, depth + 1, out);
            }
        }
    }

    fn matches_filter(&self, node: &Node, path: &str) -> bool {
        if self.filter.is_empty() {
            return true;
        }
        let needle = self.filter.to_lowercase();
        if path.to_lowercase().contains(&needle)
            || node.summary.to_lowercase().contains(&needle)
        {
            return true;
        }
        node.children
            .iter()
            .any(|child| self.matches_filter(child, &child.path(path)))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TreeGlyphs {
    pub expanded: &'static str,
    pub collapsed: &'static str,
    pub leaf: &'static str,
    pub observed: &'static str,
}

impl TreeGlyphs {
    pub fn unicode() -> Self {
        Self {
            expanded: "▾",
            collapsed: "▸",
            leaf: " ",
            observed: "@",
        }
    }

    pub fn ascii() -> Self {
        Self {
            expanded: "-",
            collapsed: "+",
            leaf: " ",
            observed: "@",
        }
    }

    pub fn for_glyphs(glyphs: crate::layout::Glyphs) -> Self {
        if glyphs == crate::layout::Glyphs::ascii() {
            Self::ascii()
        } else {
            Self::unicode()
        }
    }
}

pub fn render_lines(state: &TreeState, glyphs: TreeGlyphs) -> Vec<String> {
    state
        .rows()
        .iter()
        .map(|row| render_row(row, glyphs))
        .collect()
}

fn render_row(row: &Row, glyphs: TreeGlyphs) -> String {
    let marker = if row.node.expandable {
        if row.expanded {
            glyphs.expanded
        } else {
            glyphs.collapsed
        }
    } else {
        glyphs.leaf
    };

    let label = match &row.node.kind {
        NodeKind::Root(root) => format!("{}/", root.as_str()),
        NodeKind::Set { name, observed } => {
            if *observed {
                format!("{}{name}/", glyphs.observed)
            } else {
                format!("{name}/")
            }
        }
        NodeKind::Group { label } => format!("{label}/"),
        NodeKind::Capability { id } => id.to_string(),
        NodeKind::HookStep {
            capsule,
            phase,
            position,
        } => format!("{position}. {phase}/{}", capsule.leaf()),
        NodeKind::Entry { label, detail } => format!("{label}  {detail}"),
    };

    let line = format!(
        "{indent}{marker} {label}  {summary}",
        indent = "  ".repeat(row.depth),
        summary = row.node.summary,
    )
    .trim_end()
    .to_string();

    if glyphs.expanded == TreeGlyphs::ascii().expanded {
        ascii_fold(&line)
    } else {
        line
    }
}

pub fn ascii_fold(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '▾' | '▿' | '▼' => "-".to_string(),
            '▸' | '▹' | '▶' => "+".to_string(),
            '·' | '•' => "-".to_string(),
            '…' => "...".to_string(),
            '—' | '–' => "--".to_string(),
            '“' | '”' | '„' => "\"".to_string(),
            '‘' | '’' => "'".to_string(),
            '→' => "->".to_string(),
            '←' => "<-".to_string(),
            '✓' => "y".to_string(),
            '✗' | '×' => "x".to_string(),
            '⚠' => "!".to_string(),
            c if c.is_ascii() => c.to_string(),
            _ => "?".to_string(),
        })
        .collect()
}
