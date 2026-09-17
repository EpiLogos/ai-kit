//! First-party local observation transport for AIKit SessionSpace read models.
//!
//! `aikit-core` remains I/O-free and owns SessionSpace semantics. This adapter
//! only lets an existing [`SessionSpaceRuntime`] publish its current UI-neutral
//! read model for another first-party process to observe. It cannot restore or
//! mutate SessionSpace.

use aikit_core::session_space::{
    SessionSpaceReadModel, SessionSpaceRuntime, SESSION_SPACE_VERSION,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

pub const SESSION_SPACE_OBSERVATION_FILE_VERSION: &str = "aikit.session-space-observation-file/v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredSessionSpaceObservation {
    schema: String,
    read_model: SessionSpaceReadModel,
}

#[derive(Debug, Clone)]
pub struct SessionSpaceFileObservationProvider {
    path: PathBuf,
}

impl SessionSpaceFileObservationProvider {
    pub fn publish(
        path: impl Into<PathBuf>,
        runtime: &SessionSpaceRuntime,
    ) -> Result<Self, SessionSpaceObservationError> {
        let provider = Self { path: path.into() };
        provider.write_read_model(&runtime.read_model())?;
        Ok(provider)
    }

    pub fn open(path: impl Into<PathBuf>) -> Result<Self, SessionSpaceObservationError> {
        let provider = Self { path: path.into() };
        provider.read()?;
        Ok(provider)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn read(&self) -> Result<SessionSpaceReadModel, SessionSpaceObservationError> {
        let bytes = fs::read(&self.path)?;
        let stored: StoredSessionSpaceObservation = serde_json::from_slice(&bytes)?;
        if stored.schema != SESSION_SPACE_OBSERVATION_FILE_VERSION {
            return Err(SessionSpaceObservationError::UnsupportedSchema(
                stored.schema,
            ));
        }
        if stored.read_model.version != SESSION_SPACE_VERSION {
            return Err(SessionSpaceObservationError::UnsupportedReadModel(
                stored.read_model.version,
            ));
        }
        Ok(stored.read_model)
    }

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
