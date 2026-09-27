use std::collections::{BTreeMap, BTreeSet};

use crate::error::{AppError, ErrorKind};
use crate::manifest::Project;
use crate::model::{Finding, LineageNode, VerificationCheck, VerificationStatus};
use crate::validation::validate_project;
use crate::verification::{checks_by_id, sort_findings, verify_project};

pub fn trace_lineage(project: &Project, artifact_id: &str) -> Result<LineageTrace, AppError> {
    let validation_findings = validate_project(project)?;
    if !project.manifest.artifacts.0.contains_key(artifact_id) {
        return Err(AppError::new(
            ErrorKind::UnknownArtifact,
            format!("artifact ID {artifact_id:?} is not declared"),
        ));
    }

    let (checks, verification_findings) = verify_project(project)?;
    let checks = checks_by_id(&checks);
    let mut visited = BTreeSet::from([artifact_id.to_owned()]);
    let mut ancestors = vec![artifact_id.to_owned()];
    let mut upstream = build_upstream(project, artifact_id, &mut ancestors, &checks, &mut visited);
    upstream.sort_by(|left, right| left.id.cmp(&right.id));

    let root_status = checks
        .get(artifact_id)
        .map(|check| check.status)
        .unwrap_or(VerificationStatus::Unverified);
    let mut findings: Vec<Finding> = validation_findings
        .into_iter()
        .filter(|finding| {
            finding.code != "lineage_cycle"
                || finding
                    .entity_id
                    .as_ref()
                    .is_some_and(|id| visited.contains(id))
        })
        .collect();
    findings.extend(verification_findings.into_iter().filter(|finding| {
        finding
            .entity_id
            .as_ref()
            .is_some_and(|id| visited.contains(id))
    }));
    collect_cycle_findings(&upstream, &mut findings);
    sort_findings(&mut findings);

    Ok(LineageTrace {
        artifact: artifact_id.to_owned(),
        status: root_status,
        upstream,
        findings,
    })
}

#[derive(Debug)]
pub struct LineageTrace {
    pub artifact: String,
    pub status: VerificationStatus,
    pub upstream: Vec<LineageNode>,
    pub findings: Vec<Finding>,
}

fn build_upstream(
    project: &Project,
    id: &str,
    ancestors: &mut Vec<String>,
    checks: &BTreeMap<String, VerificationCheck>,
    visited: &mut BTreeSet<String>,
) -> Vec<LineageNode> {
    let mut parent_ids: Vec<&str> = project
        .manifest
        .lineage
        .iter()
        .filter(|edge| edge.to == id)
        .map(|edge| edge.from.as_str())
        .collect();
    parent_ids.sort_unstable();

    parent_ids
        .into_iter()
        .map(|parent_id| {
            if ancestors.iter().any(|ancestor| ancestor == parent_id) {
                visited.insert(parent_id.to_owned());
                return make_node(project, parent_id, checks, true, Vec::new());
            }
            visited.insert(parent_id.to_owned());
            ancestors.push(parent_id.to_owned());
            let children = build_upstream(project, parent_id, ancestors, checks, visited);
            ancestors.pop();
            make_node(project, parent_id, checks, false, children)
        })
        .collect()
}

fn make_node(
    project: &Project,
    id: &str,
    checks: &BTreeMap<String, VerificationCheck>,
    cycle: bool,
    upstream: Vec<LineageNode>,
) -> LineageNode {
    let check = checks.get(id);
    let (path, locator, version, revision, retrieved_at, rights) =
        if let Some(artifact) = project.manifest.artifacts.0.get(id) {
            (
                artifact.path.clone(),
                None,
                artifact.version.clone(),
                artifact.revision.clone(),
                None,
                None,
            )
        } else if let Some(source) = project.manifest.sources.0.get(id) {
            (
                source
                    .snapshot
                    .as_ref()
                    .map(|snapshot| snapshot.path.clone()),
                source.locator.clone(),
                source.version.clone(),
                source.revision.clone(),
                source.retrieved_at.clone(),
                source.rights.clone(),
            )
        } else {
            (None, None, None, None, None, None)
        };
    LineageNode {
        id: id.to_owned(),
        kind: if project.manifest.artifacts.0.contains_key(id) {
            "artifact".to_owned()
        } else {
            "source".to_owned()
        },
        status: check
            .map(|value| value.status)
            .unwrap_or(VerificationStatus::Unverified),
        availability: check.and_then(|value| value.availability),
        path,
        locator,
        version,
        revision,
        retrieved_at,
        rights,
        cycle,
        upstream,
    }
}

fn collect_cycle_findings(nodes: &[LineageNode], findings: &mut Vec<Finding>) {
    for node in nodes {
        if node.cycle {
            findings.push(Finding::new(
                "lineage_cycle",
                "error",
                Some(&node.id),
                None,
                format!("cycle encountered while tracing upstream of {}", node.id),
            ));
        }
        collect_cycle_findings(&node.upstream, findings);
    }
}
