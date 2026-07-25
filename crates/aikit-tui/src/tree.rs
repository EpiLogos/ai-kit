//! The tree: a virtual filesystem you already know how to use.
//!
//! The palette is for **invoking and toggling**; it opens, acts and disappears.
//! The tree is for **organising**; you enter it deliberately and leave when done.
//! Neither is a permanent control centre — the tree is where you *arrange* things
//! so the palette can stay one line long.
//!
//! ## It is a view, not an ownership hierarchy
//!
//! One capsule appears under `kinds/`, under every set containing it, and under
//! `registries/`. Tags-as-folders, not a filesystem you can corrupt by moving
//! something. Nothing here mutates a capsule's location, because a capsule has no
//! single location to mutate.
//!
//! ## `hooks/` shows the resolved chain in execution order
//!
//! That single screen answers the question a machine full of hook scripts cannot
//! otherwise answer — *what actually runs, in what order, when Claude edits a
//! file* — and it is the direct fix for hook scripts sitting on disk wired to
//! nothing.
//!
//! ## Why the model is pure
//!
//! Navigation, expansion, staging and the filesystem verbs are all decided here,
//! with no I/O and no rendering. That is what makes `STANDARDS.md` §5's
//! accessibility rules *testable* rather than aspirational: a test can drive the
//! same tree by keystroke and by click and assert the two reach an identical
//! state, because "state" is a value this module owns.

use std::collections::BTreeSet;

use aikit_core::id::CapsuleId;

/// What a row *is*. The tree renders rows; this is the meaning behind one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeKind {
    /// One of the six roots.
    Root(Root),
    /// A skill-set, or a nested sub-set.
    Set { name: String, observed: bool },
    /// A grouping that is not itself addressable — `skill/`, `PreToolUse/`.
    Group { label: String },
    /// A capability. The leaf that actually does something.
    Capability { id: CapsuleId },
    /// One step in a resolved hook chain, in execution order.
    HookStep {
        capsule: CapsuleId,
        phase: String,
        position: usize,
    },
    /// A context, a registry, or an inbox item: addressable, not a capability.
    Entry { label: String, detail: String },
}

/// The six roots (SPEC-III §4.1).
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

/// One row in the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub kind: NodeKind,
    /// Indent level. The root rows are 0.
    pub depth: usize,
    /// The one-line description `STANDARDS.md` §5 requires: what a screen reader
    /// gets, and what `--json` gets. Those being the same string is the point.
    pub summary: String,
    /// Whether this row has children at all.
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

    /// A stable identity for a row, used to keep the selection and the expansion
    /// set across a rebuild. Path-shaped, because the tree is path-shaped.
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

/// A row as flattened for display: the node, its indent, and its path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub path: String,
    pub node: Node,
    pub depth: usize,
    /// `true` when this row has children and they are currently shown.
    pub expanded: bool,
}

/// Everything the tree needs to decide what to draw and what a key does.
///
/// Deliberately a plain value: two `TreeState`s that compare equal *are* the same
/// state, which is what lets a test assert that a keyboard path and a mouse path
/// arrive at the same place.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TreeState {
    pub roots: Vec<Node>,
    /// Paths whose children are shown.
    pub expanded: BTreeSet<String>,
    /// Index into [`TreeState::rows`].
    pub selected: usize,
    /// Capabilities with a staged activation toggle, by path.
    pub staged: BTreeSet<CapsuleId>,
    /// Yanked capability, awaiting a `p`ut into a set.
    pub yanked: Option<CapsuleId>,
    /// The `/` filter. Empty means no filter.
    pub filter: String,
}

impl TreeState {
    pub fn new(roots: Vec<Node>) -> Self {
        Self {
            roots,
            ..Default::default()
        }
    }

    /// The visible rows, flattened depth-first with the expansion set applied.
    ///
    /// Recomputed rather than cached: the tree is small (a screen's worth matters,
    /// not the whole catalogue), and a cache that could disagree with the
    /// expansion set is a bug waiting for a resize.
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

    /// A filter keeps a row when it matches, or when any descendant does — so
    /// filtering never hides the path to a match.
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
            .any(|c| self.matches_filter(c, &c.path(path)))
    }

    pub fn selected_row(&self) -> Option<Row> {
        self.rows().into_iter().nth(self.selected)
    }

    /// The one-line description of the selection: the status bar, `--json`, and a
    /// screen reader all get this exact string.
    pub fn describe_selection(&self) -> String {
        self.selected_row()
            .map(|row| format!("{} — {}", row.path, row.node.summary))
            .unwrap_or_else(|| "nothing selected".to_string())
    }
}

/// What the tree can be asked to do.
///
/// **Every one of these is reachable by keyboard and by mouse.** That is not a
/// convention: `event.rs` maps keys to these and mouse events to these *same*
/// values, so there is no way for one input path to reach a capability the other
/// cannot. A test drives both and compares the resulting `TreeState`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeAction {
    /// `j` / `↓` / scroll-down / click-below.
    Down,
    /// `k` / `↑` / scroll-up.
    Up,
    /// `gg` / `Home`.
    First,
    /// `G` / `End`.
    Last,
    /// `Ctrl-d` / `PgDn`.
    PageDown,
    /// `Ctrl-u` / `PgUp`.
    PageUp,
    /// `l` / `→` / double-click / click on the expand marker.
    Expand,
    /// `h` / `←` / double-click an expanded row.
    Collapse,
    /// `Enter` — expand a branch, act on a leaf.
    Activate,
    /// Select a specific row: what a click resolves to.
    Select(usize),
    /// `Space` — stage an activation toggle.
    Stage,
    /// `y` — yank a capability.
    Yank,
    /// `p` — put the yanked capability into the selected set. Copy, not move:
    /// sets are views.
    Put,
    /// `d` — remove from this set. Never deletes the capability.
    RemoveFromSet,
    /// `/` — set the filter.
    Filter(String),
    /// Clear the filter.
    ClearFilter,
}

/// What a completed action asks the host to do, when it needs I/O.
///
/// The reducer is pure; anything that has to touch the store leaves as one of
/// these rather than being done inline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeEffect {
    /// Create a new writable set.
    CreateSet { set: String },
    /// Rename a writable set.
    RenameSet { from: String, to: String },
    /// Recoverably delete a writable set after confirmation.
    DeleteSet { set: String },
    /// Add `capsule` to the set at `set`.
    AddToSet { set: String, capsule: CapsuleId },
    /// Remove `capsule` from the set at `set`.
    RemoveFromSet { set: String, capsule: CapsuleId },
    /// Act on a leaf: run a script, open a skill.
    Activate { capsule: CapsuleId },
}

/// Apply an action. Pure: state in, state and effects out.
pub fn reduce(state: &mut TreeState, action: TreeAction) -> Vec<TreeEffect> {
    let rows = state.rows();
    let count = rows.len();
    let page = 10usize;

    match action {
        TreeAction::Down => {
            if count > 0 {
                state.selected = (state.selected + 1).min(count - 1);
            }
        }
        TreeAction::Up => state.selected = state.selected.saturating_sub(1),
        TreeAction::First => state.selected = 0,
        TreeAction::Last => state.selected = count.saturating_sub(1),
        TreeAction::PageDown => {
            if count > 0 {
                state.selected = (state.selected + page).min(count - 1);
            }
        }
        TreeAction::PageUp => state.selected = state.selected.saturating_sub(page),
        TreeAction::Select(index) => {
            if count > 0 {
                state.selected = index.min(count - 1);
            }
        }
        TreeAction::Expand => {
            if let Some(row) = rows.get(state.selected) {
                if row.node.expandable {
                    state.expanded.insert(row.path.clone());
                }
            }
        }
        TreeAction::Collapse => {
            if let Some(row) = rows.get(state.selected) {
                // Collapsing an already-collapsed row moves to its parent, which
                // is what every tree in every editor does and what a user's hands
                // already expect.
                if !state.expanded.remove(&row.path) {
                    if let Some(parent) = row.path.rsplit_once('/').map(|(p, _)| p.to_string()) {
                        if let Some(index) = rows.iter().position(|r| r.path == parent) {
                            state.selected = index;
                        }
                    }
                }
            }
        }
        TreeAction::Activate => {
            if let Some(row) = rows.get(state.selected) {
                if row.node.expandable {
                    if state.expanded.contains(&row.path) {
                        state.expanded.remove(&row.path);
                    } else {
                        state.expanded.insert(row.path.clone());
                    }
                } else if let NodeKind::Capability { id } = &row.node.kind {
                    return vec![TreeEffect::Activate { capsule: id.clone() }];
                }
            }
        }
        TreeAction::Stage => {
            if let Some(row) = rows.get(state.selected) {
                if let NodeKind::Capability { id } = &row.node.kind {
                    if !state.staged.remove(id) {
                        state.staged.insert(id.clone());
                    }
                }
            }
        }
        TreeAction::Yank => {
            if let Some(row) = rows.get(state.selected) {
                if let NodeKind::Capability { id } = &row.node.kind {
                    state.yanked = Some(id.clone());
                }
            }
        }
        TreeAction::Put => {
            if let (Some(row), Some(capsule)) = (rows.get(state.selected), state.yanked.clone()) {
                if let Some(set) = enclosing_set(&rows, state.selected, row) {
                    return vec![TreeEffect::AddToSet { set, capsule }];
                }
            }
        }
        TreeAction::RemoveFromSet => {
            if let Some(row) = rows.get(state.selected) {
                if let NodeKind::Capability { id } = &row.node.kind {
                    if let Some(set) = enclosing_set(&rows, state.selected, row) {
                        return vec![TreeEffect::RemoveFromSet {
                            set,
                            capsule: id.clone(),
                        }];
                    }
                }
            }
        }
        TreeAction::Filter(text) => {
            state.filter = text;
            state.selected = 0;
        }
        TreeAction::ClearFilter => {
            state.filter.clear();
            state.selected = 0;
        }
    }
    Vec::new()
}

/// The set a row sits in: itself if it is a set, else the nearest set above it.
///
/// Walking *up the visible rows* rather than parsing the path means a capability
/// listed under `kinds/` has no enclosing set and `p` there is correctly a no-op,
/// instead of inventing a set name out of a path segment.
fn enclosing_set(rows: &[Row], index: usize, row: &Row) -> Option<String> {
    if let NodeKind::Set { name, .. } = &row.node.kind {
        return Some(name.clone());
    }
    rows[..=index].iter().rev().find_map(|candidate| {
        let is_ancestor = row.path.starts_with(&format!("{}/", candidate.path));
        match &candidate.node.kind {
            NodeKind::Set { name, .. } if is_ancestor => Some(name.clone()),
            _ => None,
        }
    })
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// The marks a tree row needs, in both glyph sets.
///
/// Two complete sets rather than a per-glyph fallback, matching `layout::Glyphs`:
/// a mixed rendering, where three marks are Unicode and one is ASCII because
/// somebody forgot, is how a fallback silently rots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TreeGlyphs {
    pub expanded: &'static str,
    pub collapsed: &'static str,
    pub leaf: &'static str,
    pub staged: &'static str,
    pub unstaged: &'static str,
    pub observed: &'static str,
}

impl TreeGlyphs {
    pub fn unicode() -> Self {
        Self {
            expanded: "▾",
            collapsed: "▸",
            leaf: " ",
            staged: "[x]",
            unstaged: "[ ]",
            observed: "@",
        }
    }

    /// ASCII carries the *same information*: `+`/`-` for expansion state and
    /// `[x]`/`[ ]` for staging, per SPEC-III §4.3. Nothing is dropped, so no
    /// Unicode is ever load-bearing.
    pub fn ascii() -> Self {
        Self {
            expanded: "-",
            collapsed: "+",
            leaf: " ",
            staged: "[x]",
            unstaged: "[ ]",
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

/// Render the tree as lines of text.
///
/// Returns plain strings rather than styled spans so the *information* can be
/// snapshot-tested independently of colour — colour is redundant emphasis here,
/// never the only carrier of meaning.
pub fn render_lines(state: &TreeState, glyphs: TreeGlyphs) -> Vec<String> {
    state
        .rows()
        .iter()
        .map(|row| render_row(state, row, glyphs))
        .collect()
}

fn render_row(state: &TreeState, row: &Row, glyphs: TreeGlyphs) -> String {
    let marker = if row.node.expandable {
        if row.expanded {
            glyphs.expanded
        } else {
            glyphs.collapsed
        }
    } else {
        glyphs.leaf
    };

    // A staged capability carries its box on every row it appears on, because the
    // staging is a property of the capability, not of the row you staged it from.
    let stage = match &row.node.kind {
        NodeKind::Capability { id } => {
            if state.staged.contains(id) {
                format!("{} ", glyphs.staged)
            } else {
                format!("{} ", glyphs.unstaged)
            }
        }
        _ => String::new(),
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
        "{indent}{marker} {stage}{label}  {summary}",
        indent = "  ".repeat(row.depth),
        summary = row.node.summary,
    );
    let line = line.trim_end().to_string();

    // In ASCII mode the *text* has to be ASCII too, not only the marks. Summaries
    // and names come from live data — a set's projection summary carries `·`, and
    // a real machine has skills named `paśyantī` — so a fallback that only swapped
    // the glyphs would still emit mojibake to the terminal that asked for ASCII.
    if glyphs.expanded == TreeGlyphs::ascii().expanded {
        ascii_fold(&line)
    } else {
        line
    }
}

/// Fold a string to pure ASCII for the fallback rendering.
///
/// Common typography is transliterated to the obvious equivalent; anything else
/// non-ASCII becomes `?`. Marking a character that cannot be represented is the
/// honest option — silently dropping it would change a name, and silently emitting
/// it would defeat the whole point of the fallback.
pub fn ascii_fold(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            // The tree's own marks, so folding a Unicode line yields exactly the
            // ASCII line: ▾ expanded is `-`, ▸ collapsed is `+`.
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
            // A name AIKit cannot render in ASCII is marked, never quietly altered.
            _ => "?".to_string(),
        })
        .collect()
}
