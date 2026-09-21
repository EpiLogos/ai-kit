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

pub mod activity_evidence;
pub mod adopt;
pub mod alias_family;
pub mod app;
pub mod cli;
pub mod client;
pub mod closeout;
mod cmux_config;
pub mod collate;
pub mod config_plane;
pub mod continuity_disclosure;
pub mod control_ground;
pub mod credential;
pub(crate) mod credential_delivery;
pub mod direct_agent_session;
pub mod discover;
pub mod doctor;
pub mod domain_activation;
pub mod env;
pub mod file_context;
pub mod foreign;
pub mod gateway_ops;
pub mod hook;
pub mod json;
pub mod jump;
pub mod model_roster;
pub mod multicall;
pub mod mux_install;
pub mod orientation_packet;
pub mod pressure;
pub mod profile_ops;
pub mod project_binding;
pub mod project_recency;
pub mod projection_drift;
pub mod projects;
pub mod recognised_praxis;
pub mod route_launch;
pub mod run;
pub mod scoped_invocation;
pub mod session_lifecycle_ops;
pub mod session_space_cli;
pub mod session_space_ops;
pub mod session_space_schema;
pub mod session_space_service;
pub mod session_space_working_surface;
pub mod skill_sources;
pub mod star_commands;
pub mod system;
pub mod task;
pub mod temporal;
pub mod tree_build;
pub mod ui;
pub mod wiki;
pub mod wiki_construct;
pub mod wiki_projection;
pub mod wiki_shape;
pub mod working_environment_field;

pub use session_lifecycle_ops::SessionLifecycleServiceOps;
pub use session_space_ops::SessionSpaceCliAdapter;
pub use session_space_service::SessionSpaceServiceOps;

pub mod encounter_mcp;
pub mod encounter_service;

/// Self-invocation shape for session-space verbs. The main `aikit` binary
/// takes them under the `session-space` subcommand; the standalone
/// `aikit-session-space` binary (kept so direct callers keep working) takes
/// them unprefixed. Anything that re-invokes its own executable — resident
/// spawn, model-exec launcher, task launcher — must match its own shape.
pub fn session_space_verb_prefix() -> Option<&'static str> {
    let exe = std::env::current_exe().ok()?;
    let name = exe.file_name()?.to_str()?;
    (name == "aikit").then_some("session-space")
}
