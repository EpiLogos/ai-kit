//! AIKit adapters: multiplexers, agent clients, composable harnesses and shells.

#![forbid(unsafe_code)]

pub mod clients;
pub mod deepseek_harness;
pub mod mux;
pub mod runner;
pub mod shells;

pub use deepseek_harness::{
    deepseek_harness_conformance, DeepSeekHarnessConformance, DeepSeekShellProvider,
    DEEPSEEK_HARNESS_RELEASE, DEEPSEEK_HARNESS_UPSTREAM_REVISION,
};
