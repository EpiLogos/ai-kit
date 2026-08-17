//! Target-owned local observation bridge for [`SessionSpaceReadModel`].
//!
//! This is deliberately **not** SessionSpace persistence/restore and not a second
//! runtime. AIKit remains the semantic owner: an existing [`SessionSpaceRuntime`]
//! publishes its current UI-neutral read model, while another first-party process
//! may re-read that observation without reconstructing or mutating SessionSpace.

use crate::session_space::{SessionSpaceReadModel, SessionSpaceRuntime, SESSION_SPACE_VERSION};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

pub const SESSION_SPACE_OBSERVATION_FILE_VERSION: &str =
    "aikit.session-space-observation-file/v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredSessionSpaceObservation {
    schema: String,
    read_model: SessionSpaceReadModel,
}

/// A small first-party file binding for cross-process observation of the
/// AIKit-owned read model. The file is a replaceable transport artifact; the
/// canonical SessionSpace remains the owning runtime and its existing state.
#[derive(Debug, Clone)]
pub struct SessionSpaceFileObservationProvider {
    path: PathBuf,
}

impl SessionSpaceFileObservationProvider {
    /// Publish the current observation from an actual AIKit SessionSpace runtime.
    pub fn publish(
        path: impl Into<PathBuf>,
        runtime: &SessionSpaceRuntime,
    ) -> Result<Self, SessionSpaceObservationError> {
        let provider = Self { path: path.into() };
        provider.write_read_model(&runtime.read_model())?;
        Ok(provider)
    }

    /// Open an existing observation file and verify its AIKit-owned schema/read
    /// model before returning a handle.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, SessionSpaceObservationError> {
        let provider = Self { path: path.into() };
        provider.read()?;
        Ok(provider)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Re-read the latest owner-published observation. No runtime state is
    /// inferred from filesystem timestamps or from an external product.
    pub fn read(&self) -> Result<SessionSpaceReadModel, SessionSpaceObservationError> {
        let bytes = fs::read(&self.path)?;
        let stored: StoredSessionSpaceObservation = serde_json::from_slice(&bytes)?;
        if stored.schema != SESSION_SPACE_OBSERVATION_FILE_VERSION {
            return Err(SessionSpaceObservationError::UnsupportedSchema(stored.schema));
        }
        if stored.read_model.version != SESSION_SPACE_VERSION {
            return Err(SessionSpaceObservationError::UnsupportedReadModel(
                stored.read_model.version,
            ));
        }
        Ok(stored.read_model)
    }

    /// Republish after the owning runtime changes. This remains a one-way
    /// observation operation; the provider never applies the file back to runtime.
    pub fn republish(
        &self,
        runtime: &SessionSpaceRuntime,
    ) -> Result<SessionSpaceReadModel, SessionSpaceObservationError> {
        let read_model = runtime.read_model();
        self.write_read_model(&read_model)?;
        Ok(read_model)
    }

    fn write_read_model(
        &self,
        read_model: &SessionSpaceReadModel,
    ) -> Result<(), SessionSpaceObservationError> {
        if read_model.version != SESSION_SPACE_VERSION {
            return Err(SessionSpaceObservationError::UnsupportedReadModel(
                read_model.version.clone(),
            ));
        }
        if let Some(parent) = self
            .path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
        let stored = StoredSessionSpaceObservation {
            schema: SESSION_SPACE_OBSERVATION_FILE_VERSION.into(),
            read_model: read_model.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&stored)?;
        let temporary = temporary_path(&self.path);
        fs::write(&temporary, bytes)?;
        fs::rename(&temporary, &self.path).inspect_err(|_| {
            let _ = fs::remove_file(&temporary);
        })?;
        Ok(())
    }
}

fn temporary_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("session-space-observation.json");
    path.with_file_name(format!(".{file_name}.tmp-{}", std::process::id()))
}

#[derive(Debug, Error)]
pub enum SessionSpaceObservationError {
    #[error("AIKit SessionSpace observation I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("AIKit SessionSpace observation JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsupported AIKit SessionSpace observation schema `{0}`")]
    UnsupportedSchema(String),
    #[error("unsupported AIKit SessionSpace read-model version `{0}`")]
    UnsupportedReadModel(String),
}
