//! Per-context advisory locks.
//!
//! There is no daemon. Two `aikit apply` invocations in two panes of the same
//! session are two ordinary processes racing for one context directory, and the
//! only thing standing between them is this file lock plus the compare-and-swap
//! in [`crate::generation`].
//!
//! ## Why the lock file carries a holder record
//!
//! `flock` tells a contender that it lost; it does not tell it to whom. "aikit:
//! resource busy" is an unactionable message — the user cannot tell whether to
//! wait, to look in another pane, or to clean up after a crash. So the holder
//! writes `pid`, `host`, `purpose` and a timestamp into the file it has just
//! locked, and a contender reads that back to produce `lock.busy` with a name
//! attached. The record is advisory: a stale one from a process that died is
//! harmless, because the *lock* is held by the kernel and released on exit
//! whatever the file says.
//!
//! ## Why polling rather than a blocking lock
//!
//! `flock` with `LOCK_EX` blocks indefinitely and cannot be given a deadline. A
//! palette that hangs forever because another pane is mid-apply is worse than one
//! that says "busy: pid 4131 is applying". So the wait is a bounded poll.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fs4::FileExt;

use aikit_core::{AikitError, ContextId, Result};

use crate::events::Timestamp;
use crate::home::{create_dir_all, io_error};
use crate::AikitHome;

/// How long to wait, and what to say we are doing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockOptions {
    pub timeout: Duration,
    pub purpose: String,
    /// How often to retry while waiting.
    pub poll_interval: Duration,
}

impl Default for LockOptions {
    fn default() -> Self {
        Self {
            // Long enough to cover a normal apply in another pane, short enough
            // that a wedged process does not become an indefinite hang.
            timeout: Duration::from_secs(10),
            purpose: "unspecified".to_string(),
            poll_interval: Duration::from_millis(20),
        }
    }
}

impl LockOptions {
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    #[must_use]
    pub fn with_purpose(mut self, purpose: impl Into<String>) -> Self {
        self.purpose = purpose.into();
        self
    }
}

/// Who holds a lock, as recorded in the lock file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holder {
    pub pid: u32,
    pub host: String,
    pub purpose: String,
    pub since: String,
}

impl Holder {
    /// The phrase the busy error puts in front of the user.
    pub fn describe(&self) -> String {
        format!(
            "pid {} on {} ({}), since {}",
            self.pid, self.host, self.purpose, self.since
        )
    }

    fn render(&self) -> String {
        format!(
            "pid = {}\nhost = \"{}\"\npurpose = \"{}\"\nsince = \"{}\"\n",
            self.pid, self.host, self.purpose, self.since
        )
    }

    fn parse(raw: &str) -> Option<Self> {
        let mut pid = None;
        let mut host = String::new();
        let mut purpose = String::new();
        let mut since = String::new();
        for line in raw.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim().trim_matches('"').to_string();
            match key.trim() {
                "pid" => pid = value.parse().ok(),
                "host" => host = value,
                "purpose" => purpose = value,
                "since" => since = value,
                _ => {}
            }
        }
        Some(Self {
            pid: pid?,
            host,
            purpose,
            since,
        })
    }
}

/// Read the holder record of a lock file, if there is a legible one.
pub fn read_holder(path: &Path) -> Option<Holder> {
    let mut contents = String::new();
    File::open(path).ok()?.read_to_string(&mut contents).ok()?;
    Holder::parse(&contents)
}

/// An acquired lock. Releasing happens on drop.
pub struct ContextLock {
    file: File,
    path: PathBuf,
    key: String,
}

impl ContextLock {
    /// Take the lock named `key` under `<home>/state/locks/`.
    pub fn acquire(home: &AikitHome, key: &str, options: LockOptions) -> Result<Self> {
        create_dir_all(&home.locks())?;
        Self::acquire_at(&home.lock_file(key), key, options)
    }

    /// Take the lock guarding one context.
    pub fn for_context(
        home: &AikitHome,
        context: &ContextId,
        options: LockOptions,
    ) -> Result<Self> {
        Self::acquire(home, context.as_str(), options)
    }

    /// The general form, for callers that own the path.
    pub fn acquire_at(path: &Path, key: &str, options: LockOptions) -> Result<Self> {
        if let Some(parent) = path.parent() {
            create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|e| io_error("lock.unavailable", path, &e))?;

        let deadline = Instant::now() + options.timeout;
        loop {
            // Fully qualified: `std::fs::File` grew its own inherent `try_lock`,
            // which would otherwise shadow the trait method and change the error type.
            match FileExt::try_lock(&file) {
                Ok(()) => break,
                Err(fs4::TryLockError::WouldBlock) => {
                    if Instant::now() >= deadline {
                        return Err(busy_error(path, key, &options));
                    }
                    std::thread::sleep(options.poll_interval.min(Duration::from_millis(50)));
                }
                Err(fs4::TryLockError::Error(e)) => {
                    return Err(io_error("lock.unavailable", path, &e))
                }
            }
        }

        let mut locked = Self {
            file,
            path: path.to_path_buf(),
            key: key.to_string(),
        };
        locked.stamp(&options.purpose)?;
        Ok(locked)
    }

    /// Record who we are, now that the lock is ours.
    fn stamp(&mut self, purpose: &str) -> Result<()> {
        let holder = Holder {
            pid: std::process::id(),
            host: hostname(),
            purpose: purpose.to_string(),
            since: Timestamp::now().to_string(),
        };
        self.file
            .set_len(0)
            .map_err(|e| io_error("lock.unavailable", &self.path, &e))?;
        self.file
            .seek(SeekFrom::Start(0))
            .map_err(|e| io_error("lock.unavailable", &self.path, &e))?;
        self.file
            .write_all(holder.render().as_bytes())
            .map_err(|e| io_error("lock.unavailable", &self.path, &e))?;
        self.file
            .flush()
            .map_err(|e| io_error("lock.unavailable", &self.path, &e))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn key(&self) -> &str {
        &self.key
    }
}

impl Drop for ContextLock {
    fn drop(&mut self) {
        // Best effort: the descriptor closing releases the lock regardless, so an
        // error here has no consequence worth propagating out of a destructor.
        let _ = FileExt::unlock(&self.file);
    }
}

impl std::fmt::Debug for ContextLock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContextLock")
            .field("key", &self.key)
            .field("path", &self.path)
            .finish()
    }
}

fn busy_error(path: &Path, key: &str, options: &LockOptions) -> AikitError {
    let holder = read_holder(path);
    let described = holder
        .as_ref()
        .map(Holder::describe)
        .unwrap_or_else(|| "an unidentified process".to_string());

    let mut error = AikitError::new(
        "lock.busy",
        format!(
            "`{key}` is locked by {described}; waited {} ms for it",
            options.timeout.as_millis()
        ),
    )
    .with("key", key)
    .with("path", path.display().to_string())
    .with("holder", described)
    .with("waited_ms", options.timeout.as_millis().to_string());

    if let Some(holder) = holder {
        error = error
            .with("pid", holder.pid.to_string())
            .with("host", holder.host)
            .with("purpose", holder.purpose)
            .with("since", holder.since);
    }
    error
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("HOST"))
        .unwrap_or_else(|_| "localhost".to_string())
}
