pub mod audit;
pub mod diff;
pub mod error;
pub mod lineage;
pub mod manifest;
pub mod model;
pub mod paths;
pub mod validation;
pub mod verification;

use std::path::Path;

use crate::error::{AppError, ErrorKind};
use crate::manifest::{load_project, Project};
use crate::model::{
    AuditReport, ErrorDetail, ErrorReport, LineageReport, ValidationReport, VerificationReport,
};
use crate::verification::{sort_findings, verify_project};

pub fn validate_path(path: &Path) -> Result<ValidationReport, AppError> {
    let project = load_project(path)?;
    let findings = validation::validate_project(&project)?;
    let status = if findings.is_empty() { "pass" } else { "fail" };
    Ok(ValidationReport {
        command: "validate",
        status,
        findings,
    })
}

pub fn verify_path(path: &Path) -> Result<VerificationReport, AppError> {
    let project = load_project(path)?;
    let mut findings = validation::validate_project(&project)?;
    let (checks, integrity_findings) = verify_project(&project)?;
    findings.extend(integrity_findings);
    sort_findings(&mut findings);
    let status = if findings.is_empty() { "pass" } else { "fail" };
    Ok(VerificationReport {
        command: "verify",
        status,
        checks,
        findings,
    })
}

pub fn lineage_path(path: &Path, artifact_id: &str) -> Result<LineageReport, AppError> {
    let project = load_project(path)?;
    let trace = lineage::trace_lineage(&project, artifact_id)?;
    let artifact = project
        .manifest
        .artifacts
        .0
        .get(artifact_id)
        .expect("lineage trace validates its artifact ID");
    let status = if trace.findings.is_empty() {
        "pass"
    } else {
        "fail"
    };
    let findings = trace.findings;
    Ok(LineageReport {
        command: "lineage",
        artifact: trace.artifact,
        status,
        artifact_status: trace.status,
        artifact_path: artifact.path.clone(),
        artifact_sha256: artifact.sha256.clone(),
        artifact_version: artifact.version.clone(),
        artifact_revision: artifact.revision.clone(),
        upstream: trace.upstream,
        findings,
    })
}

pub fn audit_path(path: &Path) -> Result<AuditReport, AppError> {
    let project: Project = load_project(path)?;
    audit::audit_project(&project)
}

pub fn error_report<'a>(command: &'a str, error: &'a AppError) -> ErrorReport<'a> {
    ErrorReport {
        command,
        status: "error",
        error: ErrorDetail {
            code: error.code(),
            message: &error.message,
        },
    }
}

pub fn error_exit_code(error: &AppError) -> i32 {
    match error.kind {
        ErrorKind::UnknownArtifact
        | ErrorKind::InvalidManifest
        | ErrorKind::UnsafePath
        | ErrorKind::Execution => 2,
    }
}
