//! `aikit apply` keeps the managed tools layers current: a harness whose
//! embedded profile declares a managed tools layer naming an
//! `mcp-servers-record` seam receives the enabled+trusted `tool-protocol`
//! capsules as MCP server records in its native config — the promise
//! `aikit explain` makes ("projected as an MCP server record"), kept for the
//! same audience `aikit apply` already materialises hook seams for.
//!
//! The acceptance law: the projection rides the same resolution the encounter
//! ACP composition resolves through, foreign records are preserved, the write
//! is a reversible procedure (undo receipt in the reply), a second apply is
//! satisfied rather than a second write, and an enabled-but-untrusted capsule
//! is a `not-projected` outcome naming trust — never a silently empty plan.

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use serde_json::Value;

/// Mirrors the encounter MCP composition test's capsule: a stdio server with
/// an env value, exported as `bimba`.
const BIMBA_MANIFEST: &str = "schema = 1\n\
     id = \"tool-protocol/test/bimba\"\n\
     kind = \"tool-protocol\"\n\
     name = \"Bimba test server\"\n\
     description = \"A real tool-protocol capsule for the apply tools projection test\"\n\
     \n\
     [tool-protocol]\n\
     export_name = \"bimba\"\n\
     \n\
     [tool-protocol.server]\n\
     command = \"/usr/bin/true\"\n\
     args = [\"--port\", \"8080\"]\n\
     \n\
     [tool-protocol.server.env]\n\
     BIMBA_TOKEN = \"sk-test\"\n";

/// The foreign record already in the openclaw seam, byte-exact in the pretty
/// form the merge engine renders, so the untrusted scenario can prove the
/// "already in place" disclosure without a formatting write.
const FOREIGN_OPENCLAW: &str = r#"{
  "mcpServers": {
    "linear-server": {
      "url": "https://mcp.linear.app/sse"
    }
  }
}
"#;

/// An AIKit home with the capsule in a registry and the global profile
/// enabling it; a project to apply in; and a foreign record already in the
/// openclaw seam. `trusted` decides whether the capsule's revision is
/// reviewed before the apply.
fn scenario(trusted: bool) -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join("project/.aikit")).unwrap();
    fs::write(
        home.path().join("project/.aikit/profile.toml"),
        "schema = 1\n",
    )
    .unwrap();

    let aikit_home = home.path().join("aikit-home");
    let capsule_dir = aikit_home.join("registries/test/capsules/tool-protocol/test/bimba");
    fs::create_dir_all(&capsule_dir).unwrap();
    fs::write(capsule_dir.join("manifest.toml"), BIMBA_MANIFEST).unwrap();
    fs::create_dir_all(aikit_home.join("scopes/global")).unwrap();
    fs::write(
        aikit_home.join("scopes/global/profile.toml"),
        "schema = 1\nenable = [\"tool-protocol/test/bimba\"]\n",
    )
    .unwrap();

    let openclaw_dir = home.path().join("user-home/.openclaw");
    fs::create_dir_all(&openclaw_dir).unwrap();
    fs::write(openclaw_dir.join("mcp.json"), FOREIGN_OPENCLAW).unwrap();

    if trusted {
        assert!(
            aikit(home.path())
                .args(["trust", "record", "tool-protocol/test/bimba"])
                .args(["--note", "apply tools projection test review"])
                .output()
                .unwrap()
                .status
                .success(),
            "recording the review must succeed"
        );
    }
    home
}

fn aikit(home: &Path) -> Command {
    let mut command = Command::cargo_bin("aikit").unwrap();
    command
        .env("AIKIT_HOME", home.join("aikit-home"))
        .env("HOME", home.join("user-home"))
        .arg("--json")
        .current_dir(home.join("project"))
        .env("PATH", "/usr/bin:/bin");
    command
}

fn envelope(home: &Path, args: &[&str]) -> Value {
    let output = aikit(home).args(args).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "`aikit {args:?}` must succeed: {stdout} {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_str(&stdout).unwrap()
}

fn outcomes(home: &Path) -> Vec<Value> {
    envelope(home, &["apply"])["data"]["tools_projections"]
        .as_array()
        .unwrap()
        .clone()
}

fn outcome<'a>(projections: &'a [Value], client: &str) -> &'a Value {
    projections
        .iter()
        .find(|outcome| outcome["client"] == client)
        .unwrap_or_else(|| panic!("the {client} tools layer is reported: {projections:?}"))
}

fn openclaw_config(home: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(home.join("user-home/.openclaw/mcp.json")).unwrap())
        .unwrap()
}

#[test]
fn apply_projects_enabled_trusted_capsules_into_every_managed_tools_seam() {
    let home = scenario(true);

    let projections = outcomes(home.path());
    assert_eq!(
        projections.len(),
        3,
        "claude, zcode and openclaw declare managed mcp-servers-record seams: {projections:?}"
    );
    for projection in &projections {
        assert_eq!(projection["state"], "written", "{projection:?}");
        assert!(
            projection["undo"]
                .as_str()
                .is_some_and(|undo| undo.starts_with("aikit procedure undo")),
            "the write is reversible: {projection:?}"
        );
    }

    // The openclaw seam: the record lands around the foreign entry, and the
    // reply discloses the merge and the activation truth.
    let openclaw = outcome(&projections, "openclaw");
    assert_eq!(openclaw["slug"], "openclaw");
    assert!(
        openclaw["path"]
            .as_str()
            .is_some_and(|path| path.ends_with(".openclaw/mcp.json")),
        "the reply names the file the write landed in: {openclaw:?}"
    );
    assert_eq!(openclaw["added"], 1);
    assert_eq!(openclaw["kept_foreign"], 1);
    assert_eq!(openclaw["activation"], "restart-client");

    let merged = openclaw_config(home.path());
    assert_eq!(
        merged["mcpServers"]["bimba"],
        serde_json::json!({
            "command": "/usr/bin/true",
            "args": ["--port", "8080"],
            "env": { "BIMBA_TOKEN": "sk-test" }
        }),
        "the capsule's server record reaches the harness config: {merged}"
    );
    assert_eq!(
        merged["mcpServers"]["linear-server"],
        serde_json::json!({ "url": "https://mcp.linear.app/sse" }),
        "the foreign server survives the projection: {merged}"
    );

    // The other two managed seams receive the same record under their own
    // declared keys: claude's root `mcpServers`, zcode's `mcp.servers`.
    let claude: Value = serde_json::from_str(
        &fs::read_to_string(home.path().join("user-home/.claude.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(outcome(&projections, "claude")["added"], 1);
    assert!(
        claude["mcpServers"]["bimba"].is_object(),
        "claude's managed seam carries the record: {claude}"
    );
    let zcode: Value = serde_json::from_str(
        &fs::read_to_string(home.path().join("user-home/.zcode/cli/config.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(outcome(&projections, "zcode")["added"], 1);
    assert!(
        zcode["mcp"]["servers"]["bimba"].is_object(),
        "zcode's dotted seam carries the record: {zcode}"
    );

    // Nothing tools-related was refused: the hook-seam refusals of this
    // actuation-less scenario are the only warnings.
    let warnings = envelope(home.path(), &["apply"])["warnings"].clone();
    assert!(
        !warnings
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning.as_str().unwrap().contains("tools layer")),
        "a projected tools layer raises no tools warning: {warnings:?}"
    );

    // The re-apply is satisfied, never a second write.
    let again = outcomes(home.path());
    for projection in &again {
        assert_eq!(projection["state"], "satisfied", "{projection:?}");
    }
    let remerged = openclaw_config(home.path());
    assert_eq!(
        remerged, merged,
        "re-applying leaves the seam byte-identical"
    );
}

#[test]
fn an_enabled_but_untrusted_capsule_is_not_projected_and_names_trust() {
    let home = scenario(false);

    let projections = outcomes(home.path());
    assert_eq!(
        projections.len(),
        3,
        "the seams are still reported: {projections:?}"
    );

    // The two seams without a config have nothing to write and nothing to
    // carry: the disclosure names the withheld capsule, never an empty plan.
    for client in ["claude", "zcode"] {
        let projection = outcome(&projections, client);
        assert_eq!(projection["state"], "not-projected", "{projection:?}");
        let reason = projection["reason"].as_str().unwrap();
        assert!(
            reason.contains("not trusted"),
            "the reason turns on trust: {reason}"
        );
        assert!(
            reason.contains("tool-protocol/test/bimba"),
            "the reason names the withheld capsule: {reason}"
        );
        assert!(
            projection["path"].is_null() && projection["undo"].is_null(),
            "nothing was written, so there is no write to undo: {projection:?}"
        );
    }

    // The seam whose config already stands — foreign record only, no AIKit
    // records — is satisfied, not rewritten: nothing was written and no
    // receipt is claimed.
    let before = fs::read_to_string(home.path().join("user-home/.openclaw/mcp.json")).unwrap();
    let openclaw = outcome(&projections, "openclaw");
    assert_eq!(openclaw["state"], "satisfied", "{openclaw:?}");
    assert!(openclaw["undo"].is_null() && openclaw["procedure"].is_null());
    assert_eq!(openclaw["kept_foreign"], 1);
    assert_eq!(
        fs::read_to_string(home.path().join("user-home/.openclaw/mcp.json")).unwrap(),
        before,
        "a satisfied tools layer writes nothing"
    );

    // No empty record files appear for seams with nothing to carry.
    assert!(
        !home.path().join("user-home/.claude.json").exists()
            && !home.path().join("user-home/.zcode").exists(),
        "a not-projected seam creates no config"
    );
}
