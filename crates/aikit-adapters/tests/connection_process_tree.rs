//! Real OS lifecycle regression. These are ordinary shell process trees, not ACP
//! providers or simulated protocol acceptance. Group escape is out of scope.
#![cfg(any(target_os = "linux", target_os = "macos"))]

use aikit_adapters::connection_process::ConnectionProcess;
use std::{
    fs,
    path::Path,
    process::{Child, Command},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

struct Unrelated(Child);
impl Drop for Unrelated {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn argv(root: &Path, exit_leader: bool) -> Vec<String> {
    let script = r#"
/bin/sh -c 'sleep 300 & echo "$$ $!" > "$1/descendants"; wait' sh "$1" &
echo "$$ $!" > "$1/leader"
while [ ! -s "$1/descendants" ]; do sleep 0.01; done
if [ "$2" = exit ]; then exit 7; fi
wait
"#;
    vec![
        "/bin/sh".into(),
        "-c".into(),
        script.into(),
        "sh".into(),
        root.display().to_string(),
        if exit_leader { "exit" } else { "wait" }.into(),
    ]
}

fn pids(root: &Path) -> Vec<u32> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let (Ok(a), Ok(b)) = (
            fs::read_to_string(root.join("leader")),
            fs::read_to_string(root.join("descendants")),
        ) {
            let values: Vec<_> = a
                .split_whitespace()
                .chain(b.split_whitespace())
                .filter_map(|v| v.parse().ok())
                .collect();
            if values.len() == 4 {
                return values;
            }
        }
        assert!(Instant::now() < deadline, "real descendants did not start");
        thread::sleep(Duration::from_millis(10));
    }
}

fn running(pid: u32) -> bool {
    let output = Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    let stat = String::from_utf8_lossy(&output.stdout);
    // Non-child zombies are already dead and belong to the OS reaper. The
    // transport reaps its own direct child; it cannot waitpid grandchildren.
    !stat.trim().is_empty() && !stat.trim().starts_with('Z')
}

fn assert_stopped(pids: &[u32]) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while pids.iter().any(|pid| running(*pid)) {
        assert!(
            Instant::now() < deadline,
            "owned descendant still running: {pids:?}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn assert_leader_reaped(pid: u32) {
    assert_eq!(
        rustix::process::test_kill_process(rustix::process::Pid::from_raw(pid as i32).unwrap()),
        Err(rustix::io::Errno::SRCH),
        "direct child must be reaped, not left as a zombie"
    );
}

#[test]
fn split_termination_kills_descendants_closes_pipe_and_spares_unrelated_process() {
    let root = tempfile::tempdir().unwrap();
    let mut unrelated = Unrelated(Command::new("sleep").arg("300").spawn().unwrap());
    let (_writer, mut reader, control) =
        ConnectionProcess::spawn_split(&argv(root.path(), false), None).unwrap();
    let ids = pids(root.path());
    assert!(ids.iter().all(|pid| running(*pid)));
    let (tx, rx) = mpsc::channel();
    let read = thread::spawn(move || {
        tx.send(reader.read_line()).unwrap();
    });
    let status = control.terminate().unwrap().unwrap();
    assert!(!status.success());
    assert_eq!(control.terminate().unwrap(), Some(status));
    assert!(
        rx.recv_timeout(Duration::from_secs(5)).unwrap().is_err(),
        "all inherited stdout writers must close"
    );
    read.join().unwrap();
    assert_stopped(&ids);
    assert_leader_reaped(ids[0]);
    assert!(unrelated.0.try_wait().unwrap().is_none());
}

#[test]
fn split_drop_cleans_descendants_after_leader_exit_was_observed() {
    let root = tempfile::tempdir().unwrap();
    let (_writer, _reader, control) =
        ConnectionProcess::spawn_split(&argv(root.path(), true), None).unwrap();
    let ids = pids(root.path());
    let deadline = Instant::now() + Duration::from_secs(5);
    while control.is_running().unwrap() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        running(ids[2]) && running(ids[3]),
        "leader exit must leave a real live descendant case"
    );
    // Repeated status reads must not reap the leader and discard group ownership.
    assert!(!control.is_running().unwrap());
    drop(control);
    assert_stopped(&ids);
    assert_leader_reaped(ids[0]);
}

#[test]
fn unsplit_drop_and_explicit_termination_clean_the_same_real_tree() {
    for explicit in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mut process = ConnectionProcess::spawn(&argv(root.path(), false), None).unwrap();
        let ids = pids(root.path());
        if explicit {
            process.terminate().unwrap();
            assert!(!process.is_running().unwrap());
        }
        drop(process);
        assert_stopped(&ids);
        assert_leader_reaped(ids[0]);
    }
}
