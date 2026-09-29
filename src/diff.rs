use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::error::{AppError, ErrorKind};
use crate::manifest::load_project;

#[derive(Debug, Default, Serialize)]
pub struct EntityChangeCounts {
    pub added: usize,
    pub removed: usize,
    pub changed: usize,
}

#[derive(Debug, Default, Serialize)]
pub struct EdgeChangeCounts {
    pub added: usize,
    pub removed: usize,
}

#[derive(Debug, Serialize)]
pub struct DiffSummary {
    pub has_changes: bool,
    pub manifest_version_changed: bool,
    pub sources: EntityChangeCounts,
    pub artifacts: EntityChangeCounts,
    pub edges: EdgeChangeCounts,
}

#[derive(Debug, Serialize)]
pub struct FieldChange {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct ManifestChange {
    pub kind: &'static str,
    pub change: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relationship: Option<String>,
    pub fields: Vec<FieldChange>,
}

#[derive(Debug, Serialize)]
pub struct DiffReport {
    pub command: &'static str,
    pub status: &'static str,
    pub summary: DiffSummary,
    pub changes: Vec<ManifestChange>,
}

pub fn diff_paths(old_path: &Path, new_path: &Path) -> Result<DiffReport, AppError> {
    let old = load_project(old_path)?;
    let new = load_project(new_path)?;
    let mut summary = DiffSummary {
        has_changes: false,
        manifest_version_changed: old.manifest.version != new.manifest.version,
        sources: EntityChangeCounts::default(),
        artifacts: EntityChangeCounts::default(),
        edges: EdgeChangeCounts::default(),
    };
    let mut changes = Vec::new();

    if summary.manifest_version_changed {
        changes.push(ManifestChange {
            kind: "manifest",
            change: "changed",
            id: None,
            from: None,
            to: None,
            relationship: None,
            fields: vec![FieldChange {
                path: "version".to_owned(),
                old: Some(Value::from(old.manifest.version)),
                new: Some(Value::from(new.manifest.version)),
            }],
        });
    }

    compare_entities(
        "source",
        &old.manifest.sources.0,
        &new.manifest.sources.0,
        &mut summary.sources,
        &mut changes,
    )?;
    compare_entities(
        "artifact",
        &old.manifest.artifacts.0,
        &new.manifest.artifacts.0,
        &mut summary.artifacts,
        &mut changes,
    )?;
    compare_edges(
        &old.manifest.lineage,
        &new.manifest.lineage,
        &mut summary.edges,
        &mut changes,
    );

    summary.has_changes = summary.manifest_version_changed
        || summary.sources.added > 0
        || summary.sources.removed > 0
        || summary.sources.changed > 0
        || summary.artifacts.added > 0
        || summary.artifacts.removed > 0
        || summary.artifacts.changed > 0
        || summary.edges.added > 0
        || summary.edges.removed > 0;

    Ok(DiffReport {
        command: "diff",
        status: if summary.has_changes {
            "changed"
        } else {
            "unchanged"
        },
        summary,
        changes,
    })
}

fn compare_entities<T: Serialize>(
    kind: &'static str,
    old: &BTreeMap<String, T>,
    new: &BTreeMap<String, T>,
    counts: &mut EntityChangeCounts,
    changes: &mut Vec<ManifestChange>,
) -> Result<(), AppError> {
    for (id, old_entity) in old {
        match new.get(id) {
            None => {
                let old_value = serde_json::to_value(old_entity)
                    .map_err(|error| serialization_error(kind, id, error))?;
                let mut fields = Vec::new();
                collect_present_fields("", &old_value, false, &mut fields);
                counts.removed += 1;
                changes.push(ManifestChange {
                    kind,
                    change: "removed",
                    id: Some(id.clone()),
                    from: None,
                    to: None,
                    relationship: None,
                    fields,
                });
            }
            Some(new_entity) => {
                let old_value = serde_json::to_value(old_entity)
                    .map_err(|error| serialization_error(kind, id, error))?;
                let new_value = serde_json::to_value(new_entity)
                    .map_err(|error| serialization_error(kind, id, error))?;
                if old_value != new_value {
                    let mut fields = Vec::new();
                    collect_field_changes("", &old_value, &new_value, &mut fields);
                    counts.changed += 1;
                    changes.push(ManifestChange {
                        kind,
                        change: "changed",
                        id: Some(id.clone()),
                        from: None,
                        to: None,
                        relationship: None,
                        fields,
                    });
                }
            }
        }
    }

    for id in new.keys() {
        if let Some(new_entity) = new.get(id).filter(|_| !old.contains_key(id)) {
            let new_value = serde_json::to_value(new_entity)
                .map_err(|error| serialization_error(kind, id, error))?;
            let mut fields = Vec::new();
            collect_present_fields("", &new_value, true, &mut fields);
            counts.added += 1;
            changes.push(ManifestChange {
                kind,
                change: "added",
                id: Some(id.clone()),
                from: None,
                to: None,
                relationship: None,
                fields,
            });
        }
    }

    Ok(())
}

fn collect_present_fields(path: &str, value: &Value, is_new: bool, changes: &mut Vec<FieldChange>) {
    if let Value::Object(fields) = value {
        for (key, child) in fields {
            let field_path = if path.is_empty() {
                key.to_owned()
            } else {
                format!("{path}.{key}")
            };
            collect_present_fields(&field_path, child, is_new, changes);
        }
        return;
    }

    changes.push(FieldChange {
        path: path.to_owned(),
        old: (!is_new).then(|| value.clone()),
        new: is_new.then(|| value.clone()),
    });
}

fn collect_field_changes(path: &str, old: &Value, new: &Value, changes: &mut Vec<FieldChange>) {
    if old == new {
        return;
    }

    if let (Value::Object(old_fields), Value::Object(new_fields)) = (old, new) {
        let keys: BTreeSet<&str> = old_fields
            .keys()
            .chain(new_fields.keys())
            .map(String::as_str)
            .collect();
        for key in keys {
            let old_value = old_fields.get(key);
            let new_value = new_fields.get(key);
            let field_path = if path.is_empty() {
                key.to_owned()
            } else {
                format!("{path}.{key}")
            };
            match (old_value, new_value) {
                (Some(old_value), Some(new_value)) => {
                    collect_field_changes(&field_path, old_value, new_value, changes);
                }
                (old_value, new_value) => changes.push(FieldChange {
                    path: field_path,
                    old: old_value.cloned(),
                    new: new_value.cloned(),
                }),
            }
        }
        return;
    }

    changes.push(FieldChange {
        path: path.to_owned(),
        old: Some(old.clone()),
        new: Some(new.clone()),
    });
}

#[derive(Debug, Clone, Ord, PartialOrd, Eq, PartialEq)]
struct EdgeKey {
    from: String,
    to: String,
    relationship: String,
}

fn edge_counts(edges: &[crate::model::LineageEdge]) -> BTreeMap<EdgeKey, usize> {
    let mut counts = BTreeMap::new();
    for edge in edges {
        let key = EdgeKey {
            from: edge.from.clone(),
            to: edge.to.clone(),
            relationship: edge.relation.as_str().to_owned(),
        };
        *counts.entry(key).or_insert(0) += 1;
    }
    counts
}

fn compare_edges(
    old: &[crate::model::LineageEdge],
    new: &[crate::model::LineageEdge],
    counts: &mut EdgeChangeCounts,
    changes: &mut Vec<ManifestChange>,
) {
    let old_edges = edge_counts(old);
    let new_edges = edge_counts(new);

    for (edge, old_count) in &old_edges {
        let new_count = new_edges.get(edge).copied().unwrap_or_default();
        for _ in 0..old_count.saturating_sub(new_count) {
            counts.removed += 1;
            changes.push(edge_change("removed", edge));
        }
    }
    for (edge, new_count) in &new_edges {
        let old_count = old_edges.get(edge).copied().unwrap_or_default();
        for _ in 0..new_count.saturating_sub(old_count) {
            counts.added += 1;
            changes.push(edge_change("added", edge));
        }
    }
}

fn edge_change(change: &'static str, edge: &EdgeKey) -> ManifestChange {
    ManifestChange {
        kind: "edge",
        change,
        id: None,
        from: Some(edge.from.clone()),
        to: Some(edge.to.clone()),
        relationship: Some(edge.relationship.clone()),
        fields: Vec::new(),
    }
}

fn serialization_error(kind: &str, id: &str, error: serde_json::Error) -> AppError {
    AppError::new(
        ErrorKind::Execution,
        format!("cannot compare {kind} {id:?}: {error}"),
    )
}

pub fn render_text(report: &DiffReport) -> String {
    let mut lines = vec![if report.summary.has_changes {
        "DIFF".to_owned()
    } else {
        "NO CHANGES".to_owned()
    }];
    if report.summary.manifest_version_changed {
        if let Some(version) = report
            .changes
            .iter()
            .find(|change| change.kind == "manifest")
            .and_then(|change| change.fields.first())
        {
            lines.push(format!(
                "Manifest version: {} -> {}",
                version.old.as_ref().map(render_value).unwrap_or_default(),
                version.new.as_ref().map(render_value).unwrap_or_default()
            ));
        }
    }
    lines.push(format!(
        "Sources: +{} added, -{} removed, ~{} changed",
        report.summary.sources.added,
        report.summary.sources.removed,
        report.summary.sources.changed
    ));
    lines.push(format!(
        "Artifacts: +{} added, -{} removed, ~{} changed",
        report.summary.artifacts.added,
        report.summary.artifacts.removed,
        report.summary.artifacts.changed
    ));
    lines.push(format!(
        "Edges: +{} added, -{} removed",
        report.summary.edges.added, report.summary.edges.removed
    ));

    for change in &report.changes {
        let symbol = match change.change {
            "added" => "+",
            "removed" => "-",
            "changed" => "~",
            _ => "?",
        };
        let subject = match change.kind {
            "manifest" => "manifest".to_owned(),
            "edge" => format!(
                "edge {} -> {} [{}]",
                render_identifier(change.from.as_deref().unwrap_or("?")),
                render_identifier(change.to.as_deref().unwrap_or("?")),
                change.relationship.as_deref().unwrap_or("?")
            ),
            kind => format!(
                "{kind} {}",
                render_identifier(change.id.as_deref().unwrap_or("?"))
            ),
        };
        lines.push(format!("  {symbol} {subject}"));
        for field in &change.fields {
            let old = field
                .old
                .as_ref()
                .map(render_value)
                .unwrap_or_else(|| "<absent>".to_owned());
            let new = field
                .new
                .as_ref()
                .map(render_value)
                .unwrap_or_else(|| "<absent>".to_owned());
            lines.push(format!("      {}: {old} -> {new}", field.path));
        }
    }

    format!("{}\n", lines.join("\n"))
}

fn render_value(value: &Value) -> String {
    serde_json::to_string(value).expect("JSON values are serializable")
}

fn render_identifier(identifier: &str) -> String {
    serde_json::to_string(identifier).expect("string values are serializable")
}
