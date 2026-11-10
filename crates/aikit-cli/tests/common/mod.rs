//! Fixtures shared by the CLI integration tests.

#![allow(dead_code)]

/// End a private tmux server and remove the socket file it leaves behind.
///
/// `kill-server` stops the server but does not unlink its socket, so every run
/// of every tmux test used to leave one inode in the shared tmux directory
/// forever. On a machine that has run these suites for a while that is
/// thousands of files, and the pile is not merely untidy: tmux scans that
/// directory, and the tests themselves grow slower and flakier as it fills.
///
/// The path comes from the running server rather than from reconstructing
/// `$TMUX_TMPDIR/tmux-$(id -u)/<socket>` by hand, because a reconstruction that
/// drifts from what tmux actually chose fails silently — which is precisely how
/// a cleanup stops cleaning without anyone noticing.
pub fn end_tmux_server(socket: &str) {
    // Ask the live server where its socket is. This is exact when it answers.
    let reported = std::process::Command::new("tmux")
        .args(["-L", socket, "display-message", "-p", "#{socket_path}"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|path| !path.is_empty());

    let _ = std::process::Command::new("tmux")
        .args(["-L", socket, "kill-server"])
        .output();

    // The server may already have been gone — which is exactly what happens on
    // the failure paths this cleanup most needs to cover — and a dead server
    // cannot tell us anything. Fall back to the layout tmux documents, so a
    // test that died still takes its socket with it.
    let path = reported.map(std::path::PathBuf::from).or_else(|| {
        let base = std::env::var("TMUX_TMPDIR").unwrap_or_else(|_| "/tmp".to_string());
        let uid = String::from_utf8(
            std::process::Command::new("id")
                .arg("-u")
                .output()
                .ok()?
                .stdout,
        )
        .ok()?;
        Some(
            std::path::PathBuf::from(base)
                .join(format!("tmux-{}", uid.trim()))
                .join(socket),
        )
    });

    if let Some(path) = path {
        let _ = std::fs::remove_file(path);
    }
}

// ---------------------------------------------------------------------------
// The person's test grant (Central's gated file-map acts)
// ---------------------------------------------------------------------------

/// The token of the person's test grant (`grant_person_acts`): a controlled test value, never a live secret.
///
/// Since Central #339 each file-map act that changes the person's ground or what they have adopted (`register`, `link`,
/// `pool`, `adopt-db`, `record-adopt`, `scope-register` and the moves) is the person's own act: ctrl authenticates it on the
/// token its host hands it in `CENTRAL_NATIVE_TOKEN`, against a human grant naming that exact Action where the act stands, and
/// refuses it otherwise. A fixture World that runs one holds this grant, and only the commands that run those acts carry the
/// token.
pub const PERSON_TEST_TOKEN: &str = "aikit-fixture-person-grant-controlled-test-not-a-live-secret";

/// Write the person's test grant into the fixture World at `root`, and nowhere else: one human grant for `actions` (whole
/// Action names, such as `central.file-map.register`) at each World in `scopes` (`control:root`, or `project:<project_id>`
/// for an act that names a Work project), kept under `Control/user` and recognised in Control's relations document as the
/// World's native-action-authority Source. It changes the root's relations revision, so a fixture writes it before it reads
/// the basis an act is made on.
pub fn grant_person_acts(root: &std::path::Path, actions: &[&str], scopes: &[&str]) {
    use sha2::{Digest, Sha256};
    let source = "Control/user/aikit-fixture-person-authority.json";
    let authority = serde_json::json!({
        "schema": "central.native-action-authority/v1",
        "scope_ref": "control:root",
        "grants": [{
            "principal_ref": "human:aikit-fixture-person",
            "actor_kind": "human",
            "token_sha256": format!("{:x}", Sha256::digest(PERSON_TEST_TOKEN.as_bytes())),
            "scope_refs": scopes,
            "actions": actions,
            "expires_at_unix_seconds": u64::MAX,
        }],
    });
    std::fs::create_dir_all(root.join("Control/user")).unwrap();
    std::fs::write(root.join(source), serde_json::to_vec(&authority).unwrap()).unwrap();
    let path = root.join("Control/relations/source-relations.json");
    let mut relations = match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => serde_json::json!({
            "schema": "central.control.ground-relations/v1",
            "project_id": "control:root",
            "relations": [],
        }),
        Err(error) => panic!("the fixture World's relations document is unreadable: {error}"),
    };
    relations["relations"]
        .as_array_mut()
        .expect("the fixture World's relations document holds its relations")
        .push(serde_json::json!({
            "ref": format!("central:source:control:root:{source}"),
            "path": source,
            "roles": ["native-action-authority"],
            "provenance": "human-adopted",
            "standing": "architecture-contract",
            "treatment": "control-user",
            "recognition": "controlled-test-only",
            "recorded_at_unix_seconds": 1,
        }));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, serde_json::to_vec(&relations).unwrap()).unwrap();
}
