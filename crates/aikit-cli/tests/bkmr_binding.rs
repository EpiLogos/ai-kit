//! The bkmr project binding replaces `.current`.
//!
//! `docs/integrations/bkmr.md` §2 documents the failure this fixes: a single
//! global mutable file claiming to name "the current project", plus two other
//! layers overriding it out of band, so three entry points give three different
//! answers. Whoever ran `kbase use` last wins, for everyone.
//!
//! The replacement is a per-context declaration. The acceptance criterion is
//! exact: **two project contexts resolve to two `BKMR_DB_URL` values, with nothing
//! shared for them to race over.**

use std::fs;
use std::path::Path;

use tempfile::TempDir;

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// A home carrying the real `tool/search/bkmr` capsule from `contrib/`.
fn seed_home(home: &Path) {
    let src =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contrib/bkmr/capsules/tool/search/bkmr");
    let dst = home.join("registries/personal/capsules/tool/search/bkmr");
    fs::create_dir_all(&dst).unwrap();
    fs::copy(src.join("manifest.toml"), dst.join("manifest.toml"))
        .expect("the contrib bkmr manifest should be readable");
}

/// A project that binds a named bkmr database.
fn project_binding(root: &Path, db: &str, also: &[&str]) {
    let also = if also.is_empty() {
        String::new()
    } else {
        format!(
            "also = [{}]\n",
            also.iter()
                .map(|a| format!("\"{a}\""))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    write(
        &root.join(".aikit/profile.toml"),
        &format!(
            r#"schema = 1
enable = ["tool/search/bkmr"]

[config."tool/search/bkmr"]
db = "{db}"
dir = "~/.config/bkmr/projects"
{also}"#
        ),
    );
}

/// `aikit context env --shell bash`, as the shell integration invokes it.
fn context_env(home: &Path, project: &Path) -> String {
    let bin = assert_cmd::cargo::cargo_bin("aikit");
    let output = std::process::Command::new(&bin)
        .args(["context", "env", "--shell", "bash"])
        .env("AIKIT_HOME", home)
        .env("HOME", home)
        .current_dir(project)
        .output()
        .expect("aikit context env runs");
    assert!(
        output.status.success(),
        "context env failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn value_of(env: &str, name: &str) -> Option<String> {
    env.lines()
        .find_map(|l| l.strip_prefix(&format!("export {name}='")))
        .and_then(|rest| rest.strip_suffix("';"))
        .map(str::to_string)
}

#[test]
fn two_projects_resolve_to_two_bkmr_databases_with_nothing_shared() {
    let home = TempDir::new().unwrap();
    seed_home(home.path());

    let epi = TempDir::new().unwrap();
    let blog = TempDir::new().unwrap();
    project_binding(epi.path(), "epi-logos", &["books"]);
    project_binding(blog.path(), "next-words-blog", &[]);

    let epi_env = context_env(home.path(), epi.path());
    let blog_env = context_env(home.path(), blog.path());

    let epi_db = value_of(&epi_env, "BKMR_DB_URL").expect("epi exports BKMR_DB_URL");
    let blog_db = value_of(&blog_env, "BKMR_DB_URL").expect("blog exports BKMR_DB_URL");

    assert!(epi_db.ends_with("/epi-logos.db"), "got {epi_db}");
    assert!(blog_db.ends_with("/next-words-blog.db"), "got {blog_db}");
    assert_ne!(
        epi_db, blog_db,
        "two contexts must resolve to two databases — this is the whole point"
    );

    // The capsules' own variable agrees with the tool's, so a human typing bare
    // `bkmr` in that pane cannot be looking at a different database.
    assert_eq!(
        value_of(&epi_env, "AIKIT_BKMR_DB").as_deref(),
        Some(epi_db.as_str())
    );

    // `also` is a DECLARED set, not a directory glob.
    let set = value_of(&epi_env, "AIKIT_BKMR_DB_SET").expect("a db set is exported");
    assert!(
        set.contains("epi-logos.db") && set.contains("books.db"),
        "got {set}"
    );
    assert!(
        !value_of(&blog_env, "AIKIT_BKMR_DB_SET")
            .unwrap()
            .contains("books.db"),
        "the blog declared no cross-search set, so it gets none"
    );

    // Nothing global was written: no `.current`, no shared state to race over.
    assert!(
        !home.path().join(".config/bkmr/projects/.current").exists(),
        "the global mutable file must not be recreated in any form"
    );
}

#[test]
fn a_context_that_binds_no_database_exports_nothing_rather_than_guessing() {
    let home = TempDir::new().unwrap();
    seed_home(home.path());
    let project = TempDir::new().unwrap();
    // The tool is enabled, but no database is declared.
    write(
        &project.path().join(".aikit/profile.toml"),
        "schema = 1\nenable = [\"tool/search/bkmr\"]\n",
    );

    let env = context_env(home.path(), project.path());
    assert!(
        value_of(&env, "BKMR_DB_URL").is_none(),
        "AIKit must not invent a database; got {env:?}"
    );
}

#[test]
fn a_value_containing_shell_syntax_cannot_escape_its_quoting() {
    // Config is hand-written and paths contain surprising characters. A value that
    // became shell syntax when eval'd would be a command-injection hole in the
    // integration snippet every shell sources.
    let home = TempDir::new().unwrap();
    seed_home(home.path());
    let project = TempDir::new().unwrap();
    write(
        &project.path().join(".aikit/profile.toml"),
        "schema = 1\nenable = [\"tool/search/bkmr\"]\n\n\
         [config.\"tool/search/bkmr\"]\n\
         db = \"it's; touch /tmp/aikit-pwned; x\"\n",
    );

    let env = context_env(home.path(), project.path());
    let db = value_of(&env, "BKMR_DB_URL").expect("still exported");
    assert!(
        db.contains("touch /tmp/aikit-pwned"),
        "the value is carried verbatim inside its quoting: {db}"
    );
    assert!(
        !std::path::Path::new("/tmp/aikit-pwned").exists(),
        "the value must never be able to execute"
    );
}
