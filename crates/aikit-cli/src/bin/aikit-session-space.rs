//! The standalone `aikit-session-space` binary.
//!
//! A thin wrapper: the whole clap tree, dispatch and output contract live in
//! [`aikit_cli::session_space_cli`], so this binary and the main `aikit`
//! binary's `session-space` subcommand share exactly one implementation (O-I
//! #376). The binary is kept so anything that still invokes
//! `aikit-session-space` directly keeps working, byte-for-byte.

fn main() {
    std::process::exit(aikit_cli::session_space_cli::run_from_args(
        std::env::args_os(),
    ));
}
