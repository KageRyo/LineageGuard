use std::collections::{BTreeMap, BTreeSet};

use crate::error::{AppError, ErrorKind};
use crate::manifest::Project;
use crate::model::{Availability, Finding, Manifest};
use crate::paths::inspect_local_path;

const SHA256_LENGTH: usize = 64;

#[derive(Debug)]
struct PathBinding {
    sha256: String,
    size_bytes: Option<u64>,
    first_owner: String,
}

pub fn validate_project(project: &Project) -> Result<Vec<Finding>, AppError> {
    validate_manifest(&project.manifest, &project.root)
}

fn validate_manifest(
    manifest: &Manifest,
    root: &std::path::Path,
) -> Result<Vec<Finding>, AppError> {
    if manifest.version != 1 {
        return Err(invalid(format!(
            "manifest version {} is unsupported; expected version 1",
            manifest.version
        )));
    }
    if manifest.artifacts.0.is_empty() {
        return Err(invalid("manifest must declare at least one artifact"));
    }

    let mut all_ids = BTreeSet::new();
    for id in manifest.artifacts.0.keys().chain(manifest.sources.0.keys()) {
        validate_id(id)?;
        if !all_ids.insert(id.as_str()) {
            return Err(invalid(format!(
                "entity ID {id:?} is declared as both an artifact and a source"
            )));
        }
    }

    let mut path_bindings = BTreeMap::new();
    for (id, artifact) in &manifest.artifacts.0 {
        if let Some(path) = &artifact.path {
            inspect_local_path(root, path)?;
            let digest = artifact.sha256.as_deref().ok_or_else(|| {
                invalid(format!(
                    "artifact {id:?} declares a path without a SHA-256 digest"
                ))
            })?;
            register_path_binding(&mut path_bindings, path, digest, artifact.size_bytes, id)?;
        }
        if artifact.size_bytes.is_some() && artifact.path.is_none() {
            return Err(invalid(format!(
                "artifact {id:?} declares size_bytes without a local path"
            )));
        }
        if let Some(digest) = &artifact.sha256 {
            validate_sha256(digest, &format!("artifact {id:?}"))?;
        }
        validate_optional_text("version", artifact.version.as_deref(), id)?;
        validate_optional_text("revision", artifact.revision.as_deref(), id)?;
    }

    for (id, source) in &manifest.sources.0 {
        validate_optional_text("locator", source.locator.as_deref(), id)?;
        validate_optional_text("version", source.version.as_deref(), id)?;
        validate_optional_text("revision", source.revision.as_deref(), id)?;
        validate_optional_text("retrieved_at", source.retrieved_at.as_deref(), id)?;
        validate_optional_text("rights", source.rights.as_deref(), id)?;
        if let Some(snapshot) = &source.snapshot {
            inspect_local_path(root, &snapshot.path)?;
            validate_sha256(&snapshot.sha256, &format!("source {id:?} snapshot"))?;
            register_path_binding(
                &mut path_bindings,
                &snapshot.path,
                &snapshot.sha256,
                snapshot.size_bytes,
                id,
            )?;
        }
    }

    let mut edge_keys = BTreeSet::new();
    for edge in &manifest.lineage {
        if !all_ids.contains(edge.from.as_str()) {
            return Err(invalid(format!(
                "lineage from ID {:?} is not declared",
                edge.from
            )));
        }
        if manifest
            .sources
            .0
            .get(&edge.from)
            .is_some_and(|source| source.status == Availability::NotApplicable)
        {
            return Err(invalid(format!(
                "lineage input {:?} has status not_applicable",
                edge.from
            )));
        }
        if !manifest.artifacts.0.contains_key(&edge.to) {
            if manifest.sources.0.contains_key(&edge.to) {
                return Err(invalid(format!(
                    "lineage target {:?} must be an artifact",
                    edge.to
                )));
            }
            return Err(invalid(format!(
                "lineage target ID {:?} is not a declared artifact",
                edge.to
            )));
        }
        if !edge_keys.insert((edge.from.as_str(), edge.to.as_str(), edge.relation.as_str())) {
            return Err(invalid(format!(
                "duplicate lineage edge {:?} -> {:?}",
                edge.from, edge.to
            )));
        }
    }

    Ok(find_cycles(manifest))
}

fn register_path_binding(
    bindings: &mut BTreeMap<String, PathBinding>,
    path: &str,
    sha256: &str,
    size_bytes: Option<u64>,
    owner: &str,
) -> Result<(), AppError> {
    if let Some(existing) = bindings.get_mut(path) {
        let hash_conflicts = existing.sha256 != sha256;
        let size_conflicts = existing
            .size_bytes
            .zip(size_bytes)
            .is_some_and(|(left, right)| left != right);
        if hash_conflicts || size_conflicts {
            return Err(invalid(format!(
                "path {path:?} has conflicting integrity declarations for entities {:?} and {owner:?}",
                existing.first_owner
            )));
        }
        if existing.size_bytes.is_none() {
            existing.size_bytes = size_bytes;
        }
        return Ok(());
    }
    bindings.insert(
        path.to_owned(),
        PathBinding {
            sha256: sha256.to_owned(),
            size_bytes,
            first_owner: owner.to_owned(),
        },
    );
    Ok(())
}

fn validate_id(id: &str) -> Result<(), AppError> {
    if id.trim() != id || id.is_empty() || id.chars().any(char::is_control) {
        return Err(invalid(format!(
            "entity IDs must be non-empty, trimmed strings without control characters: {id:?}"
        )));
    }
    Ok(())
}

fn validate_optional_text(field: &str, value: Option<&str>, id: &str) -> Result<(), AppError> {
    if value.is_some_and(|text| text.trim().is_empty()) {
        return Err(invalid(format!(
            "{field} for entity {id:?} must not be empty"
        )));
    }
    Ok(())
}

fn validate_sha256(value: &str, owner: &str) -> Result<(), AppError> {
    if value.len() != SHA256_LENGTH
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(invalid(format!(
            "{owner} SHA-256 must be 64 lowercase hexadecimal characters"
        )));
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> AppError {
    AppError::new(ErrorKind::InvalidManifest, message)
}

fn find_cycles(manifest: &Manifest) -> Vec<Finding> {
    let mut adjacency: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for edge in &manifest.lineage {
        if manifest.artifacts.0.contains_key(&edge.from) {
            adjacency.entry(&edge.from).or_default().push(&edge.to);
        }
    }
    for targets in adjacency.values_mut() {
        targets.sort_unstable();
    }

    let mut states: BTreeMap<&str, u8> = manifest
        .artifacts
        .0
        .keys()
        .map(|id| (id.as_str(), 0))
        .collect();
    let mut stack = Vec::new();
    let mut cycles = BTreeSet::new();
    let ids: Vec<&str> = manifest.artifacts.0.keys().map(String::as_str).collect();
    for id in ids {
        if states.get(id) == Some(&0) {
            visit_cycle(id, &adjacency, &mut states, &mut stack, &mut cycles);
        }
    }

    cycles
        .into_iter()
        .map(|cycle| {
            Finding::new(
                "lineage_cycle",
                "error",
                cycle.first().map(String::as_str),
                None,
                format!("lineage dependency cycle: {}", cycle.join(" -> ")),
            )
        })
        .collect()
}

fn visit_cycle<'a>(
    id: &'a str,
    adjacency: &BTreeMap<&'a str, Vec<&'a str>>,
    states: &mut BTreeMap<&'a str, u8>,
    stack: &mut Vec<&'a str>,
    cycles: &mut BTreeSet<Vec<String>>,
) {
    states.insert(id, 1);
    stack.push(id);
    if let Some(targets) = adjacency.get(id) {
        for target in targets {
            match states.get(target).copied().unwrap_or(2) {
                0 => visit_cycle(target, adjacency, states, stack, cycles),
                1 => {
                    if let Some(position) = stack.iter().position(|entry| entry == target) {
                        let nodes = &stack[position..];
                        let minimum = nodes
                            .iter()
                            .enumerate()
                            .min_by_key(|(_, node)| **node)
                            .map(|(index, _)| index)
                            .unwrap_or(0);
                        let mut cycle: Vec<String> = (0..nodes.len())
                            .map(|offset| nodes[(minimum + offset) % nodes.len()].to_string())
                            .collect();
                        if let Some(first) = cycle.first().cloned() {
                            cycle.push(first);
                        }
                        cycles.insert(cycle);
                    }
                }
                _ => {}
            }
        }
    }
    stack.pop();
    states.insert(id, 2);
}
