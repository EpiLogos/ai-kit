//! Locate one already-journaled Prime session through its installed native
//! configuration. Never enumerate other sessions or read their conversation.
use aikit_adapters::connection_process::ModelEnvironment;
use aikit_core::{AikitError, Result};
use serde_json::Value;
use std::{
    io::Read,
    path::Path,
    process::{Command, Stdio},
    time::Duration,
};

fn refused(reason: impl ToString) -> AikitError {
    AikitError::new("encounter.prime_resume_basis", reason.to_string())
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
    let executable = std::fs::canonicalize(&entries[0][1]).map_err(refused)?;
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
        serde_json::from_slice(&std::fs::read(package).map_err(refused)?).map_err(refused)?;
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
    let mut child = command.spawn().map_err(refused)?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| refused("Native locator output unavailable"))?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout.take(32769).read_to_end(&mut bytes).map(|_| bytes);
        let _ = tx.send(result);
    });
    let bytes = match rx.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(bytes)) if bytes.len() <= 32768 => bytes,
        _ => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(refused(
                "Native Prime session locator failed or exceeded its bound",
            ));
        }
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let status = loop {
        match child.try_wait().map_err(refused)? {
            Some(status) => break status,
            None if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10))
            }
            None => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(refused(
                    "Native Prime session locator did not exit within its bound",
                ));
            }
        }
    };
    if !status.success() {
        return Err(refused(
            "Prime did not confirm the recorded native session header and cwd",
        ));
    }
    let found: Value = serde_json::from_slice(&bytes).map_err(refused)?;
    if found["id"] != native {
        return Err(refused("Native locator returned a different identity"));
    }
    found["file"]
        .as_str()
        .filter(|p| Path::new(p).is_absolute())
        .map(str::to_owned)
        .ok_or_else(|| refused("Native locator returned no absolute session file"))
}
