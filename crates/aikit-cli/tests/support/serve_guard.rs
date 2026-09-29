//! A spawned `aikit gateway serve` that cannot outlive the test that started it.
//!
//! A test that panics between spawn and its explicit `wait()` would otherwise
//! orphan the serve process: it keeps running, holds the test's inherited
//! stderr open, and hangs any pipeline reading the test binary's output.
//! Dropping the guard kills and reaps the child; after a happy-path `wait()`
//! the kill is a no-op on an already-exited process.

use std::io;
use std::ops::{Deref, DerefMut};
use std::process::{Child, Output};

pub struct ServeGuard(Option<Child>);

impl ServeGuard {
    pub fn new(child: Child) -> Self {
        Self(Some(child))
    }

    /// `Child::wait_with_output`, for a serve whose piped output the test reads.
    #[allow(dead_code)]
    pub fn wait_with_output(mut self) -> io::Result<Output> {
        self.0
            .take()
            .expect("serve child present")
            .wait_with_output()
    }
}

impl Deref for ServeGuard {
    type Target = Child;

    fn deref(&self) -> &Child {
        self.0.as_ref().expect("serve child present")
    }
}

impl DerefMut for ServeGuard {
    fn deref_mut(&mut self) -> &mut Child {
        self.0.as_mut().expect("serve child present")
    }
}

impl Drop for ServeGuard {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
