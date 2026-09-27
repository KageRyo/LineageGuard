use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{AppError, ErrorKind};
use crate::model::Manifest;

#[derive(Debug)]
pub struct Project {
    pub root: PathBuf,
    pub manifest: Manifest,
}

pub fn load_project(input: &Path) -> Result<Project, AppError> {
    let metadata = fs::metadata(input).map_err(|error| {
        AppError::new(
            ErrorKind::Execution,
            format!("cannot inspect project path {}: {error}", input.display()),
        )
    })?;

    let (root, manifest_path) = if metadata.is_dir() {
        let root = fs::canonicalize(input).map_err(|error| {
            AppError::new(
                ErrorKind::Execution,
                format!(
                    "cannot resolve project directory {}: {error}",
                    input.display()
                ),
            )
        })?;
        let manifest_path = root.join("lineage.yaml");
        (root, manifest_path)
    } else if metadata.is_file() {
        let manifest_path = fs::canonicalize(input).map_err(|error| {
            AppError::new(
                ErrorKind::Execution,
                format!("cannot resolve manifest path {}: {error}", input.display()),
            )
        })?;
        let root = manifest_path.parent().ok_or_else(|| {
            AppError::new(ErrorKind::Execution, "manifest has no parent directory")
        })?;
        (root.to_path_buf(), manifest_path)
    } else {
        return Err(AppError::new(
            ErrorKind::Execution,
            format!(
                "project path is not a directory or regular file: {}",
                input.display()
            ),
        ));
    };

    let contents = fs::read_to_string(&manifest_path).map_err(|error| {
        AppError::new(
            ErrorKind::Execution,
            format!("cannot read lineage manifest: {error}"),
        )
    })?;
    let manifest = yaml_serde::from_str(&contents).map_err(|error| {
        AppError::new(
            ErrorKind::InvalidManifest,
            format!("invalid lineage manifest: {error}"),
        )
    })?;

    Ok(Project { root, manifest })
}
