use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use lineageguard::model::{
    AuditReport, Finding, LineageNode, LineageReport, ValidationReport, VerificationReport,
};
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
    #[arg(long, value_enum, global = true, default_value_t = OutputFormat::Text)]
    format: OutputFormat,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
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
}

fn main() {
    let cli = Cli::parse();
    let format = cli.format;
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
    }
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
