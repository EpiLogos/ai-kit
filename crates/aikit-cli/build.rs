use std::env;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=AIKIT_BUILD_SOURCE_REVISION");
    println!("cargo:rerun-if-env-changed=AIKIT_BUILD_SOURCE_DIRTY");

    if let Ok(revision) = env::var("AIKIT_BUILD_SOURCE_REVISION") {
        if !revision.trim().is_empty() {
            println!(
                "cargo:rustc-env=AIKIT_BUILD_SOURCE_REVISION={}",
                revision.trim()
            );
            println!(
                "cargo:rustc-env=SUITE_BUILD_REVISION={}",
                short_revision(revision.trim())
            );
            let dirty = env::var("AIKIT_BUILD_SOURCE_DIRTY").unwrap_or_else(|_| "0".into());
            println!("cargo:rustc-env=AIKIT_BUILD_SOURCE_DIRTY={dirty}");
            return;
        }
    }

    let manifest = env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default();
    let revision = Command::new("git")
        .arg("-C")
        .arg(&manifest)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|value| !value.is_empty());

    if let Some(revision) = revision {
        println!("cargo:rustc-env=AIKIT_BUILD_SOURCE_REVISION={revision}");
        println!(
            "cargo:rustc-env=SUITE_BUILD_REVISION={}",
            short_revision(&revision)
        );
        let dirty = Command::new("git")
            .arg("-C")
            .arg(&manifest)
            .args(["status", "--porcelain", "--untracked-files=no"])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .is_some_and(|output| !output.stdout.is_empty());
        println!(
            "cargo:rustc-env=AIKIT_BUILD_SOURCE_DIRTY={}",
            if dirty { "1" } else { "0" }
        );
    }
}

/// The suite-wide short build identity stamped into `--version`.
fn short_revision(revision: &str) -> String {
    revision.chars().take(12).collect()
}
