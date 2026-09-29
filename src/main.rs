use std::collections::BTreeMap;
use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use lineageguard::diff::{diff_paths, render_text as render_diff};
use lineageguard::manifest::load_project;
use lineageguard::model::{
    AuditReport, Finding, LineageNode, LineageReport, Manifest, ValidationReport,
    VerificationReport,
};
use lineageguard::validation::validate_project;
use lineageguard::{
    audit_path, error_exit_code, error_report, lineage_path, validate_path, verify_path,
};
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(
    name = "lineageguard",
    version,
    about = "Validate local artifact provenance, identity, and lineage"
)]
struct Cli {
    #[arg(long, value_enum, global = true)]
    format: Option<OutputFormat>,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
    Mermaid,
    Dot,
}

#[derive(Debug, Clone, Copy)]
enum GraphFormat {
    Mermaid,
    Dot,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Validate the manifest structure and lineage graph.
    Validate {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Verify declared local artifacts and source snapshots.
    Verify {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Show the upstream lineage of one artifact.
    Lineage {
        artifact_id: String,
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Audit project provenance gaps and local integrity.
    Audit {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Compare the sources, artifacts, and edges in two manifests.
    Diff { old: PathBuf, new: PathBuf },
    /// Render all declared source, artifact, and lineage nodes as a graph.
    Graph {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
}

fn main() {
    let cli = Cli::parse();
    if let Commands::Graph { path } = &cli.command {
        let format = match cli.format {
            None | Some(OutputFormat::Mermaid) => GraphFormat::Mermaid,
            Some(OutputFormat::Dot) => GraphFormat::Dot,
            Some(OutputFormat::Text | OutputFormat::Json) => {
                eprintln!("error [invalid_format]: graph accepts --format mermaid or dot");
                std::process::exit(2);
            }
        };
        std::process::exit(run_graph(path, format));
    }

    let format = match cli.format {
        None | Some(OutputFormat::Text) => OutputFormat::Text,
        Some(OutputFormat::Json) => OutputFormat::Json,
        Some(OutputFormat::Mermaid | OutputFormat::Dot) => {
            eprintln!("error [invalid_format]: report commands accept --format text or json");
            std::process::exit(2);
        }
    };
    let (command_name, result) = match cli.command {
        Commands::Validate { path } => (
            "validate",
            validate_path(&path).map(|report| {
                let status = report.status;
                emit(&report, format, || render_validation(&report));
                if status == "pass" {
                    0
                } else {
                    1
                }
            }),
        ),
        Commands::Verify { path } => (
            "verify",
            verify_path(&path).map(|report| {
                let status = report.status;
                emit(&report, format, || render_verification(&report));
                if status == "pass" {
                    0
                } else {
                    1
                }
            }),
        ),
        Commands::Lineage { artifact_id, path } => (
            "lineage",
            lineage_path(&path, &artifact_id).map(|report| {
                let success = report.findings.is_empty();
                emit(&report, format, || render_lineage(&report));
                if success {
                    0
                } else {
                    1
                }
            }),
        ),
        Commands::Audit { path } => (
            "audit",
            audit_path(&path).map(|report| {
                let status = report.status;
                emit(&report, format, || render_audit(&report));
                if status == "pass" {
                    0
                } else {
                    1
                }
            }),
        ),
        Commands::Diff { old, new } => (
            "diff",
            diff_paths(&old, &new).map(|report| {
                emit(&report, format, || render_diff(&report));
                0
            }),
        ),
        Commands::Graph { .. } => unreachable!("graph commands are handled above"),
    };

    match result {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            if matches!(format, OutputFormat::Json) {
                emit(&error_report(command_name, &error), format, || {
                    String::new()
                });
            } else {
                eprintln!("error [{}]: {error}", error.code());
            }
            std::process::exit(error_exit_code(&error));
        }
    }
}

fn emit<T, F>(value: &T, format: OutputFormat, render_text: F)
where
    T: Serialize,
    F: FnOnce() -> String,
{
    match format {
        OutputFormat::Json => match serde_json::to_string_pretty(value) {
            Ok(json) => println!("{json}"),
            Err(error) => {
                eprintln!("error [execution_error]: cannot serialize report: {error}");
            }
        },
        OutputFormat::Text => println!("{}", render_text()),
        OutputFormat::Mermaid | OutputFormat::Dot => {
            unreachable!("graph formats are handled separately")
        }
    }
}

fn run_graph(path: &std::path::Path, format: GraphFormat) -> i32 {
    let project = match load_project(path) {
        Ok(project) => project,
        Err(error) => {
            eprintln!("error [{}]: {error}", error.code());
            return error_exit_code(&error);
        }
    };
    let findings = match validate_project(&project) {
        Ok(findings) => findings,
        Err(error) => {
            eprintln!("error [{}]: {error}", error.code());
            return error_exit_code(&error);
        }
    };
    if !findings.is_empty() {
        for finding in &findings {
            eprintln!("{}", render_finding(finding));
        }
        return 1;
    }
    print!("{}", render_graph(&project.manifest, format));
    0
}

fn render_graph(manifest: &Manifest, format: GraphFormat) -> String {
    let mut node_ids = BTreeMap::new();
    for (index, id) in manifest.sources.0.keys().enumerate() {
        node_ids.insert(id.as_str(), format!("source_{index:04}"));
    }
    for (index, id) in manifest.artifacts.0.keys().enumerate() {
        node_ids.insert(id.as_str(), format!("artifact_{index:04}"));
    }

    let mut lines = match format {
        GraphFormat::Mermaid => vec!["flowchart LR".to_owned()],
        GraphFormat::Dot => vec!["digraph lineage {".to_owned(), "  rankdir=LR;".to_owned()],
    };

    for (id, source) in &manifest.sources.0 {
        let node_id = &node_ids[id.as_str()];
        let label = format!("source: {id} (availability={})", source.status.as_str());
        match format {
            GraphFormat::Mermaid => lines.push(format!(
                "  {node_id}([\"{}\"])",
                escape_mermaid_label(&label)
            )),
            GraphFormat::Dot => lines.push(format!(
                "  {node_id} [label=\"{}\", shape=ellipse];",
                escape_dot_label(&label)
            )),
        }
    }
    for id in manifest.artifacts.0.keys() {
        let node_id = &node_ids[id.as_str()];
        let label = format!("artifact: {id}");
        match format {
            GraphFormat::Mermaid => {
                lines.push(format!("  {node_id}[\"{}\"]", escape_mermaid_label(&label)))
            }
            GraphFormat::Dot => lines.push(format!(
                "  {node_id} [label=\"{}\", shape=box];",
                escape_dot_label(&label)
            )),
        }
    }

    let mut edges: Vec<_> = manifest.lineage.iter().collect();
    edges.sort_by(|left, right| {
        (&left.from, &left.to, left.relation.as_str()).cmp(&(
            &right.from,
            &right.to,
            right.relation.as_str(),
        ))
    });
    for edge in edges {
        let from = &node_ids[edge.from.as_str()];
        let to = &node_ids[edge.to.as_str()];
        match format {
            GraphFormat::Mermaid => lines.push(format!(
                "  {from} -->|{}| {to}",
                escape_mermaid_label(edge.relation.as_str())
            )),
            GraphFormat::Dot => lines.push(format!(
                "  {from} -> {to} [label=\"{}\"];",
                escape_dot_label(edge.relation.as_str())
            )),
        }
    }

    if matches!(format, GraphFormat::Dot) {
        lines.push("}".to_owned());
    }
    format!("{}\n", lines.join("\n"))
}

fn escape_mermaid_label(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            '&' => "#38;".to_owned(),
            '"' => "#quot;".to_owned(),
            '\\' => "#92;".to_owned(),
            '#' => "#35;".to_owned(),
            ';' => "#59;".to_owned(),
            '|' => "#124;".to_owned(),
            '[' => "#91;".to_owned(),
            ']' => "#93;".to_owned(),
            '{' => "#123;".to_owned(),
            '}' => "#125;".to_owned(),
            '(' => "#40;".to_owned(),
            ')' => "#41;".to_owned(),
            '<' => "#60;".to_owned(),
            '>' => "#62;".to_owned(),
            '\'' => "#39;".to_owned(),
            other => other.to_string(),
        })
        .collect()
}

fn escape_dot_label(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            '\\' => "\\\\".to_owned(),
            '"' => "\\\"".to_owned(),
            '\n' => "\\n".to_owned(),
            '\r' => "\\r".to_owned(),
            other => other.to_string(),
        })
        .collect()
}

fn render_validation(report: &ValidationReport) -> String {
    if report.findings.is_empty() {
        return "PASS manifest is valid".to_owned();
    }
    let mut lines = vec![format!(
        "FAIL manifest validation ({} finding(s))",
        report.findings.len()
    )];
    lines.extend(report.findings.iter().map(render_finding));
    lines.join("\n")
}

fn render_verification(report: &VerificationReport) -> String {
    let heading = if report.status == "pass" {
        "PASS local integrity checks passed"
    } else {
        "FAIL local integrity checks"
    };
    let mut lines = vec![heading.to_owned()];
    for check in &report.checks {
        let availability = check
            .availability
            .map(|status| format!(" availability={}", status.as_str()))
            .unwrap_or_default();
        lines.push(format!(
            "CHECK {} [{}]: {}{}",
            check.id, check.kind, check.status, availability
        ));
    }
    lines.extend(report.findings.iter().map(render_finding));
    lines.join("\n")
}

fn render_lineage(report: &LineageReport) -> String {
    let mut root_metadata = Vec::new();
    if let Some(version) = &report.artifact_version {
        root_metadata.push(format!("version={version}"));
    }
    if let Some(revision) = &report.artifact_revision {
        root_metadata.push(format!("revision={revision}"));
    }
    if let Some(path) = &report.artifact_path {
        root_metadata.push(format!("path={path}"));
    }
    let metadata = if root_metadata.is_empty() {
        String::new()
    } else {
        format!(" {}", root_metadata.join(" "))
    };
    let mut lines = vec![format!(
        "{} [{}]{}",
        report.artifact, report.artifact_status, metadata
    )];
    render_nodes(&report.upstream, "", &mut lines);
    if report.upstream.is_empty() {
        lines.push("└── (no upstream provenance)".to_owned());
    }
    for finding in &report.findings {
        if finding.code == "lineage_cycle" {
            lines.push(render_finding(finding));
        }
    }
    lines.join("\n")
}

fn render_nodes(nodes: &[LineageNode], prefix: &str, lines: &mut Vec<String>) {
    for (index, node) in nodes.iter().enumerate() {
        let last = index + 1 == nodes.len();
        let branch = if last { "└── " } else { "├── " };
        let state = if node.cycle {
            "CYCLE".to_owned()
        } else {
            node.status.to_string()
        };
        let availability = node
            .availability
            .map(|status| format!(" availability={}", status.as_str()))
            .unwrap_or_default();
        let metadata = [
            node.version
                .as_ref()
                .map(|value| format!("version={value}")),
            node.revision
                .as_ref()
                .map(|value| format!("revision={value}")),
            node.locator
                .as_ref()
                .map(|value| format!("locator={value}")),
            node.path.as_ref().map(|value| format!("path={value}")),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        let metadata = if metadata.is_empty() {
            String::new()
        } else {
            format!(" {}", metadata.join(" "))
        };
        lines.push(format!(
            "{prefix}{branch}{} [{state}]{availability}{metadata}",
            node.id
        ));
        let child_prefix = format!("{prefix}{}", if last { "    " } else { "│   " });
        render_nodes(&node.upstream, &child_prefix, lines);
    }
}

fn render_audit(report: &AuditReport) -> String {
    let heading = if report.status == "pass" {
        "PASS provenance audit"
    } else {
        "FAIL provenance audit"
    };
    let mut lines = vec![format!("{heading} ({} finding(s))", report.findings.len())];
    for check in &report.checks {
        let availability = check
            .availability
            .map(|status| format!(" availability={}", status.as_str()))
            .unwrap_or_default();
        lines.push(format!(
            "CHECK {}: {}{}",
            check.id, check.status, availability
        ));
    }
    lines.extend(report.findings.iter().map(render_finding));
    lines.join("\n")
}

fn render_finding(finding: &Finding) -> String {
    let mut location = Vec::new();
    if let Some(id) = &finding.entity_id {
        location.push(format!("entity={id}"));
    }
    if let Some(path) = &finding.path {
        location.push(format!("path={path}"));
    }
    let location = if location.is_empty() {
        String::new()
    } else {
        format!(" {}", location.join(" "))
    };
    format!(
        "[{}] {}{}: {}",
        finding.severity, finding.code, location, finding.message
    )
}
