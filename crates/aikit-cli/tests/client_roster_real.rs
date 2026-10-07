//! The derived-roster invariants against the real Actuation detector.
//!
//! The client roster's universe is the live `actuation harness detect --json`
//! output; these tests pin the join against a real detector, not a fixture:
//!
//! 1. every descriptor the detector reports yields a status row whose
//!    detection leg names the record's own state, and the broker closes the
//!    surface — nothing invented, nothing missing;
//! 2. every embedded harness profile details a slug the catalog declares —
//!    profiles come FROM the catalog list, never the other way round.
//!
//! Skips on hosts without a reachable `actuation` binary, and becomes
//! mandatory when `AIKIT_REQUIRE_ROSTER_REAL=1` is set. Point it at a
//! source-built binary with `AIKIT_REAL_ACTUATION_BIN=/path/to/actuation`
//! (an installed binary may lag the catalog the source tree declares — the
//! lag is a cut fact, never worked around here).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use assert_cmd::Command;
use serde_json::Value;

/// Where the real detector comes from: the explicit env path, else `actuation`
/// on PATH.
fn actuation_bin() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("AIKIT_REAL_ACTUATION_BIN") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
        return None;
    }
    which("actuation")
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Run the real detector and parse its record. `None` (skip) when no detector
/// is reachable.
fn real_detection() -> Option<Value> {
    let Some(bin) = actuation_bin() else {
        if std::env::var_os("AIKIT_REQUIRE_ROSTER_REAL").is_some() {
            panic!("AIKIT_REQUIRE_ROSTER_REAL is set but no actuation binary is reachable");
        }
        eprintln!("SKIP real roster join: no actuation binary is reachable");
        return None;
    };
    let output = StdCommand::new(&bin)
        .args(["harness", "detect", "--json"])
        .output()
        .expect("spawn the real detector");
    if !output.status.success() {
        if std::env::var_os("AIKIT_REQUIRE_ROSTER_REAL").is_some() {
            panic!(
                "AIKIT_REQUIRE_ROSTER_REAL is set but the detector failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        eprintln!("SKIP real roster join: the detector failed to run");
        return None;
    }
    let record: Value = serde_json::from_slice(&output.stdout).expect("detection record parses");
    assert_eq!(
        record["schema"], "actuation.harness-detection/v1",
        "the real detector must speak the detection schema"
    );
    Some(record)
}

/// A disposable AIKit project to run `aikit client status` from, with PATH
/// carrying the real detector so the same binary serves both legs. The
/// machine's real HOME is kept: detection's config-dir probes (zcode is the
/// canary — it has no executable by design and is detected through
/// `~/.zcode` alone) must see the same environment on both legs, or the join
/// is tested against two different machines.
fn status_rows(actuation: &Path) -> Vec<Value> {
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join("project/.aikit")).unwrap();
    fs::write(
        home.path().join("project/.aikit/profile.toml"),
        "schema = 1\n",
    )
    .unwrap();
    let mut path = std::env::join_paths([
        actuation.parent().unwrap().to_path_buf(),
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
    ])
    .unwrap();
    if let Some(existing) = std::env::var_os("PATH") {
        path = std::env::join_paths(
            std::env::split_paths(&path).chain(std::env::split_paths(&existing)),
        )
        .unwrap();
    }
    let output = Command::cargo_bin("aikit")
        .unwrap()
        .env("AIKIT_HOME", home.path().join("aikit-home"))
        .env("PATH", path)
        .arg("--json")
        .args(["client", "status"])
        .current_dir(home.path().join("project"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "client status must succeed against the real detector: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
    envelope["data"]["clients"].as_array().cloned().unwrap()
}

/// The join runs two real detections; under machine contention a probe can
/// legitimately time out and disclose "unavailable" on one leg while the
/// other leg saw "detected". That is an observation race, not an incoherent
/// join — so the coherence assertion retries the whole join once before
/// failing: a genuinely incoherent join fails both times, a contended probe
/// passes on the second, quieter attempt.
#[test]
fn every_real_descriptor_yields_a_row_with_a_coherent_state() {
    if let Err(first) = assert_join_coherent() {
        eprintln!("first join attempt was incoherent ({first}); retrying once");
        if let Err(second) = assert_join_coherent() {
            panic!("the join is incoherent on both attempts:\nfirst: {first}\nsecond: {second}");
        }
    }
}

fn assert_join_coherent() -> Result<(), String> {
    join_coherence().map_err(|e| e.to_string())
}

fn join_coherence() -> Result<(), String> {
    let Some(record) = real_detection() else {
        return Ok(());
    };
    let Some(actuation) = actuation_bin() else {
        return Err("the detector that just ran is not findable".to_owned());
    };
    let rows = status_rows(&actuation);

    let record_harnesses = record["harnesses"].as_array().cloned().unwrap();
    if record_harnesses.is_empty() {
        return Err(
            "a real catalog declares harnesses; an empty record is a cut fact to investigate"
                .to_owned(),
        );
    }

    // The broker closes the surface, exactly once.
    if rows.iter().filter(|row| row["client"] == "broker").count() != 1 {
        return Err("exactly one broker row expected".to_owned());
    }

    let detection_vocabulary = [("detected", "detected"), ("not-installed", "not-installed")];
    for entry in &record_harnesses {
        let slug = entry["slug"].as_str().expect("record slug");
        let state = entry["state"].as_str().expect("record state");
        // The row may be named by its overlay's CLI-facing name, so match on
        // the join key the row carries: its catalog slug.
        let row = rows
            .iter()
            .find(|row| row["harness"].as_str() == Some(slug))
            .ok_or_else(|| format!("every descriptor yields a row; {slug} did not"))?;
        // The detection leg names the record's own state, whatever the
        // derived surface state says about installability.
        let mapped: Vec<_> = detection_vocabulary
            .iter()
            .filter(|(record_state, _)| *record_state == state)
            .map(|(_, leg)| *leg)
            .collect();
        if !mapped.is_empty() && row["detection"] != mapped[0] {
            // Under machine contention the row's own detection run can
            // degrade an entry to "unavailable". That leg still names what
            // it observed — no masking in either direction — provided the
            // degradation carries its reason. A silent mismatch, or any
            // mismatch that claims more than the record saw, stays a
            // failure.
            let disclosed_degradation =
                row["detection"] == "unavailable" && row["detection_reason"].is_string();
            if !disclosed_degradation {
                return Err(format!(
                    "{slug}: the row's detection leg ({}) must name the record's state ({}); \
                     row reason: {:?}; row state: {}",
                    row["detection"], mapped[0], row["detection_reason"], row["state"]
                ));
            }
        }
        // A coherent row always carries one of the derived states.
        if !["installable", "gap", "absent", "unavailable", "self"]
            .contains(&row["state"].as_str().unwrap_or(""))
        {
            return Err(format!(
                "{slug}: state {} is outside the derived vocabulary",
                row["state"]
            ));
        }
    }

    // Every row is the broker, or a catalog slug: a harness the record does
    // not name and no overlay declares cannot appear.
    for row in &rows {
        if row["client"] == "broker" {
            continue;
        }
        let slug = row["harness"]
            .as_str()
            .expect("every harness row names its slug");
        let in_record = record_harnesses
            .iter()
            .any(|entry| entry["slug"].as_str() == Some(slug));
        let overlaid =
            aikit_adapters::profiles::slug_for_target(&aikit_core::TargetId::new(slug)).is_some();
        if !in_record && !overlaid {
            return Err(format!(
                "row {slug} is neither in the detection record nor an overlay slug"
            ));
        }
    }
    Ok(())
}

#[test]
fn every_embedded_profile_slug_is_a_declared_catalog_slug() {
    let Some(record) = real_detection() else {
        return;
    };
    let declared: Vec<&str> = record["harnesses"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["slug"].as_str().expect("record slug"))
        .collect();

    // Profiles detail FROM the catalog list: a profile for a slug the
    // detector does not declare is AIKit knowing about a harness the catalog
    // never declared — the disease this design removes.
    for (slug, _) in aikit_adapters::profiles::all() {
        assert!(
            declared.contains(&slug),
            "profile {slug} details a slug the catalog does not declare — profiles come \
             FROM the catalog list; remove the profile or land the descriptor"
        );
    }
}
