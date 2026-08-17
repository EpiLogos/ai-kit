//! The `aikit` binary, unpacked into a library so the CLI contract can be tested
//! without spawning a process for every assertion.
//!
//! The binary itself (`src/main.rs`) is deliberately thin: it decides whether it
//! was invoked as `aikit` or under an exported command name (see [`multicall`]),
//! parses [`cli`], and hands off to the one [`app`] service that the palette also
//! speaks to. Nothing in this crate re-implements a resolver rule, a trust rule
//! or a projection rule — those live in `aikit-core`, `aikit-store` and
//! `aikit-adapters`, and this crate's job is to give them a command line, a
//! stable JSON envelope and a set of exit codes.

#![forbid(unsafe_code)]

pub mod adopt;
pub mod app;
pub mod cli;
pub mod client;
mod cmux_config;
pub mod collate;
pub mod discover;
pub mod doctor;
pub mod env;
pub mod foreign;
pub mod hook;
pub mod json;
pub mod jump;
pub mod model_roster;
pub mod multicall;
pub mod mux_install;
pub mod profile_ops;
pub mod project_binding;
pub mod projects;
pub mod run;
pub mod skill_sources;
pub mod task;
pub mod tree_build;
pub mod ui;
