//! Canonical child-message context belongs on Prime's native launcher, after
//! any AIKit model/task re-exec has resolved its admitted body.
//!
//! A direct launch carries `--agent-session` and the generation's
//! `--child-message-dir` on its own argv. AIKit's selected-model and task
//! wrappers carry their own typed arguments: they are handed the generation
//! directory as their `--child-message-dir` and add Prime's native context at
//! their final exec seam, where the native argv first exists.
use super::EncounterProtocol;
use aikit_core::{AikitError, ResourceRef, Result};
use aikit_store::AikitHome;
use std::path::{Path, PathBuf};

fn refused(reason: impl ToString) -> AikitError {
    AikitError::new("encounter.prime_child_context", reason.to_string())
}

/// One private directory per canonical session; each connection generation
/// writes beneath it.
fn session_root(home: &AikitHome, session: &ResourceRef) -> PathBuf {
    home.state().join("encounter-child-messages").join(
        blake3::hash(session.as_str().as_bytes())
            .to_hex()
            .to_string(),
    )
}

fn owned_private_dir(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir).map_err(refused)?;
    let metadata = std::fs::symlink_metadata(dir).map_err(refused)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(refused(
            "Prime child-message destination must be an owned directory",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).map_err(refused)?;
    }
    Ok(())
}

fn generation_component(generation: &str) -> bool {
    !generation.is_empty()
        && generation.len() <= 64
        && generation.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// The bounded child-to-parent message directory of one connection
/// generation: private, owned and never a symlink.
pub(crate) fn generation_dir(
    home: &AikitHome,
    session: &ResourceRef,
    generation: &str,
) -> Result<PathBuf> {
    if !generation_component(generation) {
        return Err(refused(
            "Prime child-message generation must be one plain path component",
        ));
    }
    let root = session_root(home, session);
    owned_private_dir(&root)?;
    let dir = root.join(generation);
    owned_private_dir(&dir)?;
    Ok(dir)
}

/// Each flag occurs once with exactly this value, or the argv is refused
/// unchanged; a repeated identical pair is already satisfied.
fn set_once(argv: &mut Vec<String>, flags: &[(&str, String)]) -> Result<()> {
    let mut native = argv.clone();
    for (flag, value) in flags {
        let positions: Vec<_> = native
            .iter()
            .enumerate()
            .filter_map(|(index, arg)| {
                (arg == flag || arg.starts_with(&format!("{flag}="))).then_some(index)
            })
            .collect();
        match positions.as_slice() {
            [] => native.extend([(*flag).to_owned(), value.clone()]),
            [index] if native[*index] == *flag && native.get(index + 1) == Some(value) => {}
            _ => {
                return Err(refused(format!(
                    "Native {flag} must occur once with this canonical session's exact value"
                )))
            }
        }
    }
    *argv = native;
    Ok(())
}

/// Prime's native context on its native launcher argv.
pub(crate) fn append_context(
    session: &ResourceRef,
    dir: &Path,
    argv: &mut Vec<String>,
) -> Result<()> {
    set_once(
        argv,
        &[
            ("--agent-session", session.to_string()),
            ("--child-message-dir", dir.display().to_string()),
        ],
    )
}

/// The context this launch's argv carries. A direct launch is Prime's native
/// launcher. AIKit's re-exec wrapper already names the session with its own
/// typed `--agent-session`; it is told only the generation directory, which it
/// hands Prime at its final exec.
pub(crate) fn place(
    argv: &mut Vec<String>,
    session: &ResourceRef,
    dir: &Path,
    through_wrapper: bool,
) -> Result<()> {
    if !through_wrapper {
        return append_context(session, dir, argv);
    }
    let named = argv
        .windows(2)
        .filter(|pair| pair[0] == "--agent-session")
        .collect::<Vec<_>>();
    if named.len() != 1 || named[0][1] != session.as_str() {
        return Err(refused(
            "AIKit's re-exec launcher must name this canonical session exactly once",
        ));
    }
    set_once(argv, &[("--child-message-dir", dir.display().to_string())])
}

/// The directory an exec seam was handed must be this session's own
/// generation directory, as the owner made it.
pub(crate) fn admitted_dir(home: &AikitHome, session: &ResourceRef, dir: &str) -> Result<PathBuf> {
    let path = PathBuf::from(dir);
    let generation = path.file_name().and_then(|name| name.to_str());
    if !path.is_absolute() || !generation.is_some_and(generation_component) {
        return Err(refused(
            "The child-message directory is not a generation directory",
        ));
    }
    let metadata = std::fs::symlink_metadata(&path).map_err(refused)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(refused(
            "Prime child-message destination must be an owned directory",
        ));
    }
    let root = std::fs::canonicalize(session_root(home, session)).map_err(refused)?;
    let parent = path
        .parent()
        .map(std::fs::canonicalize)
        .transpose()
        .map_err(refused)?;
    if parent.as_deref() != Some(root.as_path()) {
        return Err(refused(
            "The child-message directory belongs to another canonical session",
        ));
    }
    Ok(path)
}

/// AIKit's final native exec: a Prime body receives its canonical context;
/// any other body refuses a context it cannot carry.
pub(crate) fn final_exec(
    protocol: EncounterProtocol,
    home: &AikitHome,
    session: &ResourceRef,
    dir: Option<&str>,
    argv: &mut Vec<String>,
) -> Result<()> {
    match (protocol, dir) {
        (EncounterProtocol::PrimeRpc, Some(dir)) => {
            let dir = admitted_dir(home, session, dir)?;
            append_context(session, &dir, argv)
        }
        (EncounterProtocol::PrimeRpc, None) => Err(refused(
            "A Prime body needs its owner's child-message directory at final exec",
        )),
        (_, Some(_)) => Err(refused(
            "Only a Prime body carries a child-message directory",
        )),
        (_, None) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GENERATION: &str = "01JBQ3Z8M6V2Y7K9T4R5W1X0AB";

    #[test]
    fn native_child_context_is_stable_private_and_distinct_per_canonical_session() {
        let root = tempfile::tempdir().unwrap();
        let home = AikitHome::at(root.path());
        let first = ResourceRef::parse("agent-session/first").unwrap();
        let second = ResourceRef::parse("agent-session/second").unwrap();
        let dir = generation_dir(&home, &first, GENERATION).unwrap();
        let mut argv = vec![
            "/native/prime-launcher".into(),
            "--prime-bin".into(),
            "/native/prime".into(),
        ];
        append_context(&first, &dir, &mut argv).unwrap();
        let once = argv.clone();
        append_context(&first, &dir, &mut argv).unwrap();
        assert_eq!(argv, once);
        assert_eq!(argv[4], first.as_str());
        assert_eq!(argv[6], dir.display().to_string());
        std::fs::write(
            dir.join("native-child-message.json"),
            b"retained child words",
        )
        .unwrap();
        assert_eq!(
            std::fs::read(
                generation_dir(&home, &first, GENERATION)
                    .unwrap()
                    .join("native-child-message.json")
            )
            .unwrap(),
            b"retained child words"
        );
        let other = generation_dir(&home, &second, GENERATION).unwrap();
        assert_ne!(dir, other);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for private in [dir.parent().unwrap(), dir.as_path()] {
                assert_eq!(
                    std::fs::metadata(private).unwrap().permissions().mode() & 0o777,
                    0o700
                );
            }
        }
        assert!(append_context(&second, &other, &mut argv).is_err());
        assert_eq!(argv, once);
        assert!(generation_dir(&home, &first, "../escape").is_err());
        assert!(generation_dir(&home, &first, "").is_err());
    }

    #[test]
    fn native_context_refuses_conflicting_duplicate_and_assignment_flags_without_partial_argv() {
        let root = tempfile::tempdir().unwrap();
        let home = AikitHome::at(root.path());
        let session = ResourceRef::parse("agent-session/first").unwrap();
        let dir = generation_dir(&home, &session, GENERATION).unwrap();
        for tail in [
            vec!["--agent-session", "agent-session/other"],
            vec![
                "--agent-session",
                "agent-session/first",
                "--agent-session",
                "agent-session/first",
            ],
            vec!["--child-message-dir", "/unrelated"],
            vec!["--agent-session=agent-session/first"],
            vec!["--agent-session"],
        ] {
            let mut argv = std::iter::once("/native/prime-launcher")
                .chain(tail)
                .map(str::to_owned)
                .collect::<Vec<_>>();
            let original = argv.clone();
            assert!(append_context(&session, &dir, &mut argv).is_err());
            assert_eq!(argv, original);
        }
    }

    #[test]
    fn a_wrapper_launch_is_told_only_the_generation_directory() {
        let root = tempfile::tempdir().unwrap();
        let home = AikitHome::at(root.path());
        let session = ResourceRef::parse("agent-session/first").unwrap();
        let dir = generation_dir(&home, &session, GENERATION).unwrap();
        let wrapper = [
            "/aikit",
            "encounter-model-exec",
            "--agent-session",
            "agent-session/first",
            "--provider",
            "prime",
            "--expected-model-basis",
            "basis",
        ]
        .map(str::to_owned)
        .to_vec();
        let mut argv = wrapper.clone();
        place(&mut argv, &session, &dir, true).unwrap();
        assert_eq!(argv[..wrapper.len()], wrapper[..]);
        assert_eq!(
            argv[wrapper.len()..],
            ["--child-message-dir".to_owned(), dir.display().to_string()]
        );
        // A wrapper that names no session, or another one, is refused whole.
        for mut other in [
            wrapper[..2].to_vec(),
            [&wrapper[..3], &["agent-session/other".to_owned()]].concat(),
        ] {
            let original = other.clone();
            assert!(place(&mut other, &session, &dir, true).is_err());
            assert_eq!(other, original);
        }
        // A direct launch is Prime's own launcher: the whole context.
        let mut direct = vec!["/native/prime-launcher".to_owned()];
        place(&mut direct, &session, &dir, false).unwrap();
        assert_eq!(
            direct[1..],
            [
                "--agent-session".to_owned(),
                session.to_string(),
                "--child-message-dir".to_owned(),
                dir.display().to_string()
            ]
        );
    }

    #[test]
    fn the_final_exec_admits_only_this_sessions_generation_directory_for_prime() {
        let root = tempfile::tempdir().unwrap();
        let home = AikitHome::at(root.path());
        let session = ResourceRef::parse("agent-session/first").unwrap();
        let other = ResourceRef::parse("agent-session/second").unwrap();
        let dir = generation_dir(&home, &session, GENERATION).unwrap();
        let foreign = generation_dir(&home, &other, GENERATION).unwrap();
        let native = || vec!["/native/prime-launcher".to_owned()];

        let mut argv = native();
        final_exec(
            EncounterProtocol::PrimeRpc,
            &home,
            &session,
            Some(&dir.display().to_string()),
            &mut argv,
        )
        .unwrap();
        assert_eq!(
            argv[1..],
            [
                "--agent-session".to_owned(),
                session.to_string(),
                "--child-message-dir".to_owned(),
                dir.display().to_string()
            ]
        );

        let unrelated = root.path().join(GENERATION);
        std::fs::create_dir_all(&unrelated).unwrap();
        for refused_dir in [
            Some(foreign.display().to_string()),
            Some(unrelated.display().to_string()),
            Some(dir.join("..").join("..").display().to_string()),
            Some(GENERATION.to_owned()),
            None,
        ] {
            let mut argv = native();
            assert!(final_exec(
                EncounterProtocol::PrimeRpc,
                &home,
                &session,
                refused_dir.as_deref(),
                &mut argv
            )
            .is_err());
            assert_eq!(argv, native());
        }
        #[cfg(unix)]
        {
            let link = dir.parent().unwrap().join("01JBQ3Z8M6V2Y7K9T4R5W1X0LK");
            std::os::unix::fs::symlink(&dir, &link).unwrap();
            let mut argv = native();
            assert!(final_exec(
                EncounterProtocol::PrimeRpc,
                &home,
                &session,
                Some(&link.display().to_string()),
                &mut argv
            )
            .is_err());
        }

        // Other bodies carry no Prime context and refuse one handed to them.
        let mut argv = native();
        final_exec(EncounterProtocol::PiRpc, &home, &session, None, &mut argv).unwrap();
        assert_eq!(argv, native());
        assert!(final_exec(
            EncounterProtocol::Acp,
            &home,
            &session,
            Some(&dir.display().to_string()),
            &mut argv
        )
        .is_err());
    }
}
