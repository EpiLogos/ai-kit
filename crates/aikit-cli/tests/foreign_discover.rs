//! `aikit init` discovers the foreign skill roots already on the machine,
//! read-only, and counts the problems a user cannot otherwise see. Every
//! assertion is against a real directory tree with real symlinks, because the
//! dead-symlink and two-level-layout behaviours are properties of the filesystem.

use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;

use aikit_cli::foreign::{self, ForeignRoot};

fn skill(dir: &Path, name: &str, frontmatter_ok: bool) {
    let root = dir.join(name);
    fs::create_dir_all(&root).unwrap();
    let body = if frontmatter_ok {
        format!("---\nname: {name}\ndescription: A real skill named {name}.\n---\n\n# {name}\n")
    } else {
        // Looks like a skill (has SKILL.md) but the frontmatter is unusable.
        "# no frontmatter here\n".to_string()
    };
    fs::write(root.join("SKILL.md"), body).unwrap();
}

#[test]
fn discovery_counts_skills_dead_symlinks_and_missing_frontmatter() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();

    // A flat Claude-style root: two real skills, one hand-made skill with broken
    // frontmatter, one symlink into a sibling tree, and one dead symlink.
    let claude = home.join(".claude/skills");
    fs::create_dir_all(&claude).unwrap();
    skill(&claude, "pdf", true);
    skill(&claude, "docx", true);
    skill(&claude, "half-made", false);

    // A real skill living elsewhere, linked into the Claude root (valid link).
    let agents = home.join(".agents/skills");
    fs::create_dir_all(&agents).unwrap();
    skill(&agents, "shared-skill", true);
    symlink(agents.join("shared-skill"), claude.join("shared-skill")).unwrap();

    // A dead symlink: its target does not exist.
    symlink(home.join("gone/nowhere"), claude.join("broken-link")).unwrap();

    // A Hermes-style TWO-LEVEL layout: category/<skill>.
    let hermes = home.join(".hermes/skills");
    fs::create_dir_all(hermes.join("nara")).unwrap();
    skill(&hermes.join("nara"), "para", true);
    skill(&hermes.join("nara"), "pasyanti", true);

    let roots = foreign::default_roots(home);
    let found = foreign::discover(&roots);

    let by_label = |label: &str| -> ForeignRoot {
        found
            .iter()
            .find(|r| r.label == label)
            .unwrap_or_else(|| panic!("expected a {label} root; got {found:?}"))
            .clone()
    };

    let claude_root = by_label("@claude");
    assert_eq!(
        claude_root.skills, 3,
        "pdf, docx and the linked shared-skill are real skills"
    );
    assert_eq!(claude_root.dead_symlinks, 1, "broken-link is a dead symlink");
    assert_eq!(
        claude_root.missing_frontmatter, 1,
        "half-made has no usable frontmatter"
    );
    assert_eq!(claude_root.problems(), 2);

    let hermes_root = by_label("@hermes");
    assert_eq!(
        hermes_root.skills, 2,
        "the two-level nara/ category must be indexed, not missed"
    );

    // A root that does not exist on this machine is simply absent, not an error.
    assert!(
        found.iter().all(|r| r.label != "@codex"),
        "an absent root is skipped, never reported"
    );
}
