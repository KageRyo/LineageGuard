use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{AppError, ErrorKind};

#[derive(Debug, Clone)]
pub struct PathInspection {
    pub resolved: PathBuf,
    pub exists: bool,
    pub is_file: bool,
    pub through_symlink: bool,
    pub mutable_component: bool,
}

pub fn inspect_local_path(root: &Path, declared: &str) -> Result<PathInspection, AppError> {
    validate_relative_path(declared)?;
    let mut current = root.to_path_buf();
    let parts: Vec<&str> = declared.split('/').collect();
    let mut through_symlink = false;
    let mutable_component = parts
        .iter()
        .any(|part| part.eq_ignore_ascii_case("current"));

    for (index, part) in parts.iter().enumerate() {
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                through_symlink = true;
                current = fs::canonicalize(&current).map_err(|error| {
                    AppError::new(
                        ErrorKind::UnsafePath,
                        format!("path {declared:?} contains an unresolved symbolic link: {error}"),
                    )
                })?;
                if !current.starts_with(root) {
                    return Err(AppError::new(
                        ErrorKind::UnsafePath,
                        format!("path {declared:?} resolves outside the project root"),
                    ));
                }
            }
            Ok(metadata) => {
                if index + 1 < parts.len() && !metadata.is_dir() {
                    return Err(AppError::new(
                        ErrorKind::UnsafePath,
                        format!("path {declared:?} has a non-directory parent component"),
                    ));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(AppError::new(
                    ErrorKind::Execution,
                    format!("cannot inspect declared path {declared:?}: {error}"),
                ));
            }
        }
    }

    match fs::metadata(&current) {
        Ok(metadata) => Ok(PathInspection {
            resolved: current,
            exists: true,
            is_file: metadata.is_file(),
            through_symlink,
            mutable_component,
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(PathInspection {
            resolved: current,
            exists: false,
            is_file: false,
            through_symlink,
            mutable_component,
        }),
        Err(error) => Err(AppError::new(
            ErrorKind::Execution,
            format!("cannot inspect declared path {declared:?}: {error}"),
        )),
    }
}

pub fn validate_relative_path(declared: &str) -> Result<(), AppError> {
    if declared.is_empty()
        || declared.starts_with('/')
        || declared.starts_with('\\')
        || declared.contains('\\')
        || Path::new(declared).is_absolute()
        || declared.split('/').any(|part| {
            part.is_empty()
                || part == "."
                || part == ".."
                || part.ends_with(' ')
                || part.ends_with('.')
                || part.chars().any(|character| {
                    character.is_ascii_control()
                        || matches!(character, '<' | '>' | ':' | '"' | '|' | '?' | '*')
                })
                || is_windows_reserved_device_name(part)
        })
    {
        return Err(AppError::new(
            ErrorKind::UnsafePath,
            format!("path must be a safe relative path using '/' separators: {declared:?}"),
        ));
    }
    Ok(())
}

fn is_windows_reserved_device_name(component: &str) -> bool {
    let basename = component
        .split('.')
        .next()
        .unwrap_or(component)
        .trim_end_matches([' ', '.']);
    let basename = basename.to_ascii_uppercase();
    matches!(basename.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|prefix| {
            basename.strip_prefix(prefix).is_some_and(|suffix| {
                matches!(
                    suffix,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
        })
}
