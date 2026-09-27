use std::collections::BTreeMap;

use crate::error::AppError;
use crate::manifest::Project;
use crate::model::{AuditReport, Availability, Finding};
use crate::paths::inspect_local_path;
use crate::validation::validate_project;
use crate::verification::{sort_findings, verify_project};

pub fn audit_project(project: &Project) -> Result<AuditReport, AppError> {
    let mut findings = validate_project(project)?;
    let (checks, verification_findings) = verify_project(project)?;
    findings.extend(verification_findings);

    let mut upstream_by_artifact: BTreeMap<&str, usize> = BTreeMap::new();
    for edge in &project.manifest.lineage {
        *upstream_by_artifact.entry(&edge.to).or_default() += 1;
    }

    for (id, artifact) in &project.manifest.artifacts.0 {
        if upstream_by_artifact.get(id.as_str()).copied().unwrap_or(0) == 0 {
            findings.push(Finding::new(
                "artifact_without_provenance",
                "gap",
                Some(id),
                artifact.path.as_deref(),
                "artifact has no declared upstream lineage",
            ));
        }
        if let Some(path) = &artifact.path {
            add_mutable_path_finding(project, id, path, &mut findings)?;
        }
    }

    for (id, source) in &project.manifest.sources.0 {
        match source.status {
            Availability::Unavailable => findings.push(Finding::new(
                "source_unavailable",
                "gap",
                Some(id),
                None,
                "source is explicitly declared unavailable",
            )),
            Availability::Unknown => findings.push(Finding::new(
                "source_unknown",
                "gap",
                Some(id),
                None,
                "source availability is explicitly unknown",
            )),
            Availability::Available | Availability::NotApplicable => {}
        }
        if source.status == Availability::Available && source.snapshot.is_none() {
            findings.push(Finding::new(
                "source_not_pinned",
                "gap",
                Some(id),
                None,
                "available source has no local snapshot bound to a SHA-256 digest",
            ));
        }
        if source.status == Availability::Available && source.locator.is_none() {
            findings.push(Finding::new(
                "source_locator_missing",
                "gap",
                Some(id),
                None,
                "available source has no locator",
            ));
        }
        if let Some(snapshot) = &source.snapshot {
            add_mutable_path_finding(project, id, &snapshot.path, &mut findings)?;
        }
    }

    sort_findings(&mut findings);
    let status = if findings.is_empty() { "pass" } else { "fail" };
    Ok(AuditReport {
        command: "audit",
        status,
        checks,
        findings,
    })
}

fn add_mutable_path_finding(
    project: &Project,
    id: &str,
    path: &str,
    findings: &mut Vec<Finding>,
) -> Result<(), AppError> {
    let inspection = inspect_local_path(&project.root, path)?;
    if inspection.through_symlink || inspection.mutable_component {
        let mut reasons = Vec::new();
        if inspection.through_symlink {
            reasons.push("uses a symbolic link");
        }
        if inspection.mutable_component {
            reasons.push("contains a 'current' path component");
        }
        findings.push(Finding::new(
            "mutable_path",
            "gap",
            Some(id),
            Some(path),
            format!("declared path {}", reasons.join(" and ")),
        ));
    }
    Ok(())
}
