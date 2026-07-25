use std::fs;
use std::io::Write;
use std::path::Path;

use tempfile::NamedTempFile;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RewriteError {
    #[error("refusing to replace symlink {0}")]
    Symlink(String),
    #[error("failed to read metadata for {path}: {source}")]
    Metadata {
        path: String,
        source: std::io::Error,
    },
    #[error("failed to create temporary file beside {path}: {source}")]
    Create {
        path: String,
        source: std::io::Error,
    },
    #[error("failed to write temporary file for {path}: {source}")]
    Write {
        path: String,
        source: std::io::Error,
    },
    #[error("failed to preserve permissions for {path}: {source}")]
    Permissions {
        path: String,
        source: std::io::Error,
    },
    #[error("failed to atomically replace {path}: {source}")]
    Persist {
        path: String,
        source: std::io::Error,
    },
}

pub fn atomic_replace(path: &Path, contents: &str) -> Result<(), RewriteError> {
    let display = path.display().to_string();
    let metadata = fs::symlink_metadata(path).map_err(|source| RewriteError::Metadata {
        path: display.clone(),
        source,
    })?;
    if metadata.file_type().is_symlink() {
        return Err(RewriteError::Symlink(display));
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut temporary = NamedTempFile::new_in(parent).map_err(|source| RewriteError::Create {
        path: display.clone(),
        source,
    })?;
    temporary
        .write_all(contents.as_bytes())
        .map_err(|source| RewriteError::Write {
            path: display.clone(),
            source,
        })?;
    temporary
        .as_file()
        .set_permissions(metadata.permissions())
        .map_err(|source| RewriteError::Permissions {
            path: display.clone(),
            source,
        })?;
    temporary
        .as_file_mut()
        .sync_all()
        .map_err(|source| RewriteError::Write {
            path: display.clone(),
            source,
        })?;
    temporary
        .persist(path)
        .map_err(|error| RewriteError::Persist {
            path: display,
            source: error.error,
        })?;
    Ok(())
}
