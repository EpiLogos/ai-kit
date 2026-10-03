//! Additional hosted characterization; every child uses the existing native owner.
//! Source archives and CompilerArtifact binaries are prerequisites, not fake Worlds.
#![cfg(any(target_os = "linux", target_os = "macos"))]

use std::collections::BTreeSet;
use std::error::Error;
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use aikit_adapters::runner::{CommandRunner, Output, SystemRunner};
use aikit_core::AikitError;
use rustix::fs::OFlags;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const OLD_HEAD: &str = "b07ced38f17319305ebbee06502df8ad96f3b0eb";
const CASES: [&str; 2] = [
    "actual_old_api_manifest_deadline_requires_native_timeout_not_completed_status",
    "actual_old_api_two_stream_lf_overflow_requires_native_capacity_refusal",
];
const SENSITIVITY_CASE: &str = "actual_native_spawn_failure_keeps_fixture_across_assertion_unwind";
const RUNNER_CASES: [&str; 4] = [
    "runner::tests::actual_script_lf_capacity_refuses_zero_before_spawn_and_keeps_generic_default",
    "runner::tests::actual_script_policy_survives_limited_runner_clone_and_status_mapping",
    "runner::tests::actual_body_free_diagnostics_keep_generic_compatibility_and_original_decoder_cause",
    "runner::tests::actual_body_free_spawn_diagnostics_omit_private_program_cwd_and_arguments",
];
const RUN_CASES: [&str; 10] = [
    "actual_capture_preserves_both_streams_line_basis_and_shared_completed_result",
    "actual_manifest_capture_deadline_reaches_the_same_native_owner",
    "actual_capture_lf_boundary_is_shared_across_both_native_streams",
    "actual_capture_large_single_line_keeps_the_existing_byte_capacity",
    "actual_capture_signal_status_matches_the_capsule_status_contract",
    "actual_capture_invalid_utf8_is_refused_without_a_semantic_report",
    "actual_closed_delivery_socket_preserves_completed_capture_and_original_io",
    "actual_unfinished_descendant_capture_is_not_a_completed_capsule_result",
    "actual_empty_and_unterminated_capture_preserve_the_old_digest_basis",
    "actual_selected_capture_failure_keeps_partial_output_and_arguments_out_of_printers",
];
const EXPORT_CASES: [&str; 3] = [
    "actual_applied_capture_export_and_run_cli_deliver_exact_streams_and_status",
    "actual_capture_export_still_refuses_unreviewed_and_changed_applied_revisions",
    "actual_capture_export_reuses_the_declared_deadline_and_scope",
];
// These native ownership gates remain in the ordinary full suite as well.
const LIFECYCLE_CASES: [&str; 8] = [
    "runner::tests::actual_owned_group_signal_and_direct_child_reap_are_separate_results",
    "runner::tests::actual_completed_eof_preserves_a_background_child_with_closed_streams",
    "runner::tests::actual_external_reap_marks_ownership_lost_and_drop_preserves_live_background_group",
    "runner::tests::actual_leader_exit_does_not_wait_for_descendant_held_pipes",
    "runner::tests::actual_output_capacity_failure_retains_possible_execution_effects_for_both_streams",
    "runner::tests::actual_spawn_failure_keeps_original_io_and_no_execution",
    "runner::tests::actual_strict_stdout_and_stderr_refusal_keep_decoding_cause_and_completed_lifecycle",
    "runner::tests::actual_strict_decoding_waits_for_the_same_held_descendant_pipe_retirement",
];

fn invalid(message: &str) -> io::Error { io::Error::new(io::ErrorKind::InvalidData, message) }
fn string<'a>(value: &'a Value, key: &str) -> io::Result<&'a str> {
    value.get(key).and_then(Value::as_str).ok_or_else(|| invalid("Missing actual string fact"))
}
fn array<'a>(value: &'a Value, key: &str) -> io::Result<&'a Vec<Value>> {
    value.get(key).and_then(Value::as_array).ok_or_else(|| invalid("Missing actual array fact"))
}
fn normal_absolute(path: &Path) -> bool {
    path.is_absolute() && path.components().all(|c| matches!(c, Component::RootDir | Component::Normal(_)))
}
fn identity(meta: &Metadata) -> (u64, u64) { (meta.dev(), meta.ino()) }
fn open_regular(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new().read(true)
        .custom_flags((OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC).bits() as i32).open(path)?;
    let held = file.metadata()?;
    let named = fs::symlink_metadata(path)?;
    if !held.is_file() || held.nlink() != 1 || !named.is_file() || identity(&held) != identity(&named) {
        return Err(invalid("Actual evidence must be an unchanged ordinary single-link file"));
    }
    Ok(file)
}
fn digest(path: &Path) -> io::Result<String> {
    let mut file = open_regular(path)?;
    let before = file.metadata()?;
    let mut hash = Sha256::new();
    let mut block = [0; 16_384];
    loop {
        let n = file.read(&mut block)?;
        if n == 0 { break; }
        hash.update(&block[..n]);
    }
    let after = file.metadata()?;
    let named = fs::symlink_metadata(path)?;
    if identity(&before) != identity(&after) || identity(&after) != identity(&named)
        || before.len() != after.len() || before.modified()? != after.modified()?
        || !named.is_file() || named.nlink() != 1 {
        return Err(invalid("Actual file basis changed while hashing"));
    }
    Ok(format!("{:x}", hash.finalize()))
}
fn read_json(path: &Path) -> io::Result<Value> {
    let mut file = open_regular(path)?;
    let mut bytes = Vec::new();
    (&mut file).take(16 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 16 * 1024 * 1024 { return Err(invalid("Actual gate metadata exceeds its mechanical budget")); }
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

struct Gate {
    root: PathBuf,
    held: File,
    tuple: (u64, u64),
    product: PathBuf,
    metadata: Value,
    metadata_digest: String,
    evidence: PathBuf,
    held_evidence: File,
}
impl Gate {
    fn admit() -> io::Result<Self> {
        let root = PathBuf::from(std::env::var_os("AIKIT_SCRIPT_CHARACTERIZATION_ROOT")
            .ok_or_else(|| invalid("Actual hosted source and CompilerArtifact prerequisites must be supplied"))?);
        let product = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize()?;
        let scratch = product.join("ProjectCentral/now/tmp").canonicalize()?;
        if !normal_absolute(&root) || root.canonicalize()? != root || !root.starts_with(&scratch) || root == scratch {
            return Err(invalid("Actual gate must be exclusive physical product scratch"));
        }
        let held = OpenOptions::new().read(true)
            .custom_flags((OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC).bits() as i32).open(&root)?;
        let meta = held.metadata()?;
        if !meta.is_dir() || identity(&meta) != identity(&fs::symlink_metadata(&root)?) {
            return Err(invalid("Actual admitted gate affiliation is unavailable"));
        }
        let metadata_digest = digest(&root.join("gate.json"))?;
        let metadata = read_json(&root.join("gate.json"))?;
        if digest(&root.join("gate.json"))? != metadata_digest {
            return Err(invalid("Actual prerequisite metadata changed during admission"));
        }
        if string(&metadata, "old_head")? != OLD_HEAD || string(&metadata, "checkout_root")? != product.to_str().ok_or_else(|| invalid("Native source coordinate must be UTF8"))?
            || metadata["default_outcome"] != "success"
            || metadata["inner_refusal_observation_contract"] != "expected_command_failure_original_and_structured_cleanup/v2"
            || metadata["retained_fixture_contract"] != "retained_before_effect_and_on_unwind/v2" {
            return Err(invalid("Actual source or earlier owner gate does not qualify"));
        }
        let evidence = root.join("driver-evidence");
        fs::create_dir(&evidence)?; // exclusive, no retry or replacement of an earlier receipt
        fs::set_permissions(&evidence, fs::Permissions::from_mode(0o700))?;
        let held_evidence = OpenOptions::new().read(true)
            .custom_flags((OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC).bits() as i32).open(&evidence)?;
        let gate = Self { root, held, tuple: identity(&meta), product, metadata, metadata_digest, evidence, held_evidence };
        gate.check()?;
        for (role, target) in [("old", "captured_script_old_api_characterization"),
            ("new", "captured_script_old_api_characterization"), ("runner", "aikit_adapters"),
            ("run_exec", "run_exec"), ("multicall_symlink", "multicall_symlink"),
            ("palette_run_intent", "palette_run_intent"), ("scoped_praxis_native", "scoped_praxis_native"),
            ("driver", "captured_script_characterization"), ("aikit", "aikit")] {
            let binding = &gate.metadata["executables"][role];
            let artifact = &binding["compiler_artifact"];
            let src = Path::new(string(&artifact["target"], "src_path")?);
            let source_root = Path::new(string(binding, "cwd")?);
            if artifact["reason"] != "compiler-artifact" || artifact["target"]["name"] != target
                || artifact["profile"]["test"] != (role != "aikit")
                || artifact["executable"] != binding["original_executable"]
                || !src.starts_with(source_root)
                || digest(Path::new(string(binding, "path")?))? != string(binding, "sha256")? {
                return Err(invalid("Actual CompilerArtifact/source/binary relation does not match"));
            }
        }
        let driver = &gate.metadata["executables"]["driver"];
        if std::env::current_exe()?.canonicalize()? != Path::new(string(driver,"original_executable")?).canonicalize()?
            || digest(&std::env::current_exe()?)? != string(driver,"sha256")? {
            return Err(invalid("Selected driver is not the actual same-source compiled artifact"));
        }
        Ok(gate)
    }
    fn check_directory(&self) -> io::Result<()> {
        let named = fs::symlink_metadata(&self.root)?;
        if !named.is_dir() || identity(&named) != self.tuple || identity(&self.held.metadata()?) != self.tuple
            || self.root.canonicalize()? != self.root {
            return Err(invalid("Actual gate directory moved or was substituted; retained evidence must not be cleaned"));
        }
        let evidence = fs::symlink_metadata(&self.evidence)?;
        if !evidence.is_dir() || identity(&evidence) != identity(&self.held_evidence.metadata()?) {
            return Err(invalid("Actual evidence directory changed; no redirected output is admitted"));
        }
        Ok(())
    }
    fn check(&self) -> io::Result<()> {
        self.check_directory()?;
        if digest(&self.root.join("gate.json"))? != self.metadata_digest {
            return Err(invalid("Actual prerequisite metadata changed"));
        }
        for entry in array(&self.metadata, "files")? {
            let path = Path::new(string(entry, "path")?);
            if !normal_absolute(path) || !(path.starts_with(&self.root) || path.starts_with(&self.product)) {
                return Err(invalid("Actual Source/bin evidence escaped its declared physical aperture"));
            }
            let meta = fs::symlink_metadata(path)?;
            if entry["dev"].as_u64() != Some(meta.dev()) || entry["ino"].as_u64() != Some(meta.ino())
                || digest(path)? != string(entry, "sha256")? {
                return Err(invalid("Actual Source/lock/input/binary basis changed; comparison is unavailable"));
            }
        }
        for entry in array(&self.metadata, "symlinks")? {
            let path = Path::new(string(entry, "path")?);
            if !normal_absolute(path) || !(path.starts_with(&self.root) || path.starts_with(&self.product))
                || !fs::symlink_metadata(path)?.file_type().is_symlink()
                || fs::read_link(path)? != Path::new(string(entry, "target")?) {
                return Err(invalid("Actual archived Source link changed"));
            }
        }
        Ok(())
    }
    fn retained_fixture(&self, role: &str, case: &str) -> io::Result<Value> {
        self.check_directory()?;
        let binding = &self.metadata["executables"][role];
        let source_root = Path::new(string(binding, "cwd")?);
        let scratch = source_root.join("ProjectCentral/now/tmp").canonicalize()?;
        if !scratch.starts_with(source_root) { return Err(invalid("Retained fixture escaped actual compiled Source")); }
        let mut selected = Vec::new();
        for entry in fs::read_dir(&scratch)? {
            let entry = entry?;
            let name = entry.file_name();
            if !name.to_str().is_some_and(|name| name.starts_with("characterization-retained-")) { continue; }
            let root = entry.path();
            let named = fs::symlink_metadata(&root)?;
            if !named.is_dir() || root.canonicalize()? != root { return Err(invalid("Retained native fixture lost physical custody")); }
            let held = OpenOptions::new().read(true)
                .custom_flags((OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC).bits() as i32).open(&root)?;
            let before_path = root.join("characterization-before.json");
            let before = read_json(&before_path)?;
            if before["case"] != case { continue; }
            if before["schema"] != "aikit.characterization-owned-fixture/v2"
                || string(&before, "root_path")? != root.to_str().ok_or_else(|| invalid("Owned fixture coordinate must be UTF8"))?
                || Path::new(string(&before,"source_manifest_dir")?) != source_root.join("crates/aikit-cli")
                || before["root_dev"].as_u64() != Some(named.dev()) || before["root_ino"].as_u64() != Some(named.ino())
                || identity(&held.metadata()?) != identity(&named) {
                return Err(invalid("Actual pre-effect fixture and compiled source affiliation do not match"));
            }
            let payload_path = root.join("payload/run.sh");
            let mut payload = open_regular(&payload_path)?;
            let metadata = payload.metadata()?;
            let mut bytes = Vec::new();
            (&mut payload).take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
            if bytes.len() > 1024 * 1024 || before["payload_dev"].as_u64() != Some(metadata.dev())
                || before["payload_ino"].as_u64() != Some(metadata.ino()) || before["payload_mode"].as_u64() != Some(u64::from(metadata.mode()))
                || before["payload_bytes"].as_u64() != Some(bytes.len() as u64)
                || string(&before,"payload_blake3")? != blake3::hash(&bytes).to_hex().as_str()
                || identity(&fs::symlink_metadata(&payload_path)?) != identity(&metadata)
                || identity(&fs::symlink_metadata(&root)?) != identity(&named) {
                return Err(invalid("Actual retained fixture payload changed after native operation/unwind"));
            }
            let outcome_path = root.join("characterization-outcome.json");
            let outcome = read_json(&outcome_path)?;
            if outcome["case"] != case { return Err(invalid("Actual retained native outcome belongs to another fixture")); }
            let unwind = if case == SENSITIVITY_CASE { Some(read_json(&root.join("characterization-unwind.json"))?) } else { None };
            selected.push(json!({"before":before,"actual_outcome":outcome,"actual_unwind":unwind,
                "before_sha256":digest(&before_path)?,"outcome_sha256":digest(&outcome_path)?,
                "custody_scope":"same native owned fixture remains; deliberately no automatic/path cleanup; not semantic World identity"}));
        }
        if selected.len() != 1 { return Err(invalid("Exactly one actual retained fixture must witness each native selection")); }
        self.check_directory()?;
        Ok(selected.remove(0))
    }
    fn write(&self, label: &str, bytes: &[u8]) -> io::Result<()> {
        self.check_directory()?;
        if label.is_empty() || !label.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b)) {
            return Err(invalid("Only the declared internal evidence name is admitted"));
        }
        let mut file = OpenOptions::new().write(true).create_new(true).mode(0o600)
            .custom_flags((OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC).bits() as i32).open(self.evidence.join(label))?;
        file.write_all(bytes)?;
        file.sync_all()?;
        self.check_directory()
    }
    fn run(&self, role: &str, label: &str, args: &[&str]) -> Result<Output, Box<dyn Error>> {
        self.check()?;
        let binding = self.metadata["executables"].get(role).ok_or_else(|| invalid("Actual compiled executable is missing"))?;
        let exe = Path::new(string(binding, "path")?);
        if !exe.starts_with(&self.root) || digest(exe)? != string(binding, "sha256")? {
            return Err(invalid("Actual retained CompilerArtifact binary changed").into());
        }
        let expected_head = if role == "old" { OLD_HEAD } else { string(&self.metadata,"current_head")? };
        if string(binding,"source_head")? != expected_head { return Err(invalid("Actual binary belongs to another Source cut").into()); }
        let cwd = Path::new(string(binding, "cwd")?);
        if !normal_absolute(cwd) || cwd.canonicalize()? != cwd || !(cwd.starts_with(&self.root) || cwd == self.product) {
            return Err(invalid("Actual compiled test source coordinate is unavailable").into());
        }
        let mut argv = vec![exe.to_str().ok_or_else(|| invalid("Native executable coordinate is not UTF8"))?.to_owned()];
        argv.extend(args.iter().map(|s| (*s).to_owned()));
        let runner = SystemRunner::new().with_cwd(cwd).with_strict_utf8().with_body_free_diagnostics()
            .with_env_removed("CENTRAL_ROOT").with_env_removed("CENTRAL_NATIVE_TOKEN")
            .with_env_removed("AIKIT_HOME").with_env_removed("AIKIT_CONTEXT_ID");
        // The native owner alone captures/cancels/reaps. An error is never retried
        // or reinterpreted as the expected semantic failure of the old owner.
        let result = runner.run_with_limits(&argv, Duration::from_secs(20), 1024 * 1024, true);
        match result {
            Ok(output) => {
                let retention = (|| -> io::Result<()> {
                self.write(&format!("{label}.stdout"), output.stdout.as_bytes())?;
                self.write(&format!("{label}.stderr"), output.stderr.as_bytes())?;
                self.write(&format!("{label}.receipt.json"), &serde_json::to_vec_pretty(&json!({
                    "role":role,"native_completed_capture":true,"status":output.status,
                    "stdout_bytes":output.stdout.len(),"stderr_bytes":output.stderr.len(),
                    "binary_sha256":string(binding,"sha256")?,"source_head":string(binding,"source_head")?,
                    "strict_utf8":true,"requested_live_seconds":20,"aggregate_bytes":1048576,
                    "separate_native_retirement_seconds":2})).map_err(io::Error::other)?)?;
                self.check()
                })();
                if let Err(cause) = retention {
                    // The actual completed capture survives an evidence IO fault;
                    // no semantic result or cleanup receipt is reconstructed.
                    return Err(Box::new(AikitError::new("test.capture_evidence_unavailable", "Actual completed capture could not be retained")
                        .with_io_source(cause).with_native_capture(Some(output.status), output.stdout.into_bytes(), output.stderr.into_bytes())));
                }
                Ok(output)
            }
            Err(failure) => {
                let retention = (|| -> io::Result<()> {
                if let Some(actual) = failure.native_capture() {
                    self.write(&format!("{label}.partial.stdout"), &actual.stdout)?;
                    self.write(&format!("{label}.partial.stderr"), &actual.stderr)?;
                }
                let primary = failure.source().and_then(|e| e.downcast_ref::<io::Error>())
                    .map(|e| json!({"kind":format!("{:?}",e.kind()),"errno":e.raw_os_error()}));
                let secondary: Vec<_> = failure.secondary_io_sources()
                    .map(|e| json!({"kind":format!("{:?}",e.kind()),"errno":e.raw_os_error()})).collect();
                let keys = ["execution_started","effects","direct_child_reaped","leader_ownership",
                    "stdout_eof","stderr_eof","observation_stage","cleanup_status","cleanup_error_kind",
                    "output_bytes","stdout_bytes","stderr_bytes"];
                let details: serde_json::Map<String, Value> = keys.into_iter().filter_map(|key|
                    failure.details().get(key).map(|actual| (key.to_owned(), json!(actual)))).collect();
                self.write(&format!("{label}.refusal.json"), &serde_json::to_vec_pretty(&json!({
                    "role":role,"code":failure.code(),"actual_details":details,"primary_io":primary,
                    "secondary_io":secondary,"actual_capture_status":failure.native_capture().and_then(|c|c.status),
                    "native_completed_capture":false,"semantic_comparison":"unavailable","retry":false})).map_err(io::Error::other)?)?;
                self.check()
                })();
                let failure = match retention {
                    Ok(()) => failure,
                    Err(cause) => {
                        let supplemental = AikitError::new("test.capture_evidence_unavailable", "Actual failure evidence could not be retained").with_io_source(cause);
                        failure.with_secondary_io_source_from(&supplemental)
                    }
                };
                Err(Box::new(failure))
            }
        }
    }
}

fn roster(output: &Output) -> io::Result<BTreeSet<String>> {
    if output.status != 0 { return Err(invalid("Compiled native roster command failed")); }
    Ok(output.stdout.lines().filter_map(|s|s.strip_suffix(": test").map(str::to_owned)).collect())
}
fn summary(output: &Output, state: &str, passed: usize, failed: usize) -> io::Result<String> {
    let summaries: Vec<_> = output.stdout.lines().chain(output.stderr.lines())
        .filter(|s|s.starts_with("test result: ")).collect();
    let prefix = format!("test result: {state}. {passed} passed; {failed} failed; 0 ignored; 0 measured;");
    if summaries.len() != 1 || !summaries[0].starts_with(&prefix)
        || (output.status == 0) != (failed == 0) {
        return Err(invalid("Actual libtest result was not the exact unskipped native selection"));
    }
    Ok(summaries[0].to_owned())
}
fn clean_inner_retirement(actual: &Value) -> bool {
    if actual["native_outcome"] != "refused" || actual["execution_started"] != "true"
        || actual["direct_child_reaped"] != "true" || actual["original_source_present"] != false
        || !actual["cleanup_cause"].is_null() || !actual["additional_cleanup_causes"].is_null() {
        return false;
    }
    let known = actual["known_exit_status"].as_i64();
    let cleanup = actual["cleanup_exit_status"].as_i64();
    if known.is_some() == cleanup.is_some() || known.or(cleanup).is_none_or(|status| status < 0) {
        return false;
    }
    match actual["group_signal"].as_str() {
        Some("delivered") => actual["group_absence_cause"].is_null(),
        // Native AlreadyAbsent is a real non-failure observation. It still
        // needs its original cause, successful actual reap and exit status.
        Some("already-absent") => actual["group_absence_cause"].is_object(),
        Some("not-needed") => actual["group_absence_cause"].is_null()
            && actual["stdout_eof"] == true && actual["stderr_eof"] == true,
        _ => false,
    }
}

fn observation(output: &Output) -> io::Result<Value> {
    let prefix = "AIKIT_CAPTURE_NATIVE_OBSERVATION ";
    let observations: Vec<_> = output.stdout.lines().chain(output.stderr.lines())
        .filter_map(|line|line.find(prefix).map(|i|&line[i+prefix.len()..])).collect();
    if observations.len() != 1 { return Err(invalid("Exactly one actual native outcome is required")); }
    serde_json::from_str(observations[0]).map_err(io::Error::other)
}

#[test]
#[ignore = "requires both actual complete source archives, locked builds and admitted CompilerArtifacts from existing hosted CI"]
fn actual_old_owner_negative_and_current_owner_positive_use_same_native_capture() -> Result<(), Box<dyn Error>> {
    let gate = Gate::admit()?;
    let mut facts = Vec::new();
    for role in ["old", "new"] {
        let listed = gate.run(role, &format!("{role}-compiled2"), &["--list"])?;
        let ignored = gate.run(role, &format!("{role}-ignored"), &["--ignored", "--list"])?;
        let available = roster(&listed)?;
        let ignored = roster(&ignored)?;
        if available.len() != 6 || !available.contains(SENSITIVITY_CASE) || ignored.contains(SENSITIVITY_CASE)
            || !CASES.iter().all(|case|available.contains(*case) && !ignored.contains(*case)) {
            return Err(invalid("Both genuine characterization definitions must be compiled and unignored").into());
        }
        for (index, case) in CASES.iter().enumerate() {
            let output = gate.run(role, &format!("{role}-{index}"), &[*case,"--exact","--nocapture","--test-threads=1"])?;
            let actual = observation(&output)?;
            if actual["case"] != *case { return Err(invalid("Native outcome belongs to a different selection").into()); }
            let retained = gate.retained_fixture(role,case)?;
            if retained["actual_outcome"] != actual { return Err(invalid("Actual native observation and retained fixture disagree").into()); }
            let result = if role == "old" {
                if actual["native_outcome"] != "completed" || actual["status"] != 0 || actual["detached"] != false
                    || actual["known_control_matches"] != true || actual["output_rows"] != if index == 0 { 1 } else { 65_537 } {
                    return Err(invalid("Old negative lacks the actual completed controlled operation").into());
                }
                summary(&output,"FAILED",0,1)?
            } else {
                if !clean_inner_retirement(&actual)
                    || actual["cause_observation_contract"] != "expected_command_failure_original_and_structured_cleanup/v2"
                    || (index == 0 && actual["code"] != "mux.command_timeout")
                    || (index == 1 && (actual["code"] != "mux.command_output_limit" || actual["observation_stage"] != "line_projection_capacity"
                        || !actual["stdout_eof"].is_boolean() || !actual["stderr_eof"].is_boolean())) {
                    return Err(invalid("Current positive lacks the actual native refusal facts").into());
                }
                summary(&output,"ok",1,0)?
            };
            facts.push(json!({"role":role,"case":case,"actual_native_outcome":actual,"actual_retained_fixture":retained,"actual_libtest_result":result,"status":output.status}));
        }
    }
    let output = gate.run("new","new-real-failure-unwind", &[SENSITIVITY_CASE,"--exact","--nocapture","--test-threads=1"])?;
    let actual = observation(&output)?;
    let retained = gate.retained_fixture("new",SENSITIVITY_CASE)?;
    if actual["case"] != SENSITIVITY_CASE || actual["native_outcome"] != "refused"
        || actual["code"] != "mux.command_spawn_failed" || actual["execution_started"] != "false"
        || actual["original_source_present"] != true || actual["original_io"]["kind"] != "NotFound"
        || clean_inner_retirement(&actual) || retained["actual_outcome"] != actual
        || retained["actual_unwind"]["case"] != SENSITIVITY_CASE
        || retained["actual_unwind"]["actual_assertion_unwind_observed"] != true
        || retained["actual_unwind"]["fixture_verified_after_unwind"] != true
        || retained["actual_unwind"]["native_kind"] != actual["original_io"]["kind"]
        || retained["actual_unwind"]["native_errno"] != actual["original_io"]["raw_os_error"] {
        return Err(invalid("Genuine native failure/unwind did not preserve its actual IO and owned fixture").into());
    }
    facts.push(json!({"role":"new","scope":"genuine spawn failure/fixture unwind sensitivity, not forced cleanup syscall failure",
        "case":SENSITIVITY_CASE,"actual_native_outcome":actual,"actual_retained_fixture":retained,
        "actual_libtest_result":summary(&output,"ok",1,0)?,"status":output.status}));
    let groups: [(&str, &[&str], usize); 5] = [
        ("runner", &RUNNER_CASES, 0), ("run_exec", &RUN_CASES, 13),
        ("multicall_symlink", &EXPORT_CASES, 7), ("palette_run_intent", &[], 1), ("scoped_praxis_native", &[], 1),
    ];
    let mut current_count = 0;
    for (role, required, whole_count) in groups {
        let all = roster(&gate.run(role, &format!("{role}-list"), &["--list"])?)?;
        let ignored = roster(&gate.run(role, &format!("{role}-ignored"), &["--ignored","--list"])?)?;
        if !required.iter().all(|case|all.contains(*case) && !ignored.contains(*case))
            || (whole_count != 0 && all.len() != whole_count) {
            return Err(invalid("Actual current compiled native target roster is incomplete or changed").into());
        }
        for (index, case) in required.iter().enumerate() {
            let output = gate.run(role, &format!("{role}-case-{index}"), &[*case,"--exact","--nocapture","--test-threads=1"])?;
            let result = summary(&output,"ok",1,0)?;
            facts.push(json!({"role":role,"case":case,"actual_libtest_result":result,"status":output.status}));
            current_count += 1;
        }
        if whole_count != 0 {
            let output = gate.run(role, &format!("{role}-whole"), &["--nocapture","--test-threads=1"])?;
            let result = summary(&output,"ok",whole_count,0)?;
            facts.push(json!({"role":role,"scope":"whole native target, including inherited regressions","actual_libtest_result":result,"status":output.status}));
        }
    }
    if current_count != 17 { return Err(invalid("Current native seventeen-case execution census differs").into()); }
    let runner_roster = roster(&gate.run("runner","lifecycle-list", &["--list"])?)?;
    let runner_ignored = roster(&gate.run("runner","lifecycle-ignored", &["--ignored","--list"])?)?;
    for (index, case) in LIFECYCLE_CASES.iter().enumerate() {
        if !runner_roster.contains(*case) || runner_ignored.contains(*case) {
            return Err(invalid("Existing actual native lifecycle selection is missing or ignored").into());
        }
        let output = gate.run("runner", &format!("lifecycle-{index}"), &[*case,"--exact","--nocapture","--test-threads=1"])?;
        facts.push(json!({"scope":"existing owner lifecycle","case":case,"actual_libtest_result":summary(&output,"ok",1,0)?,"status":output.status}));
    }
    gate.check()?;
    gate.write("actual-results.json", &serde_json::to_vec_pretty(&json!({
        "old_characterizations":2,"new_characterizations":2,"current_unique_required_definitions":current_count,
        "whole_native_targets":4,"existing_lifecycle_definitions":LIFECYCLE_CASES.len(),"genuine_failure_unwind_sensitivity":1,"actual_results":facts,
        "scope":"actual hosted locked Source/native Linux-or-Mac qualification; no installed/H/other-platform claim"}))?)?;
    // No path-based sweeper or fixture teardown while a failed child may exist.
    // The same native owner has retired each successful capture; public evidence
    // and source archives remain owned for always-upload and host job disposal.
    Ok(())
}
