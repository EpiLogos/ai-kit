//! Canonical child-message context belongs on Prime's native launcher, after
//! any AIKit model/task re-exec has resolved its admitted body.
use aikit_core::{AikitError, ResourceRef, Result};
use aikit_store::AikitHome;
use std::path::PathBuf;

fn refused(reason: impl ToString) -> AikitError {
    AikitError::new("encounter.prime_child_context", reason.to_string())
}

pub(super) fn child_message_dir(home: &AikitHome, session: &ResourceRef) -> Result<PathBuf> {
    let dir = home.state().join("encounter-child-messages").join(
        blake3::hash(session.as_str().as_bytes())
            .to_hex()
            .to_string(),
    );
    std::fs::create_dir_all(&dir).map_err(refused)?;
    let metadata = std::fs::symlink_metadata(&dir).map_err(refused)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(refused(
            "Prime child-message destination must be an owned directory",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).map_err(refused)?;
    }
    Ok(dir)
}

pub(crate) fn append_context(
    home: &AikitHome,
    session: &ResourceRef,
    argv: &mut Vec<String>,
) -> Result<()> {
    let dir = child_message_dir(home, session)?;
    let mut native = argv.clone();
    for (flag, value) in [
        ("--agent-session", session.to_string()),
        ("--child-message-dir", dir.display().to_string()),
    ] {
        let positions: Vec<_> = native
            .iter()
            .enumerate()
            .filter_map(|(index, arg)| {
                (arg == flag || arg.starts_with(&format!("{flag}="))).then_some(index)
            })
            .collect();
        match positions.as_slice() {
            [] => native.extend([flag.to_owned(), value]),
            [index] if native[*index] == flag && native.get(index + 1) == Some(&value) => {}
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_child_context_is_stable_private_and_distinct_per_canonical_session() {
        let root = tempfile::tempdir().unwrap();
        let home = AikitHome::at(root.path());
        let first = ResourceRef::parse("agent-session/first").unwrap();
        let second = ResourceRef::parse("agent-session/second").unwrap();
        let mut argv = vec![
            "/native/prime-launcher".into(),
            "--prime-bin".into(),
            "/native/prime".into(),
        ];
        append_context(&home, &first, &mut argv).unwrap();
        let once = argv.clone();
        append_context(&home, &first, &mut argv).unwrap();
        assert_eq!(argv, once);
        let dir = child_message_dir(&home, &first).unwrap();
        assert_eq!(argv[4], first.as_str());
        assert_eq!(argv[6], dir.display().to_string());
        std::fs::write(
            dir.join("native-child-message.json"),
            b"retained child words",
        )
        .unwrap();
        assert_eq!(
            std::fs::read(
                child_message_dir(&home, &first)
                    .unwrap()
                    .join("native-child-message.json")
            )
            .unwrap(),
            b"retained child words"
        );
        assert_ne!(dir, child_message_dir(&home, &second).unwrap());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(dir).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
        assert!(append_context(&home, &second, &mut argv).is_err());
        assert_eq!(argv, once);
    }

    #[test]
    fn native_context_refuses_conflicting_duplicate_and_assignment_flags_without_partial_argv() {
        let root = tempfile::tempdir().unwrap();
        let home = AikitHome::at(root.path());
        let session = ResourceRef::parse("agent-session/first").unwrap();
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
            assert!(append_context(&home, &session, &mut argv).is_err());
            assert_eq!(argv, original);
        }
    }
}
