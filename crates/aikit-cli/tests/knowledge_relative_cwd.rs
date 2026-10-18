//! A relative `-C` is the same place as its absolute spelling: the Central-root
//! lookup walks the path's ancestors, and the last ancestor of a relative path is the
//! empty path (the current directory), which cannot be canonicalised. Before the fix,
//! `aikit -C Work/child knowledge search` and `aikit -C . knowledge search` answered
//! `central.root_context_unavailable: No such file or directory` while the same
//! command with an absolute `-C` worked.

use std::path::Path;
use std::process::Command;

fn central(root: &Path) {
    for directory in [
        "Control/user",
        "Control/agents",
        "Control/machines",
        "Work/child",
        ".central",
    ] {
        std::fs::create_dir_all(root.join(directory)).unwrap();
    }
}

fn knowledge_search(cwd: &Path, home: &Path, selected: &str) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_aikit"))
        .current_dir(cwd)
        .env("AIKIT_HOME", home.join("aikit-home"))
        .env("HOME", home)
        .env_remove("CENTRAL_ROOT")
        .args(["--json", "-C", selected, "knowledge", "search", "zzzqqq"])
        .output()
        .expect("the aikit binary runs");
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn a_relative_selected_directory_finds_the_central_root_like_its_absolute_spelling() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap().join("Central");
    central(&root);
    let home = temp.path().canonicalize().unwrap().join("home");
    std::fs::create_dir_all(&home).unwrap();

    let absolute = knowledge_search(&root, &home, &root.join("Work/child").display().to_string());
    assert!(
        !absolute.contains("root_context_unavailable"),
        "the absolute spelling is the reference: {absolute}"
    );
    for selected in ["Work/child", "Work", ".", "./Work/child"] {
        let reply = knowledge_search(&root, &home, selected);
        assert!(
            !reply.contains("root_context_unavailable"),
            "-C {selected} must resolve the Central root like the absolute spelling: {reply}"
        );
    }
}
