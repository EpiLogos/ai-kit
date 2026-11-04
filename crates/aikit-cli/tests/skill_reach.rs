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

/// Every path under `dir` with its kind, size and modification time: what a
/// read-only command must leave exactly as it found it.
fn listing(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(next) = stack.pop() {
        let Ok(entries) = fs::read_dir(&next) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let meta = fs::symlink_metadata(&path).unwrap();
            out.push(format!(
                "{} {} {:?} {:?}",
                path.display(),
                meta.len(),
                meta.file_type(),
                meta.modified().ok()
            ));
            if meta.is_dir() {
                stack.push(path);
            }
        }
    }
    out.sort();
    out
}

/// `aikit skill reach`: the verdict envelope and whether the process passed.
fn reach(world: &World) -> (bool, Value) {
    let output = run(world.home(), world.project(), &["skill", "reach"]);
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "skill reach printed no envelope ({error}): {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.success(), envelope)
}

fn root_of<'a>(envelope: &'a Value, harness: &str) -> &'a Value {
    envelope["data"]["roots"]
        .as_array()
        .and_then(|roots| roots.iter().find(|root| root["harness"] == harness))
        .unwrap_or_else(|| panic!("no {harness} root in {envelope}"))
}

#[test]
fn the_delivery_check_fails_on_every_gap_and_passes_when_each_authored_skill_reaches_each_root() {
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
    let root = world.root();
    fs::create_dir_all(&root).unwrap();

    // Authored but enabled nowhere, and a harness root nothing feeds: both gaps.
    let (passed, unwired) = reach(&world);
    assert!(!passed, "a gap must fail the check: {unwired}");
    assert_eq!(unwired["data"]["verdict"], "gap", "{unwired}");
    assert_eq!(
        root_of(&unwired, "@agents")["wired"],
        "unwired",
        "{unwired}"
    );
    let unassigned = unwired["data"]["unassigned"].to_string();
    assert!(
        unassigned.contains("skill/personal/alpha") && unassigned.contains("skill/personal/beta"),
        "{unwired}"
    );
    assert!(
        warnings(&unwired).contains("aikit enable"),
        "each gap names its way out: {}",
        warnings(&unwired)
    );

    // Enabled by the rule and wired: every authored skill reaches the root.
    world.ok(&["enable", "skill/personal/**", "--scope", "user"]);
    world.wire();
    // A harness AIKit does not feed is disclosed, never counted as a gap.
    write(
        &world.home().join(".hermes/skills/own/SKILL.md"),
        "---\nname: own\ndescription: Hermes's own.\n---\n",
    );
    let before = listing(world.home());
    let (passed, complete) = reach(&world);
    assert!(passed, "{complete}");
    assert_eq!(complete["data"]["verdict"], "complete", "{complete}");
    assert_eq!(complete["data"]["gaps"], 0, "{complete}");
    let agents = root_of(&complete, "@agents");
    assert_eq!(agents["wired"], "user-scope", "{complete}");
    assert_eq!(agents["linked"], 2, "{complete}");
    assert!(
        agents["read_by"].to_string().contains("codex")
            && agents["read_by"].to_string().contains("pi"),
        "{complete}"
    );
    assert!(
        root_of(&complete, "@hermes")["fed_from"].is_null(),
        "{complete}"
    );
    // The generation tree as apply spells its links.
    let wired = fs::read_link(root.join("beta"))
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    assert_eq!(
        listing(world.home()),
        before,
        "the delivery check reads; it never links, prunes or applies"
    );

    // A link deleted by hand: the skill no longer reaches that root.
    fs::remove_file(root.join("alpha")).unwrap();
    let (passed, absent) = reach(&world);
    assert!(!passed, "{absent}");
    assert_eq!(
        root_of(&absent, "@agents")["absent"],
        serde_json::json!(["alpha"])
    );

    // A directory the generation does not own, in the generated skill's place.
    skill(&root, "alpha");
    let (passed, shadowed) = reach(&world);
    assert!(!passed, "{shadowed}");
    assert_eq!(
        root_of(&shadowed, "@agents")["shadowed"],
        serde_json::json!(["alpha"])
    );
    fs::remove_dir_all(root.join("alpha")).unwrap();
    // Relinked by hand through the home's own spelling (on macOS a temporary
    // home resolves through /private): a link that reaches the skill reaches it.
    std::os::unix::fs::symlink(world.user_tree().join("alpha"), root.join("alpha")).unwrap();
    assert!(reach(&world).0, "relinked, the root is whole again");

    // A catalogued skill set is switched on when each member reaches: by its
    // own id, or by another copy under the same name (one copy each). A member
    // with no copy enabled and no decision against it is a gap.
    let members = world.home().join("skillsets/reader/members");
    write(
        &members,
        "# the reader's skills\nskill/personal/alpha\nskill/upstream/beta\nskill/upstream/zeta\n",
    );
    let (passed, unset) = reach(&world);
    assert!(!passed, "{unset}");
    assert_eq!(
        unset["data"]["unreached"],
        serde_json::json!([{"skill": "skill/upstream/zeta", "sets": ["reader"]}]),
        "{unset}"
    );
    assert!(
        warnings(&unset).contains("skill/upstream/zeta"),
        "{}",
        warnings(&unset)
    );
    write(&members, "skill/personal/alpha\nskill/upstream/beta\n");
    assert!(reach(&world).0, "every member reaches by one copy");

    // One copy each: pi reads its own root and ~/.agents/skills. A link to the
    // same skill is one copy; a second skill under the same name is two.
    let pi = world.home().join(".pi/agent/skills");
    fs::create_dir_all(&pi).unwrap();
    std::os::unix::fs::symlink(root.join("beta"), pi.join("beta")).unwrap();
    assert!(
        reach(&world).0,
        "a link to the same skill is not a second copy"
    );
    write(
        &pi.join("alpha-copy/SKILL.md"),
        "---\nname: alpha\ndescription: A second alpha.\n---\n",
    );
    let (passed, twice) = reach(&world);
    assert!(!passed, "{twice}");
    let duplicates = twice["data"]["duplicates"].to_string();
    assert!(
        duplicates.contains("\"pi\"")
            && duplicates.contains("\"alpha\"")
            && duplicates.contains("alpha-copy"),
        "{twice}"
    );
    fs::remove_dir_all(pi.join("alpha-copy")).unwrap();

    // A generation that did not build an active skill: the tree lacks it, so
    // no root can have it.
    fs::rename(wired.join("alpha"), wired.join("alpha.unbuilt")).unwrap();
    let (passed, unbuilt) = reach(&world);
    assert!(!passed, "{unbuilt}");
    assert_eq!(
        root_of(&unbuilt, "@agents")["unprojected"],
        serde_json::json!(["skill/personal/alpha"]),
        "{unbuilt}"
    );
    fs::rename(wired.join("alpha.unbuilt"), wired.join("alpha")).unwrap();
    assert!(reach(&world).0);

    // A skill the generation no longer carries leaves a link naming nothing.
    world.ok(&["disable", "skill/personal/beta", "--scope", "user"]);
    std::os::unix::fs::symlink(wired.join("beta"), root.join("beta")).unwrap();
    let (passed, dangling) = reach(&world);
    assert!(!passed, "{dangling}");
    assert_eq!(
        root_of(&dangling, "@agents")["dangling"],
        serde_json::json!(["beta"])
    );
    // An exact disable is a decision, not a gap.
    assert!(
        !dangling["data"]["unassigned"].to_string().contains("beta"),
        "{dangling}"
    );
}
