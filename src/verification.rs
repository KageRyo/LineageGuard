use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;

use sha2::{Digest, Sha256};

use crate::error::{AppError, ErrorKind};
use crate::manifest::Project;
use crate::model::{Availability, Finding, VerificationCheck, VerificationStatus};
use crate::paths::inspect_local_path;

struct FileToVerify<'a> {
    id: &'a str,
    kind: &'a str,
    path: &'a str,
    expected_sha256: Option<&'a str>,
    expected_size_bytes: Option<u64>,
    availability: Option<Availability>,
}

pub fn verify_project(
    project: &Project,
) -> Result<(Vec<VerificationCheck>, Vec<Finding>), AppError> {
    let mut checks = Vec::new();
    let mut findings = Vec::new();

    for (id, artifact) in &project.manifest.artifacts.0 {
        let check = if let Some(path) = &artifact.path {
            verify_file(
                &project.root,
                FileToVerify {
                    id,
                    kind: "artifact",
                    path,
                    expected_sha256: artifact.sha256.as_deref(),
                    expected_size_bytes: artifact.size_bytes,
                    availability: None,
                },
                &mut findings,
            )?
        } else {
            findings.push(Finding::new(
                "artifact_not_materialized",
                "error",
                Some(id),
                None,
                "artifact has no local path to verify",
            ));
            VerificationCheck {
                id: id.clone(),
                kind: "artifact".to_owned(),
                status: VerificationStatus::Unverified,
                availability: None,
                path: None,
                expected_sha256: artifact.sha256.clone(),
                actual_sha256: None,
                expected_size_bytes: artifact.size_bytes,
                actual_size_bytes: None,
            }
        };
        checks.push(check);
    }

    for (id, source) in &project.manifest.sources.0 {
        let check = if let Some(snapshot) = &source.snapshot {
            verify_file(
                &project.root,
                FileToVerify {
                    id,
                    kind: "source_snapshot",
                    path: &snapshot.path,
                    expected_sha256: Some(&snapshot.sha256),
                    expected_size_bytes: snapshot.size_bytes,
                    availability: Some(source.status),
                },
                &mut findings,
            )?
        } else {
            let status = match source.status {
                Availability::Available => VerificationStatus::Unverified,
                Availability::Unavailable => VerificationStatus::Unavailable,
                Availability::Unknown => VerificationStatus::Unknown,
                Availability::NotApplicable => VerificationStatus::NotApplicable,
            };
            VerificationCheck {
                id: id.clone(),
                kind: "source".to_owned(),
                status,
                availability: Some(source.status),
                path: None,
                expected_sha256: None,
                actual_sha256: None,
                expected_size_bytes: None,
                actual_size_bytes: None,
            }
        };
        checks.push(check);
    }

    checks.sort_by(|left, right| left.id.cmp(&right.id));
    sort_findings(&mut findings);
    Ok((checks, findings))
}

fn verify_file(
    root: &std::path::Path,
    item: FileToVerify<'_>,
    findings: &mut Vec<Finding>,
) -> Result<VerificationCheck, AppError> {
    let inspection = inspect_local_path(root, item.path)?;
    if !inspection.exists || !inspection.is_file {
        let code = if item.kind == "artifact" {
            "artifact_missing"
        } else {
            "source_snapshot_missing"
        };
        findings.push(Finding::new(
            code,
            "error",
            Some(item.id),
            Some(item.path),
            format!(
                "declared {} file is missing or is not a regular file",
                item.kind
            ),
        ));
        return Ok(VerificationCheck {
            id: item.id.to_owned(),
            kind: item.kind.to_owned(),
            status: VerificationStatus::Missing,
            availability: item.availability,
            path: Some(item.path.to_owned()),
            expected_sha256: item.expected_sha256.map(str::to_owned),
            actual_sha256: None,
            expected_size_bytes: item.expected_size_bytes,
            actual_size_bytes: None,
        });
    }

    let mut file = File::open(&inspection.resolved).map_err(|error| {
        AppError::new(
            ErrorKind::Execution,
            format!(
                "cannot read declared {} file {:?}: {error}",
                item.kind, item.path
            ),
        )
    })?;
    let actual_size_bytes = file
        .metadata()
        .map_err(|error| {
            AppError::new(
                ErrorKind::Execution,
                format!(
                    "cannot inspect declared {} file {:?}: {error}",
                    item.kind, item.path
                ),
            )
        })?
        .len();
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|error| {
            AppError::new(
                ErrorKind::Execution,
                format!(
                    "cannot hash declared {} file {:?}: {error}",
                    item.kind, item.path
                ),
            )
        })?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    let actual_sha256: String = digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let hash_matches = item
        .expected_sha256
        .is_some_and(|expected| expected == actual_sha256);
    let size_matches = item
        .expected_size_bytes
        .is_none_or(|expected| expected == actual_size_bytes);

    if !hash_matches {
        findings.push(Finding::new(
            "hash_mismatch",
            "error",
            Some(item.id),
            Some(item.path),
            format!(
                "SHA-256 does not match the declared value for {}",
                item.kind
            ),
        ));
    }
    if !size_matches {
        findings.push(Finding::new(
            "size_mismatch",
            "error",
            Some(item.id),
            Some(item.path),
            format!(
                "byte size does not match the declared value for {}",
                item.kind
            ),
        ));
    }

    let status = if !hash_matches {
        VerificationStatus::HashMismatch
    } else if !size_matches {
        VerificationStatus::SizeMismatch
    } else {
        VerificationStatus::Verified
    };
    Ok(VerificationCheck {
        id: item.id.to_owned(),
        kind: item.kind.to_owned(),
        status,
        availability: item.availability,
        path: Some(item.path.to_owned()),
        expected_sha256: item.expected_sha256.map(str::to_owned),
        actual_sha256: Some(actual_sha256),
        expected_size_bytes: item.expected_size_bytes,
        actual_size_bytes: Some(actual_size_bytes),
    })
}

pub fn checks_by_id(checks: &[VerificationCheck]) -> BTreeMap<String, VerificationCheck> {
    checks
        .iter()
        .cloned()
        .map(|check| (check.id.clone(), check))
        .collect()
}

pub fn sort_findings(findings: &mut [Finding]) {
    findings.sort_by(|left, right| {
        left.code
            .cmp(&right.code)
            .then_with(|| left.entity_id.cmp(&right.entity_id))
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.message.cmp(&right.message))
    });
}
