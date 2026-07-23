//! AIKit core: a context-scoped capability router for agentic terminal work.
//!
//! The central object is not a global "active set" but an **effective capability
//! view resolved for a context**, where a context is roughly
//!
//! ```text
//! user + host + project scope chain + session space + task + target client
//! ```
//!
//! Everything else in AIKit — the registry, the palette, multiplexer
//! integrations, the hook bank, the capture pipeline — is a view or a consumer of
//! that resolution.
//!
//! This crate is deliberately free of I/O. The resolver takes a catalog, a stack
//! of scope layers and an environment, and returns a resolved graph plus an
//! explanation and a deterministic hash. That is what makes it testable without a
//! filesystem, and what makes generations content-addressable.

#![forbid(unsafe_code)]

pub mod arg;
pub mod capsule;
pub mod catalog;
pub mod context;
pub mod duration;
pub mod effects;
pub mod error;
pub mod guidance;
pub mod hooks;
pub mod id;
pub mod platform;
pub mod policy;
pub mod profile;
pub mod projection;
pub mod resolve;
pub mod scope;
pub mod search;
pub mod session;
pub mod trust;

pub use error::{AikitError, Result};
