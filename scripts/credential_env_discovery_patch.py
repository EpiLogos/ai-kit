#!/usr/bin/env python3
"""Temporary cloud codemod: declared env routes are discoverable without reading values."""
from pathlib import Path

path = Path("crates/aikit-adapters/src/credential_provider.rs")
text = path.read_text()
old = '''    /// Discover a named environment source without importing its secret value.
    pub fn discover(
        credential_ref: CredentialRef,
        env_var: impl Into<String>,
        project_env: Option<&Path>,
    ) -> Result<Self> {
        let env_var = env_var.into();
        if env_var.trim().is_empty() {
            return Err(provider_error(
                "credential.env_var_invalid",
                "environment variable name must not be empty",
            ));
        }
        let shell_available = std::env::var_os(&env_var).is_some();
        let project_available = project_env
            .map(|path| dotenv_contains_key(path, &env_var))
            .transpose()?
            .unwrap_or(false);
        let provenance = if shell_available {
            format!("shell-env:{env_var}")
        } else if project_available {
            format!(
                "project-env:{}#{env_var}",
                project_env.expect("project path is present").display()
            )
        } else {
            format!("shell-env:{env_var}")
        };
        Ok(Self {
            credential_ref,
            env_var,
            source_available: shell_available || project_available,
            value: None,
            provenance,
        })
    }
'''
new = '''    /// Discover a declared environment route without reading its secret value.
    /// Existence is intentionally not probed here: the route is discoverable,
    /// while actual source presence is tested only after explicit `--from-env`.
    pub fn discover(
        credential_ref: CredentialRef,
        env_var: impl Into<String>,
        project_env: Option<&Path>,
    ) -> Result<Self> {
        let env_var = env_var.into();
        if env_var.trim().is_empty() {
            return Err(provider_error(
                "credential.env_var_invalid",
                "environment variable name must not be empty",
            ));
        }
        let provenance = project_env
            .map(|path| format!("project-env:{}#{env_var}", path.display()))
            .unwrap_or_else(|| format!("shell-env:{env_var}"));
        Ok(Self {
            credential_ref,
            env_var,
            source_available: true,
            value: None,
            provenance,
        })
    }
'''
if new not in text:
    if old not in text:
        raise SystemExit("generated env discovery anchor not found")
    text = text.replace(old, new, 1)

helper_start = '''fn dotenv_contains_key(path: &Path, name: &str) -> Result<bool> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(provider_error(
                "credential.project_env_unreadable",
                format!("could not read {}: {error}", path.display()),
            ))
        }
    };
    Ok(text.lines().any(|raw| {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            return false;
        }
        let line = line.strip_prefix("export ").unwrap_or(line).trim();
        line.split_once('=')
            .map(|(key, _)| key.trim() == name)
            .unwrap_or(false)
    }))
}

'''
text = text.replace(helper_start, "", 1)
path.write_text(text)
