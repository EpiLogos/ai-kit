//! Locate one already-journaled Prime session through its installed native
//! configuration. Never enumerate other sessions or read their conversation.
use aikit_adapters::connection_process::ModelEnvironment;
use aikit_adapters::runner::{Output, SystemRunner};
use aikit_core::{AikitError, Result};
use serde_json::Value;
use std::{
    path::Path,
    process::{Command, Stdio},
    time::Duration,
};

fn refused(reason: impl ToString) -> AikitError {
    AikitError::new("encounter.prime_resume_basis", reason.to_string())
}
fn refused_io(cause: std::io::Error) -> AikitError {
    let failure = refused(&cause);
    failure.with_io_source(cause)
}

/// The existing native process owner retains actual child/capture/cleanup
/// evidence. This wrapper adds domain context, not a second supervisor.
fn capture_locator(command: &mut Command) -> Result<Output> {
    SystemRunner::new()
        .with_timeout(Duration::from_secs(5))
        .with_output_limit_bytes(32768)
        .with_strict_utf8()
        .with_body_free_diagnostics()
        .capture_command(command)
        .map_err(|cause| {
            let mut failure = refused("Native Prime session locator was not confirmed")
                .with("native_failure_code", cause.code())
                .with_io_source_from(&cause)
                .with_private_native_cause(&cause);
            // These are this explicitly body-free native owner's actual
            // lifecycle/cause facts, not values derived from a guessed status.
            for (key, value) in cause.details() {
                failure = failure.with(key, value);
            }
            failure
        })
}

pub(super) fn locate(
    argv: &[String],
    native: &str,
    cwd: &Path,
    environment: Option<&ModelEnvironment>,
) -> Result<String> {
    if native.len() != 36
        || native.bytes().enumerate().any(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b != b'-'
            } else {
                !b.is_ascii_hexdigit()
            }
        })
    {
        return Err(refused("Prime resume needs the exact recorded native UUID"));
    }
    let entries: Vec<_> = argv.windows(2).filter(|p| p[0] == "--prime-bin").collect();
    if entries.len() != 1 {
        return Err(refused(
            "Prime resume requires one admitted native --prime-bin executable",
        ));
    }
    if cwd.to_str().is_none() {
        return Err(refused("Native Node locator cannot represent this cwd without replacement")
            .with("observation_stage", "locator_argument_encoding")
            .with("execution_started", "false"));
    }
    let executable = std::fs::canonicalize(&entries[0][1]).map_err(refused_io)?;
    let dist = executable
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| refused("Prime package path is unavailable"))?;
    let config = dist.join("config.js");
    let package = dist
        .parent()
        .ok_or_else(|| refused("Prime package root is unavailable"))?
        .join("package.json");
    let release: Value =
        serde_json::from_slice(&std::fs::read(package).map_err(refused_io)?).map_err(refused)?;
    if release["version"] != aikit_adapters::prime_rpc_connection::PRIME_AGENT_RELEASE {
        return Err(refused(
            "Prime session filename contract is not verified for this installed release",
        ));
    }
    // getSessionsDir is Prime's own read-only configuration route. The native
    // 0.9.4 SessionManager stores <UUID>.jsonl in that directory. Read only the
    // selected file's bounded first header; RPC switch_session performs reopen.
    const SCRIPT: &str = r#"
import {pathToFileURL} from 'node:url';
import fs from 'node:fs';
import path from 'node:path';
const [config,id,cwd]=process.argv.slice(1);
const {getSessionsDir}=await import(pathToFileURL(config).href);
const file=path.resolve(getSessionsDir(),`${id}.jsonl`);
const stat=fs.lstatSync(file);
if(!stat.isFile()||stat.isSymbolicLink())throw Error('Native session must be a regular file');
const fd=fs.openSync(file,'r');let header;
try { const b=Buffer.alloc(65536);const n=fs.readSync(fd,b,0,b.length,0);const end=b.subarray(0,n).indexOf(10);if(end<0)throw Error('Native session header exceeds bound');header=JSON.parse(b.subarray(0,end).toString('utf8')); } finally { fs.closeSync(fd); }
if(header.type!=='session'||header.id!==id||typeof header.cwd!=='string'||fs.realpathSync(header.cwd)!==fs.realpathSync(cwd))throw Error('Native session header differs from the recorded id/cwd');
process.stdout.write(JSON.stringify({file,id,cwd:fs.realpathSync(cwd)}));
"#;
    if config.to_str().is_none() {
        return Err(refused("Native Node locator cannot represent this config path without replacement")
            .with("observation_stage", "locator_argument_encoding")
            .with("execution_started", "false"));
    }
    let mut command = Command::new("node");
    command
        .args(["--input-type=module", "--eval", SCRIPT])
        .arg(&config)
        .arg(native)
        .arg(cwd)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(environment) = environment {
        environment.apply(&mut command);
    }
    selected_file(capture_locator(&mut command)?, native)
}

fn selected_file(output: Output, native: &str) -> Result<String> {
    if !output.ok() {
        let status = output.status;
        return Err(refused("Prime did not confirm the recorded native session header and cwd")
            .with("locator_status", status.to_string())
            .with_native_capture(Some(status), output.stdout.into_bytes(), output.stderr.into_bytes()));
    }
    let found: Value = serde_json::from_str(&output.stdout).map_err(refused)?;
    if found["id"] != native {
        return Err(refused("Native locator returned a different identity"));
    }
    found["file"]
        .as_str()
        .filter(|p| Path::new(p).is_absolute())
        .map(str::to_owned)
        .ok_or_else(|| refused("Native locator returned no absolute session file"))
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests {
    use super::*;
    use std::error::Error;
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    use std::time::Instant;

    fn assert_actual_retirement(failure: &AikitError) {
        assert_eq!(failure.details()["direct_child_reaped"], "true");
        let signal = failure.details()["group_signal"].as_str();
        assert!(matches!(signal, "delivered" | "already-absent" | "not-needed"));
        if signal == "not-needed" {
            assert_eq!(failure.details()["stdout_eof"], "true");
            assert_eq!(failure.details()["stderr_eof"], "true");
            assert_eq!(failure.details()["capture_cancelled"], "false");
            let actual = failure.native_capture().unwrap().status.unwrap().to_string();
            assert_eq!(failure.details()["known_exit_status"], actual);
        }
        assert!(!failure.details().contains_key("cleanup_cause"));
        assert!(!failure.details().contains_key("additional_cleanup_causes"));
        // ESRCH is an actual group-absence observation, not an invented
        // completion witness or a reason to discard other cleanup errors.
        for cause in failure.secondary_io_sources() {
            assert_eq!(signal, "already-absent");
            assert_eq!(cause.raw_os_error(), Some(3));
        }
    }

    #[test]
    fn actual_prime_resume_bad_material_path_retains_original_os_cause() {
        assert!(std::fs::metadata("/dev/null").unwrap().file_type().is_char_device());
        let failure = locate(
            &["--prime-bin".into(), "/dev/null/prime-locator".into()],
            "5c347cc8-4926-42cf-919c-1e892681c6a8",
            Path::new("/"),
            None,
        ).unwrap_err();
        assert_eq!(failure.code(), "encounter.prime_resume_basis");
        let cause = failure.source().unwrap().downcast_ref::<std::io::Error>().unwrap();
        assert_eq!(cause.kind(), std::io::ErrorKind::NotADirectory);
        assert_eq!(cause.raw_os_error(), Some(20));
        assert!(failure.native_capture().is_none());
        assert!(failure.private_native_cause().is_none());
    }

    #[test]
    fn actual_native_locator_spawn_failure_retains_same_original_cause() {
        assert!(std::fs::metadata("/dev/null").unwrap().file_type().is_char_device());
        let mut command = Command::new("/dev/null/prime-locator");
        let failure = capture_locator(&mut command).unwrap_err();
        assert_eq!(failure.code(), "encounter.prime_resume_basis");
        assert_eq!(failure.details()["native_failure_code"], "mux.command_spawn_failed");
        assert_eq!(failure.details()["execution_started"], "false");
        let cause = failure.source().unwrap().downcast_ref::<std::io::Error>().unwrap();
        let original = failure.private_native_cause().unwrap();
        let original_io = original.source().unwrap().downcast_ref::<std::io::Error>().unwrap();
        assert!(std::ptr::eq(cause, original_io));
        assert_eq!(cause.kind(), std::io::ErrorKind::NotADirectory);
        assert_eq!(cause.raw_os_error(), Some(20));
        assert_eq!(failure.secondary_io_sources().count(), 0);
        assert!(failure.native_capture().is_none());
        assert!(!format!("{failure:?}").contains("/dev/null/prime-locator"));
    }

    #[test]
    fn actual_native_locator_completed_nonzero_keeps_status_and_bounded_streams() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "printf actual-locator-output; printf actual-locator-diagnostic >&2; exit 11"]);
        let output = capture_locator(&mut command).unwrap();
        assert_eq!(output.status, 11);
        assert_eq!(output.stdout, "actual-locator-output");
        assert_eq!(output.stderr, "actual-locator-diagnostic");
        assert!(!output.ok());
        let failure = selected_file(output, "5c347cc8-4926-42cf-919c-1e892681c6a8").unwrap_err();
        assert_eq!(failure.code(), "encounter.prime_resume_basis");
        assert_eq!(failure.details()["locator_status"], "11");
        let capture = failure.native_capture().unwrap();
        assert_eq!(capture.status, Some(11));
        assert_eq!(capture.stdout, b"actual-locator-output");
        assert_eq!(capture.stderr, b"actual-locator-diagnostic");
        assert!(!format!("{failure:?}").contains("actual-locator-diagnostic"));
    }

    #[test]
    fn actual_native_locator_capacity_refusal_retains_real_capture_and_retirement() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "head -c 32769 /dev/zero"]);
        let started = Instant::now();
        let failure = capture_locator(&mut command).unwrap_err();
        assert_eq!(failure.details()["native_failure_code"], "mux.command_output_limit");
        assert_eq!(failure.details()["output_limit_bytes"], "32768");
        assert_eq!(failure.details()["stream"], "stdout");
        assert_actual_retirement(&failure);
        let capture = failure.native_capture().expect("actual retained native bytes");
        assert!(!capture.stdout.is_empty());
        assert!(capture.stdout.len() <= 32768);
        assert!(capture.stdout.iter().all(|byte| *byte == 0));
        assert!(capture.status.is_some());
        assert!(started.elapsed() < Duration::from_secs(8));
    }

    #[test]
    fn actual_native_locator_deadline_retains_partial_bytes_and_inner_retirement() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "printf locator-private-output-canary; printf locator-private-error-canary >&2; sleep 30"]);
        let started = Instant::now();
        let failure = capture_locator(&mut command).unwrap_err();
        assert_eq!(failure.details()["native_failure_code"], "mux.command_timeout");
        assert_eq!(failure.details()["execution_started"], "true");
        assert_actual_retirement(&failure);
        let capture = failure.native_capture().unwrap();
        assert_eq!(capture.stdout, b"locator-private-output-canary");
        assert_eq!(capture.stderr, b"locator-private-error-canary");
        assert!(capture.status.is_some());
        let original = failure.private_native_cause().unwrap();
        assert!(std::ptr::eq(capture, original.native_capture().unwrap()));
        for rendered in [failure.to_string(), format!("{failure:?}")] {
            assert!(!rendered.contains("locator-private-output-canary"));
            assert!(!rendered.contains("locator-private-error-canary"));
        }
        // No post-cleanup EOF is invented from the outer test's completion.
        assert!(started.elapsed() < Duration::from_secs(8));
    }

    #[test]
    fn actual_native_locator_invalid_utf8_retains_real_encoding_cause_and_bytes() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "printf '\\377'"]);
        let failure = capture_locator(&mut command).unwrap_err();
        assert_eq!(failure.details()["native_failure_code"], "mux.command_utf8_invalid");
        assert_eq!(failure.details()["stdout_eof"], "true");
        assert_eq!(failure.details()["stderr_eof"], "true");
        assert_eq!(failure.details()["capture_cancelled"], "false");
        assert_actual_retirement(&failure);
        let cause = failure.source().unwrap().downcast_ref::<std::io::Error>().unwrap();
        assert_eq!(cause.kind(), std::io::ErrorKind::InvalidData);
        assert_eq!(cause.raw_os_error(), None);
        let capture = failure.native_capture().unwrap();
        assert_eq!(capture.status, Some(0));
        assert_eq!(capture.stdout, [255]);
        assert!(capture.stderr.is_empty());
    }

    fn material_basis(path: &Path) -> (u64, u64, u64, u32, i64, i64, i64, i64) {
        let metadata = std::fs::symlink_metadata(path).unwrap();
        assert!(metadata.is_file() && !metadata.file_type().is_symlink());
        (metadata.dev(), metadata.ino(), metadata.len(), metadata.mode(),
         metadata.mtime(), metadata.mtime_nsec(), metadata.ctime(), metadata.ctime_nsec())
    }

    #[test]
    #[ignore = "requires a genuine pinned installed Prime owner-produced, quiescent selected session in admitted Run fixture; no fabricated package/config/header or model prompt"]
    fn actual_installed_prime_locator_reads_same_selected_owner_header_without_transcript_copy() {
        let argv: Vec<String> = serde_json::from_str(
            &std::env::var("AIKIT_PRIME_RESUME_NATIVE_ARGV").expect("actual admitted Prime argv JSON"),
        ).unwrap();
        let native = std::env::var("AIKIT_PRIME_RESUME_NATIVE_ID").expect("actual native owner session UUID");
        let cwd = std::path::PathBuf::from(std::env::var_os("AIKIT_PRIME_RESUME_NATIVE_CWD").expect("actual native owner cwd"));
        let expected = std::path::PathBuf::from(std::env::var_os("AIKIT_PRIME_RESUME_NATIVE_FILE").expect("actual owner-returned selected session file"));
        let root = std::path::PathBuf::from(std::env::var_os("AIKIT_PRIME_RESUME_NATIVE_FIXTURE_ROOT").expect("admitted owned native Run fixture"));
        assert!(root.is_absolute() && expected.is_absolute() && cwd.is_absolute());
        let root_metadata = std::fs::symlink_metadata(&root).unwrap();
        assert!(root_metadata.is_dir() && !root_metadata.file_type().is_symlink());
        let root = root.canonicalize().unwrap();
        assert!(expected.canonicalize().unwrap().starts_with(&root));
        assert!(cwd.canonicalize().unwrap().starts_with(&root));
        let before = material_basis(&expected);
        let result = locate(&argv, &native, &cwd, None).unwrap();
        assert_eq!(Path::new(&result).canonicalize().unwrap(), expected.canonicalize().unwrap());
        assert_eq!(material_basis(&expected), before);
        // This case records only metadata and the selected locator result.
        // The unchanged Node script reads a bounded prefix to find the header;
        // later bytes do not leave Node. No native fixture is removed here.
    }
}
