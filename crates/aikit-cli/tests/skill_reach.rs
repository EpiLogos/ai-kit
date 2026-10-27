//! A skill authored in the spool reaches the user's agents with no list to
//! keep, or the command says why not.
//!
//! The user-level harness roots (`~/.claude/skills`, `~/.agents/skills`) are fed
//! from one well-known context, the user scope, which resolves the global
//! profile and nothing else. Rules in that profile name families
//! (`skill/personal/**`), so a skill catalogued tomorrow is covered without
//! anyone appending its id.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::TempDir;

const USER_SCOPE: &str = "ctx_user-scope";

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn skill(ground: &Path, name: &str) {
    write(
        &ground.join(name).join("SKILL.md"),
        &format!("---\nname: {name}\ndescription: The {name} skill.\n---\nBody.\n"),
    );
}

fn run(home: &Path, dir: &Path, args: &[&str]) -> Output {
    Command::new(assert_cmd::cargo::cargo_bin("aikit"))
        .args(args)
        .arg("--json")
        .env("AIKIT_HOME", home)
        .env("HOME", home)
        .env_remove("AIKIT_CONTEXT_ID")
        .env_remove("AIKIT_SESSION_ID")
        .env_remove("CENTRAL_ROOT")
        .current_dir(dir)
        .output()
        .unwrap()
}

fn ok(home: &Path, dir: &Path, args: &[&str]) -> Value {
    let output = run(home, dir, args);
    assert!(
        output.status.success(),
        "{args:?}: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn warnings(envelope: &Value) -> String {
    envelope["warnings"]
        .as_array()
        .map(|all| {
            all.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

fn author(home: &Path, dir: &Path, ground: &Path) -> Value {
    ok(home, dir, &["source", "sync", "personal"]);
    ok(home, dir, &["source", "promote", "personal", "--trust"]);
    let _ = ground;
    Value::Null
}

struct World {
    home: TempDir,
    project: TempDir,
    ground: TempDir,
}

impl World {
    fn new() -> Self {
        let world = Self {
            home: TempDir::new().unwrap(),
            project: TempDir::new().unwrap(),
            ground: TempDir::new().unwrap(),
        };
        write(
            &world.project.path().join(".aikit/profile.toml"),
            "schema = 1\n",
        );
        world
    }

    fn home(&self) -> &Path {
        self.home.path()
    }

    fn project(&self) -> &Path {
        self.project.path()
    }

    fn root(&self) -> PathBuf {
        self.home().join(".agents/skills")
    }

    fn user_tree(&self) -> PathBuf {
        self.home()
            .join("state/contexts")
            .join(USER_SCOPE)
            .join("current/projections/codex/.agents/skills")
    }

    fn global_profile(&self) -> PathBuf {
        self.home().join("scopes/global/profile.toml")
    }

    fn ok(&self, args: &[&str]) -> Value {
        ok(self.home(), self.project(), args)
    }

    /// The reviewed first wiring of a harness root to the user scope.
    fn wire(&self) {
        let root = self.root();
        fs::create_dir_all(&root).unwrap();
        let tree = self.user_tree();
        let wire = [
            "adopt",
            root.to_str().unwrap(),
            "--projection",
            tree.to_str().unwrap(),
        ];
        let preview = self.ok(&wire);
        let mut confirm = wire.to_vec();
        confirm.extend([
            "--yes",
            "--expect-digest",
            preview["data"]["review_digest"].as_str().unwrap(),
        ]);
        self.ok(&confirm);
    }
}

#[test]
fn a_skill_authored_after_the_rule_reaches_the_agent_with_no_list_to_keep() {
    let world = World::new();
    skill(world.ground.path(), "alpha");
    skill(world.ground.path(), "beta");
    ok(
        world.home(),
        world.project(),
        &[
            "source",
            "add-directory",
            "personal",
            world.ground.path().to_str().unwrap(),
            "--control-ground",
        ],
    );
    author(world.home(), world.project(), world.ground.path());

    // One rule, written once, in the global profile.
    let rule = world.ok(&["enable", "skill/personal/**", "--scope", "user"]);
    assert_eq!(rule["data"]["effect"], "allow");
    assert_eq!(rule["data"]["covers"], 2);
    let profile = fs::read_to_string(world.global_profile()).unwrap();
    assert!(
        profile.contains("allow") && profile.contains("skill/personal/**"),
        "{profile}"
    );
    assert!(
        !profile.contains("skill/personal/alpha"),
        "a rule names the family, not its members: {profile}"
    );
    assert!(world.user_tree().join("alpha/SKILL.md").is_file());

    // The first wiring of a harness root is the explicit, reviewed adoption.
    world.wire();
    let root = world.root();
    assert!(root.join("alpha").is_symlink() && root.join("beta").is_symlink());

    // A skill authored afterwards: sync, promote — nothing else.
    skill(world.ground.path(), "gamma");
    let promoted = {
        ok(
            world.home(),
            world.project(),
            &["source", "sync", "personal"],
        );
        ok(
            world.home(),
            world.project(),
            &["source", "promote", "personal", "--trust"],
        )
    };
    assert!(
        root.join("gamma").is_symlink() && root.join("gamma/SKILL.md").is_file(),
        "a skill covered by the rule must reach the root when it is promoted: {promoted}"
    );
    assert_eq!(
        promoted["data"]["user_scope"]["skill_roots"][0]["state"], "linked",
        "{promoted}"
    );

    // An exact disable withdraws one member, and the link goes with it.
    world.ok(&["disable", "skill/personal/beta", "--scope", "user"]);
    assert!(!root.join("beta").exists() && !root.join("beta").is_symlink());
    assert!(root.join("alpha").is_symlink() && root.join("gamma").is_symlink());

    // A deny rule withdraws a family, and forgetting it brings the family back.
    let denied = world.ok(&["disable", "skill/personal/g*", "--scope", "user"]);
    assert_eq!(denied["data"]["effect"], "deny");
    assert!(!root.join("gamma").is_symlink());
    let forgotten = world.ok(&[
        "disable",
        "skill/personal/g*",
        "--scope",
        "user",
        "--forget",
    ]);
    assert_eq!(forgotten["data"]["effect"], "forgotten");
    assert!(root.join("gamma").is_symlink());

    // A rule that names nothing is refused instead of written.
    let typo = run(
        world.home(),
        world.project(),
        &["enable", "skill/persnal/*", "--scope", "user"],
    );
    assert!(!typo.status.success());
    assert!(String::from_utf8_lossy(&typo.stdout).contains("names no capability"));
}

#[test]
fn the_user_scope_resolves_the_global_profile_alone() {
    let world = World::new();
    skill(world.ground.path(), "alpha");
    skill(world.ground.path(), "beta");
    ok(
        world.home(),
        world.project(),
        &[
            "source",
            "add-directory",
            "personal",
            world.ground.path().to_str().unwrap(),
            "--control-ground",
        ],
    );
    author(world.home(), world.project(), world.ground.path());
    world.ok(&["enable", "skill/personal/alpha", "--scope", "user"]);
    world.wire();

    // The project asks for beta. That is the project's business: it must not
    // appear in the roots every session of the user reads.
    write(
        &world.project().join(".aikit/profile.toml"),
        "schema = 1\nenable = [\"skill/personal/beta\"]\n",
    );
    let applied = world.ok(&["apply", "--user"]);
    assert!(world.root().join("alpha").is_symlink());
    assert!(
        !world.root().join("beta").exists(),
        "a project layer leaked into the user scope: {applied}"
    );
    // An apply in the project itself is not nagged about the user's roots.
    let project_apply = world.ok(&["apply"]);
    assert!(
        !warnings(&project_apply).contains("resolves through"),
        "{}",
        warnings(&project_apply)
    );
}

#[test]
fn a_root_that_is_not_wired_to_the_user_scope_is_named_with_the_way_to_wire_it() {
    let world = World::new();
    skill(world.ground.path(), "alpha");
    ok(
        world.home(),
        world.project(),
        &[
            "source",
            "add-directory",
            "personal",
            world.ground.path().to_str().unwrap(),
            "--control-ground",
        ],
    );
    author(world.home(), world.project(), world.ground.path());
    fs::create_dir_all(world.home().join(".claude/skills")).unwrap();
    let rule = world.ok(&["enable", "skill/personal/**", "--scope", "user"]);
    let notes = warnings(&rule);
    assert!(
        notes.contains("is not wired to the user scope")
            && notes.contains("aikit adopt")
            && notes.contains("--projection"),
        "{notes}"
    );
}

#[test]
fn a_skill_whose_directory_differs_from_its_name_is_said_at_sync() {
    let world = World::new();
    write(
        &world.ground.path().join("workcell-thing/SKILL.md"),
        "---\nname: thing\ndescription: The thing skill.\n---\nBody.\n",
    );
    skill(world.ground.path(), "agreeing");
    ok(
        world.home(),
        world.project(),
        &[
            "source",
            "add-directory",
            "personal",
            world.ground.path().to_str().unwrap(),
            "--control-ground",
        ],
    );
    let synced = world.ok(&["source", "sync", "personal"]);
    let notes = warnings(&synced);
    assert!(
        notes.contains("skill directory `workcell-thing` declares name `thing`"),
        "{notes}"
    );
    assert!(
        !notes.contains("`agreeing`"),
        "an agreeing skill is not mentioned: {notes}"
    );
}
