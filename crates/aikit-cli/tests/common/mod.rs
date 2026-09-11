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
            std::process::Command::new("id").arg("-u").output().ok()?.stdout,
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
