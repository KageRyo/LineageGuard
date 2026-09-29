use std::fs;
use std::process::{Command, Output};

use tempfile::TempDir;

fn project() -> TempDir {
    let directory = tempfile::tempdir().expect("temporary project directory");
    fs::create_dir_all(directory.path().join("sources")).expect("create source directory");
    fs::create_dir_all(directory.path().join("data")).expect("create data directory");
    fs::write(
        directory.path().join("sources/report.txt"),
        b"report bytes\n",
    )
    .expect("write source snapshot");
    fs::write(directory.path().join("data/events.csv"), b"event_id\n1\n").expect("write artifact");
    fs::write(
        directory.path().join("lineage.yaml"),
        "version: 1\nsources:\n  official-report:\n    status: available\n    locator: https://example.gov/report\n    revision: report-2026\n    retrieved_at: '2026-09-01T10:00:00Z'\n    rights: public-domain\n    snapshot:\n      path: sources/report.txt\n      sha256: 7b282874e6c72645ce9f4688d5cb40edfeeeaf980bfc7d26e0ad0e940fefcf97\n      size_bytes: 13\nartifacts:\n  events-v1:\n    path: data/events.csv\n    sha256: 8336d8801f75d65bbb33de833eeca48f8079e90ed50accde75b6641ad81e3486\nlineage:\n  - from: official-report\n    to: events-v1\n    type: derived_from\n",
    )
    .expect("write manifest");
    directory
}

fn invoke(args: &[&str], root: &std::path::Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lineageguard"))
        .args(args)
        .arg(root)
        .output()
        .expect("run lineageguard")
}

fn invoke_diff(
    old_manifest: &std::path::Path,
    new_manifest: &std::path::Path,
    format: Option<&str>,
) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_lineageguard"));
    command.arg("diff").arg(old_manifest).arg(new_manifest);
    if let Some(format) = format {
        command.args(["--format", format]);
    }
    command.output().expect("run lineageguard diff")
}

fn write_named_manifest(root: &std::path::Path, name: &str, contents: &str) -> std::path::PathBuf {
    let path = root.join(name);
    fs::write(&path, contents).expect("write named manifest");
    path
}

fn write_manifest(root: &std::path::Path, manifest: &str) {
    fs::write(root.join("lineage.yaml"), manifest).expect("write manifest");
}

fn output_json(output: &Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|_| panic!("invalid JSON: {}", String::from_utf8_lossy(&output.stdout)))
}

fn assert_reason(output: &Output, reason: &str) {
    let json = output_json(output);
    let findings = json["findings"].as_array().expect("findings array");
    assert!(
        findings.iter().any(|finding| finding["code"] == reason),
        "expected finding {reason}: {json}"
    );
}

#[test]
fn validate_accepts_a_versioned_source_to_artifact_manifest() {
    let directory = project();
    let output = invoke(&["validate"], directory.path());
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "PASS manifest is valid\n"
    );
}

#[test]
fn verify_checks_bytes_and_keeps_availability_separate() {
    let directory = project();
    write_manifest(
        directory.path(),
        "version: 1\nsources:\n  official-report:\n    status: available\n    locator: https://example.gov/report\n    snapshot:\n      path: sources/report.txt\n      sha256: 7b282874e6c72645ce9f4688d5cb40edfeeeaf980bfc7d26e0ad0e940fefcf97\n  missing-archive:\n    status: unavailable\n    locator: https://example.gov/archive\n  unknown-station:\n    status: unknown\nartifacts:\n  events-v1:\n    path: data/events.csv\n    sha256: 8336d8801f75d65bbb33de833eeca48f8079e90ed50accde75b6641ad81e3486\nlineage:\n  - {from: official-report, to: events-v1, type: derived_from}\n  - {from: missing-archive, to: events-v1, type: derived_from}\n  - {from: unknown-station, to: events-v1, type: derived_from}\n",
    );

    let output = invoke(&["verify", "--format", "json"], directory.path());
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["status"], "pass");
    let checks = json["checks"].as_array().expect("checks array");
    assert!(checks
        .iter()
        .any(|item| item["id"] == "official-report" && item["status"] == "verified"));
    assert!(checks
        .iter()
        .any(|item| item["id"] == "missing-archive" && item["availability"] == "unavailable"));
    assert!(checks
        .iter()
        .any(|item| item["id"] == "unknown-station" && item["availability"] == "unknown"));
}

#[test]
fn verify_reports_a_hash_mismatch_without_printing_file_contents() {
    let directory = project();
    fs::write(
        directory.path().join("data/events.csv"),
        b"secret altered bytes\n",
    )
    .expect("alter artifact");
    let output = invoke(&["verify", "--format", "json"], directory.path());
    assert_eq!(output.status.code(), Some(1));
    assert_reason(&output, "hash_mismatch");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!combined.contains("secret altered bytes"));
}

#[test]
fn graph_defaults_to_mermaid_and_includes_isolated_nodes_and_availability() {
    let directory = project();
    write_manifest(
        directory.path(),
        "version: 1\nsources:\n  official: {status: available}\n  isolated-source: {status: unknown}\nartifacts:\n  derived: {}\n  isolated-artifact: {}\nlineage:\n  - {from: official, to: derived, type: derived_from}\n",
    );

    let output = invoke(&["graph"], directory.path());

    assert_eq!(output.status.code(), Some(0));
    let graph = String::from_utf8(output.stdout).unwrap();
    assert!(graph.starts_with("flowchart LR\n"));
    assert!(graph.contains("source: official #40;availability=available#41;"));
    assert!(graph.contains("source: isolated-source #40;availability=unknown#41;"));
    assert!(graph.contains("artifact: isolated-artifact"));
    assert!(graph.contains("derived_from"));
    assert!(!graph.contains("VERIFIED"));
    assert!(output.stderr.is_empty());
}

#[test]
fn graph_mermaid_escapes_html_like_label_markup() {
    let directory = project();
    write_manifest(
        directory.path(),
        "version: 1\nsources:\n  '<br>': {status: available}\nartifacts:\n  '<b>': {}\nlineage:\n  - {from: '<br>', to: '<b>', type: derived_from}\n",
    );

    let output = invoke(&["graph"], directory.path());

    assert_eq!(output.status.code(), Some(0));
    let graph = String::from_utf8(output.stdout).unwrap();
    assert!(graph.contains("#60;br#62;"));
    assert!(graph.contains("#60;b#62;"));
    assert!(!graph.contains("<br>"));
    assert!(!graph.contains("<b>"));
}

#[test]
fn graph_dot_escapes_labels_and_is_deterministic() {
    let first_directory = project();
    let second_directory = project();
    let first_manifest = "version: 1\nsources:\n  'source \\\"quoted\\\" \\\\ [x]': {status: available}\n  isolated: {status: not_applicable}\nartifacts:\n  'artifact \\\"result\\\"': {path: data/not-materialized.csv, sha256: 8336d8801f75d65bbb33de833eeca48f8079e90ed50accde75b6641ad81e3486}\nlineage:\n  - {from: 'source \\\"quoted\\\" \\\\ [x]', to: 'artifact \\\"result\\\"', type: derived_from}\n";
    let second_manifest = "version: 1\nsources:\n  isolated: {status: not_applicable}\n  'source \\\"quoted\\\" \\\\ [x]': {status: available}\nartifacts:\n  'artifact \\\"result\\\"': {path: data/not-materialized.csv, sha256: 8336d8801f75d65bbb33de833eeca48f8079e90ed50accde75b6641ad81e3486}\nlineage:\n  - {from: 'source \\\"quoted\\\" \\\\ [x]', to: 'artifact \\\"result\\\"', type: derived_from}\n";
    write_manifest(first_directory.path(), first_manifest);
    write_manifest(second_directory.path(), second_manifest);

    let first = invoke(&["graph", "--format", "dot"], first_directory.path());
    let second = invoke(&["graph", "--format", "dot"], second_directory.path());

    assert_eq!(first.status.code(), Some(0));
    assert_eq!(first.stdout, second.stdout);
    let graph = String::from_utf8(first.stdout).unwrap();
    assert!(graph.starts_with("digraph lineage {\n"));
    assert!(graph.contains(r#"\\\"quoted\\\""#));
    assert!(graph.contains("\\\\"));
    assert!(graph.contains("availability=not_applicable"));
    assert!(!graph.contains("not-materialized.csv"));
    assert!(!graph.contains("VERIFIED"));
}

#[test]
fn graph_fails_without_partial_output_for_cycles_or_incompatible_format() {
    let directory = project();
    write_manifest(
        directory.path(),
        "version: 1\nartifacts:\n  a: {}\n  b: {}\nlineage:\n  - {from: a, to: b, type: derived_from}\n  - {from: b, to: a, type: derived_from}\n",
    );

    let cycle = invoke(&["graph"], directory.path());
    assert_eq!(cycle.status.code(), Some(1));
    assert!(cycle.stdout.is_empty());
    assert!(String::from_utf8_lossy(&cycle.stderr).contains("lineage_cycle"));

    let incompatible = invoke(&["graph", "--format", "json"], directory.path());
    assert_eq!(incompatible.status.code(), Some(2));
    assert!(incompatible.stdout.is_empty());
    assert!(String::from_utf8_lossy(&incompatible.stderr).contains("mermaid or dot"));
}

#[test]
fn diff_reports_entity_fields_and_edge_changes_as_json() {
    let old_root = tempfile::tempdir().expect("old manifest directory");
    let new_root = tempfile::tempdir().expect("new manifest directory");
    let old_manifest = write_named_manifest(
        old_root.path(),
        "old.yaml",
        "version: 1\nsources:\n  keep: {status: unknown}\n  removed-source: {status: unavailable}\n  snapshot-source:\n    status: available\n    snapshot: {path: sources/old.bin, sha256: 0000000000000000000000000000000000000000000000000000000000000000}\nartifacts:\n  keep-artifact:\n    path: data/old.csv\n    sha256: 0000000000000000000000000000000000000000000000000000000000000000\n  removed-artifact: {}\nlineage:\n  - {from: keep, to: keep-artifact, type: derived_from}\n  - {from: removed-source, to: keep-artifact, type: derived_from}\n",
    );
    let new_manifest = write_named_manifest(
        new_root.path(),
        "new.yaml",
        "version: 1\nsources:\n  added-source: {status: available}\n  keep: {status: available, revision: rev-2}\n  snapshot-source:\n    status: available\n    snapshot: {path: sources/new.bin, sha256: 0000000000000000000000000000000000000000000000000000000000000000}\nartifacts:\n  added-artifact: {}\n  keep-artifact:\n    path: data/new.csv\n    sha256: 1111111111111111111111111111111111111111111111111111111111111111\nlineage:\n  - {from: keep, to: keep-artifact, type: derived_from}\n  - {from: added-source, to: added-artifact, type: derived_from}\n",
    );

    let output = invoke_diff(&old_manifest, &new_manifest, Some("json"));

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = output_json(&output);
    assert_eq!(report["command"], "diff");
    assert_eq!(report["status"], "changed");
    assert_eq!(report["summary"]["sources"]["added"], 1);
    assert_eq!(report["summary"]["sources"]["removed"], 1);
    assert_eq!(report["summary"]["sources"]["changed"], 2);
    assert_eq!(report["summary"]["artifacts"]["added"], 1);
    assert_eq!(report["summary"]["artifacts"]["removed"], 1);
    assert_eq!(report["summary"]["artifacts"]["changed"], 1);
    assert_eq!(report["summary"]["edges"]["added"], 1);
    assert_eq!(report["summary"]["edges"]["removed"], 1);
    assert_eq!(report["summary"]["has_changes"], true);

    let changes = report["changes"].as_array().expect("changes array");
    let added_source = changes
        .iter()
        .find(|change| change["kind"] == "source" && change["id"] == "added-source")
        .expect("added source");
    assert_eq!(added_source["fields"][0]["path"], "status");
    assert_eq!(added_source["fields"][0]["new"], "available");
    let removed_source = changes
        .iter()
        .find(|change| change["kind"] == "source" && change["id"] == "removed-source")
        .expect("removed source");
    assert_eq!(removed_source["fields"][0]["path"], "status");
    assert_eq!(removed_source["fields"][0]["old"], "unavailable");

    let source_update = changes
        .iter()
        .find(|change| change["kind"] == "source" && change["id"] == "keep")
        .expect("updated source");
    assert_eq!(source_update["change"], "changed");
    let status_change = source_update["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|field| field["path"] == "status")
        .expect("status field change");
    assert_eq!(status_change["old"], "unknown");
    assert_eq!(status_change["new"], "available");
    let revision_change = source_update["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|field| field["path"] == "revision")
        .expect("added revision field");
    assert!(revision_change.get("old").is_none());
    assert_eq!(revision_change["new"], "rev-2");

    let snapshot_update = changes
        .iter()
        .find(|change| change["kind"] == "source" && change["id"] == "snapshot-source")
        .expect("updated source snapshot");
    assert_eq!(snapshot_update["fields"][0]["path"], "snapshot.path");
    assert_eq!(snapshot_update["fields"][0]["old"], "sources/old.bin");
    assert_eq!(snapshot_update["fields"][0]["new"], "sources/new.bin");

    let artifact_update = changes
        .iter()
        .find(|change| change["kind"] == "artifact" && change["id"] == "keep-artifact")
        .expect("updated artifact");
    assert_eq!(artifact_update["fields"][0]["path"], "path");
    assert_eq!(artifact_update["fields"][0]["old"], "data/old.csv");
    assert_eq!(artifact_update["fields"][0]["new"], "data/new.csv");

    assert!(changes.iter().any(|change| {
        change["kind"] == "edge"
            && change["change"] == "added"
            && change["from"] == "added-source"
            && change["to"] == "added-artifact"
    }));
    assert!(changes.iter().any(|change| {
        change["kind"] == "edge"
            && change["change"] == "removed"
            && change["from"] == "removed-source"
            && change["to"] == "keep-artifact"
    }));
}

#[test]
fn diff_text_shows_summary_and_field_values() {
    let old_root = tempfile::tempdir().expect("old manifest directory");
    let new_root = tempfile::tempdir().expect("new manifest directory");
    let old_manifest = write_named_manifest(
        old_root.path(),
        "old.yaml",
        "version: 1\nsources:\n  source-a: {status: unknown}\nartifacts:\n  result:\n    path: data/old.csv\n    sha256: 0000000000000000000000000000000000000000000000000000000000000000\nlineage: []\n",
    );
    let new_manifest = write_named_manifest(
        new_root.path(),
        "new.yaml",
        "version: 1\nsources:\n  source-a: {status: available}\nartifacts:\n  result:\n    path: data/new.csv\n    sha256: 1111111111111111111111111111111111111111111111111111111111111111\nlineage: []\n",
    );

    let output = invoke_diff(&old_manifest, &new_manifest, None);

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Sources: +0 added, -0 removed, ~1 changed"));
    assert!(text.contains("Artifacts: +0 added, -0 removed, ~1 changed"));
    assert!(text.contains("Edges: +0 added, -0 removed"));
    assert!(text.contains("status: \"unknown\" -> \"available\""));
    assert!(text.contains("path: \"data/old.csv\" -> \"data/new.csv\""));
}

#[test]
fn diff_is_deterministic_and_does_not_read_payloads() {
    let first_old = tempfile::tempdir().expect("first old directory");
    let first_new = tempfile::tempdir().expect("first new directory");
    let second_old = tempfile::tempdir().expect("second old directory");
    let second_new = tempfile::tempdir().expect("second new directory");
    let old_a = "version: 1\nsources:\n  z-source: {status: unknown}\n  a-source: {status: available}\nartifacts:\n  z-result: {path: missing/old.csv, sha256: 0000000000000000000000000000000000000000000000000000000000000000}\n  a-result: {}\nlineage:\n  - {from: z-source, to: z-result, type: derived_from}\n  - {from: a-source, to: z-result, type: derived_from}\n";
    let old_b = "version: 1\nsources:\n  a-source: {status: available}\n  z-source: {status: unknown}\nartifacts:\n  a-result: {}\n  z-result: {path: missing/old.csv, sha256: 0000000000000000000000000000000000000000000000000000000000000000}\nlineage:\n  - {from: a-source, to: z-result, type: derived_from}\n  - {from: z-source, to: z-result, type: derived_from}\n";
    let new_a = "version: 1\nsources:\n  z-source: {status: available}\n  a-source: {status: available}\nartifacts:\n  z-result: {path: missing/new.csv, sha256: 1111111111111111111111111111111111111111111111111111111111111111}\n  a-result: {}\nlineage:\n  - {from: a-source, to: z-result, type: derived_from}\n  - {from: z-source, to: z-result, type: derived_from}\n";
    let new_b = "version: 1\nsources:\n  a-source: {status: available}\n  z-source: {status: available}\nartifacts:\n  a-result: {}\n  z-result: {path: missing/new.csv, sha256: 1111111111111111111111111111111111111111111111111111111111111111}\nlineage:\n  - {from: z-source, to: z-result, type: derived_from}\n  - {from: a-source, to: z-result, type: derived_from}\n";
    let first_old_manifest = write_named_manifest(first_old.path(), "old.yaml", old_a);
    let first_new_manifest = write_named_manifest(first_new.path(), "new.yaml", new_a);
    let second_old_manifest = write_named_manifest(second_old.path(), "old.yaml", old_b);
    let second_new_manifest = write_named_manifest(second_new.path(), "new.yaml", new_b);

    let first = invoke_diff(&first_old_manifest, &first_new_manifest, Some("json"));
    let second = invoke_diff(&second_old_manifest, &second_new_manifest, Some("json"));

    assert_eq!(first.status.code(), Some(0));
    assert_eq!(second.status.code(), Some(0));
    assert_eq!(first.stdout, second.stdout);
}

#[test]
fn diff_reports_unchanged_manifests_and_rejects_invalid_formats_or_yaml() {
    let root = tempfile::tempdir().expect("manifest directory");
    let contents = "version: 1\nartifacts:\n  result: {}\n";
    let old_manifest = write_named_manifest(root.path(), "old.yaml", contents);
    let new_manifest = write_named_manifest(root.path(), "new.yaml", contents);

    let unchanged = invoke_diff(&old_manifest, &new_manifest, Some("json"));
    assert_eq!(unchanged.status.code(), Some(0));
    let report = output_json(&unchanged);
    assert_eq!(report["status"], "unchanged");
    assert_eq!(report["summary"]["has_changes"], false);
    assert_eq!(report["changes"].as_array().unwrap().len(), 0);

    let invalid_format = invoke_diff(&old_manifest, &new_manifest, Some("mermaid"));
    assert_eq!(invalid_format.status.code(), Some(2));
    assert!(invalid_format.stdout.is_empty());

    let invalid_manifest = write_named_manifest(root.path(), "invalid.yaml", "version: [");
    let invalid_yaml = invoke_diff(&invalid_manifest, &new_manifest, None);
    assert_eq!(invalid_yaml.status.code(), Some(2));
    assert!(invalid_yaml.stdout.is_empty());
    assert!(String::from_utf8_lossy(&invalid_yaml.stderr).contains("invalid_manifest"));
}

#[test]
fn diff_reports_manifest_version_changes_and_duplicate_edge_removals() {
    let old_root = tempfile::tempdir().expect("old manifest directory");
    let new_root = tempfile::tempdir().expect("new manifest directory");
    let old_manifest = write_named_manifest(
        old_root.path(),
        "old.yaml",
        "version: 1\nsources:\n  source-a: {status: available}\nartifacts:\n  result: {}\nlineage:\n  - {from: source-a, to: result, type: derived_from}\n  - {from: source-a, to: result, type: derived_from}\n",
    );
    let new_manifest = write_named_manifest(
        new_root.path(),
        "new.yaml",
        "version: 2\nsources:\n  source-a: {status: available}\nartifacts:\n  result: {}\nlineage:\n  - {from: source-a, to: result, type: derived_from}\n",
    );

    let output = invoke_diff(&old_manifest, &new_manifest, Some("json"));

    assert_eq!(output.status.code(), Some(0));
    let report = output_json(&output);
    assert_eq!(report["summary"]["manifest_version_changed"], true);
    assert_eq!(report["summary"]["edges"]["removed"], 1);
    let version_change = report["changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|change| change["kind"] == "manifest")
        .expect("manifest version change");
    assert_eq!(version_change["fields"][0]["old"], 1);
    assert_eq!(version_change["fields"][0]["new"], 2);
}

#[test]
fn diff_text_escapes_control_characters_in_manifest_ids() {
    let old_root = tempfile::tempdir().expect("old manifest directory");
    let new_root = tempfile::tempdir().expect("new manifest directory");
    let old_manifest = write_named_manifest(
        old_root.path(),
        "old.yaml",
        "version: 1\nartifacts:\n  result: {}\n",
    );
    let new_manifest = write_named_manifest(
        new_root.path(),
        "new.yaml",
        "version: 1\nsources:\n  \"unsafe\\u001b[31m\": {status: available}\nartifacts:\n  result: {}\n",
    );

    let output = invoke_diff(&old_manifest, &new_manifest, None);

    assert_eq!(output.status.code(), Some(0));
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(!text.contains('\u{1b}'));
    assert!(text.contains("source \"unsafe\\u001b[31m\""));
}

#[test]
fn verify_reports_a_missing_declared_artifact() {
    let directory = project();
    fs::remove_file(directory.path().join("data/events.csv")).expect("remove artifact");
    let output = invoke(&["verify", "--format", "json"], directory.path());
    assert_eq!(output.status.code(), Some(1));
    assert_reason(&output, "artifact_missing");
}

#[test]
fn verify_reports_a_declared_size_mismatch() {
    let directory = project();
    write_manifest(
        directory.path(),
        "version: 1\nartifacts:\n  events-v1:\n    path: data/events.csv\n    sha256: 8336d8801f75d65bbb33de833eeca48f8079e90ed50accde75b6641ad81e3486\n    size_bytes: 999\n",
    );
    let output = invoke(&["verify", "--format", "json"], directory.path());
    assert_eq!(output.status.code(), Some(1));
    assert_reason(&output, "size_mismatch");
}

#[test]
fn verify_reports_a_missing_declared_source_snapshot() {
    let directory = project();
    write_manifest(
        directory.path(),
        "version: 1\nsources:\n  source-a:\n    status: available\n    snapshot:\n      path: sources/missing.txt\n      sha256: 7b282874e6c72645ce9f4688d5cb40edfeeeaf980bfc7d26e0ad0e940fefcf97\nartifacts:\n  events-v1: {path: data/events.csv, sha256: 8336d8801f75d65bbb33de833eeca48f8079e90ed50accde75b6641ad81e3486}\nlineage:\n  - {from: source-a, to: events-v1, type: derived_from}\n",
    );
    let output = invoke(&["verify", "--format", "json"], directory.path());
    assert_eq!(output.status.code(), Some(1));
    assert_reason(&output, "source_snapshot_missing");
}

#[test]
fn verify_keeps_not_applicable_explicit_without_failing_integrity() {
    let directory = project();
    write_manifest(
        directory.path(),
        "version: 1\nsources:\n  out-of-scope: {status: not_applicable}\nartifacts:\n  events-v1: {path: data/events.csv, sha256: 8336d8801f75d65bbb33de833eeca48f8079e90ed50accde75b6641ad81e3486}\n",
    );
    let output = invoke(&["verify", "--format", "json"], directory.path());
    assert_eq!(output.status.code(), Some(0));
    let check = output_json(&output)["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["id"] == "out-of-scope")
        .unwrap()
        .clone();
    assert_eq!(check["availability"], "not_applicable");
    assert_eq!(check["status"], "not_applicable");
}

#[test]
fn validate_rejects_a_not_applicable_source_used_by_lineage() {
    let directory = project();
    write_manifest(
        directory.path(),
        "version: 1\nsources:\n  out-of-scope: {status: not_applicable}\nartifacts:\n  events-v1: {path: data/events.csv, sha256: 8336d8801f75d65bbb33de833eeca48f8079e90ed50accde75b6641ad81e3486}\nlineage:\n  - {from: out-of-scope, to: events-v1, type: derived_from}\n",
    );
    let output = invoke(&["validate", "--format", "json"], directory.path());
    assert_eq!(output.status.code(), Some(2));
    let error = output_json(&output)["error"].clone();
    assert_eq!(error["code"], "invalid_manifest");
    assert!(error["message"]
        .as_str()
        .expect("error message")
        .contains("not_applicable"));
}

#[test]
fn validate_rejects_a_missing_lineage_reference_as_configuration_error() {
    let directory = project();
    write_manifest(
        directory.path(),
        "version: 1\nartifacts:\n  events-v1:\n    path: data/events.csv\n    sha256: 8336d8801f75d65bbb33de833eeca48f8079e90ed50accde75b6641ad81e3486\nlineage:\n  - {from: absent-source, to: events-v1, type: derived_from}\n",
    );
    let output = invoke(&["validate", "--format", "json"], directory.path());
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(output_json(&output)["error"]["code"], "invalid_manifest");
}

#[test]
fn validate_rejects_duplicate_entity_ids() {
    let directory = project();
    write_manifest(
        directory.path(),
        "version: 1\nartifacts:\n  events-v1: {path: data/events.csv, sha256: 8336d8801f75d65bbb33de833eeca48f8079e90ed50accde75b6641ad81e3486}\n  events-v1: {path: data/other.csv, sha256: 8336d8801f75d65bbb33de833eeca48f8079e90ed50accde75b6641ad81e3486}\n",
    );
    let output = invoke(&["validate", "--format", "json"], directory.path());
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(output_json(&output)["error"]["code"], "invalid_manifest");
}

#[test]
fn validate_rejects_conflicting_hashes_for_one_declared_path() {
    let directory = project();
    write_manifest(
        directory.path(),
        "version: 1\nsources:\n  source-copy:\n    status: available\n    snapshot:\n      path: data/events.csv\n      sha256: 7b282874e6c72645ce9f4688d5cb40edfeeeaf980bfc7d26e0ad0e940fefcf97\nartifacts:\n  events-v1:\n    path: data/events.csv\n    sha256: 8336d8801f75d65bbb33de833eeca48f8079e90ed50accde75b6641ad81e3486\n",
    );
    let output = invoke(&["validate", "--format", "json"], directory.path());
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(output_json(&output)["error"]["code"], "invalid_manifest");
}

#[test]
fn validate_rejects_malformed_sha256() {
    let directory = project();
    write_manifest(
        directory.path(),
        "version: 1\nartifacts:\n  events-v1: {path: data/events.csv, sha256: ABCD}\n",
    );
    let output = invoke(&["validate", "--format", "json"], directory.path());
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(output_json(&output)["error"]["code"], "invalid_manifest");
}

#[test]
fn validate_rejects_parent_traversal_paths() {
    let directory = project();
    write_manifest(
        directory.path(),
        "version: 1\nartifacts:\n  events-v1: {path: ../outside.csv, sha256: 8336d8801f75d65bbb33de833eeca48f8079e90ed50accde75b6641ad81e3486}\n",
    );
    let output = invoke(&["validate", "--format", "json"], directory.path());
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(output_json(&output)["error"]["code"], "unsafe_path");
}

#[test]
fn validate_rejects_absolute_paths() {
    let directory = project();
    write_manifest(
        directory.path(),
        "version: 1\nartifacts:\n  events-v1: {path: /outside/events.csv, sha256: 8336d8801f75d65bbb33de833eeca48f8079e90ed50accde75b6641ad81e3486}\n",
    );
    let output = invoke(&["validate", "--format", "json"], directory.path());
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(output_json(&output)["error"]["code"], "unsafe_path");
}

#[test]
fn validate_rejects_windows_drive_paths_on_every_platform() {
    let directory = project();
    write_manifest(
        directory.path(),
        "version: 1\nartifacts:\n  events-v1: {path: C:/outside/events.csv, sha256: 8336d8801f75d65bbb33de833eeca48f8079e90ed50accde75b6641ad81e3486}\n",
    );
    let output = invoke(&["validate", "--format", "json"], directory.path());
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(output_json(&output)["error"]["code"], "unsafe_path");
}

#[test]
fn validate_rejects_windows_nonportable_components_on_every_platform() {
    let directory = project();
    for path in [
        "data/events.csv:metadata",
        "data/NUL",
        "data/NUL.txt",
        "data/COM1.log",
        "data/LPT³.csv",
        "data/trailing.",
        "data/trailing ",
        "data/invalid?.csv",
    ] {
        let manifest = format!(
            "version: 1\nartifacts:\n  events-v1:\n    path: \"{path}\"\n    sha256: 8336d8801f75d65bbb33de833eeca48f8079e90ed50accde75b6641ad81e3486\n"
        );
        write_manifest(directory.path(), &manifest);
        let output = invoke(&["validate", "--format", "json"], directory.path());
        assert_eq!(
            output.status.code(),
            Some(2),
            "path should be rejected on every platform: {path:?}"
        );
        assert_eq!(output_json(&output)["error"]["code"], "unsafe_path");
    }
}

#[cfg(unix)]
#[test]
fn validate_rejects_a_symlink_that_escapes_the_project_root() {
    use std::os::unix::fs::symlink;

    let directory = project();
    let outside = tempfile::tempdir().expect("outside directory");
    fs::write(outside.path().join("external.csv"), b"outside\n").expect("outside payload");
    symlink(
        outside.path().join("external.csv"),
        directory.path().join("data/escape.csv"),
    )
    .expect("create external symlink");
    write_manifest(
        directory.path(),
        "version: 1\nartifacts:\n  events-v1: {path: data/escape.csv, sha256: 8336d8801f75d65bbb33de833eeca48f8079e90ed50accde75b6641ad81e3486}\n",
    );
    let output = invoke(&["validate", "--format", "json"], directory.path());
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(output_json(&output)["error"]["code"], "unsafe_path");
}

#[cfg(unix)]
#[test]
fn audit_marks_an_in_root_symlink_as_a_mutable_path() {
    use std::os::unix::fs::symlink;

    let directory = project();
    fs::write(
        directory.path().join("data/events-immutable.csv"),
        b"event_id\n1\n",
    )
    .expect("write target");
    symlink(
        directory.path().join("data/events-immutable.csv"),
        directory.path().join("data/events-link.csv"),
    )
    .expect("create internal symlink");
    write_manifest(
        directory.path(),
        "version: 1\nartifacts:\n  events-v1: {path: data/events-link.csv, sha256: 8336d8801f75d65bbb33de833eeca48f8079e90ed50accde75b6641ad81e3486}\nlineage: []\n",
    );
    let output = invoke(&["audit", "--format", "json"], directory.path());
    assert_eq!(output.status.code(), Some(1));
    assert_reason(&output, "mutable_path");
}

#[test]
fn audit_allows_but_flags_a_current_path_component() {
    let directory = project();
    fs::create_dir_all(directory.path().join("data/current")).expect("create mutable path");
    fs::write(
        directory.path().join("data/current/events.csv"),
        b"event_id\n1\n",
    )
    .expect("write current artifact");
    write_manifest(
        directory.path(),
        "version: 1\nartifacts:\n  events-v1: {path: data/current/events.csv, sha256: 8336d8801f75d65bbb33de833eeca48f8079e90ed50accde75b6641ad81e3486}\nlineage: []\n",
    );
    let validation = invoke(&["validate"], directory.path());
    assert_eq!(validation.status.code(), Some(0));
    let audit = invoke(&["audit", "--format", "json"], directory.path());
    assert_eq!(audit.status.code(), Some(1));
    assert_reason(&audit, "mutable_path");
}

#[test]
fn lineage_traverses_generations_and_sorts_multiple_parents() {
    let directory = project();
    write_manifest(
        directory.path(),
        "version: 1\nsources:\n  z-source: {status: available}\n  a-source:\n    status: available\n    locator: https://example.test/a-source\n    version: source-v2\n    revision: rev-a\n    retrieved_at: '2026-09-01T00:00:00Z'\n    rights: synthetic-test\nartifacts:\n  middle: {path: data/events.csv, sha256: 8336d8801f75d65bbb33de833eeca48f8079e90ed50accde75b6641ad81e3486}\n  result: {path: data/events.csv, sha256: 8336d8801f75d65bbb33de833eeca48f8079e90ed50accde75b6641ad81e3486}\nlineage:\n  - {from: z-source, to: middle, type: derived_from}\n  - {from: middle, to: result, type: derived_from}\n  - {from: z-source, to: result, type: derived_from}\n  - {from: a-source, to: result, type: derived_from}\n",
    );
    let output = invoke(&["lineage", "result", "--format", "json"], directory.path());
    assert_eq!(output.status.code(), Some(0));
    let json = output_json(&output);
    assert_eq!(json["status"], "pass");
    assert_eq!(json["artifact"], "result");
    assert_eq!(json["upstream"][0]["id"], "a-source");
    assert_eq!(
        json["upstream"][0]["locator"],
        "https://example.test/a-source"
    );
    assert_eq!(json["upstream"][0]["version"], "source-v2");
    assert_eq!(json["upstream"][0]["revision"], "rev-a");
    assert_eq!(json["upstream"][0]["rights"], "synthetic-test");
    assert_eq!(json["upstream"][1]["id"], "middle");
    assert_eq!(json["upstream"][2]["id"], "z-source");
}

#[test]
fn lineage_ignores_integrity_failures_outside_the_requested_ancestry() {
    let directory = project();
    write_manifest(
        directory.path(),
        "version: 1\nsources:\n  source-a: {status: available}\nartifacts:\n  wanted: {path: data/events.csv, sha256: 8336d8801f75d65bbb33de833eeca48f8079e90ed50accde75b6641ad81e3486}\n  unrelated: {path: data/missing.csv, sha256: 8336d8801f75d65bbb33de833eeca48f8079e90ed50accde75b6641ad81e3486}\nlineage:\n  - {from: source-a, to: wanted, type: derived_from}\n",
    );
    let output = invoke(&["lineage", "wanted", "--format", "json"], directory.path());
    assert_eq!(output.status.code(), Some(0));
    assert!(output_json(&output)["findings"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn lineage_ignores_a_cycle_outside_the_requested_ancestry() {
    let directory = project();
    write_manifest(
        directory.path(),
        "version: 1\nsources:\n  source-a: {status: available}\nartifacts:\n  wanted: {path: data/events.csv, sha256: 8336d8801f75d65bbb33de833eeca48f8079e90ed50accde75b6641ad81e3486}\n  loop-a: {path: data/events.csv, sha256: 8336d8801f75d65bbb33de833eeca48f8079e90ed50accde75b6641ad81e3486}\n  loop-b: {path: data/events.csv, sha256: 8336d8801f75d65bbb33de833eeca48f8079e90ed50accde75b6641ad81e3486}\nlineage:\n  - {from: source-a, to: wanted, type: derived_from}\n  - {from: loop-a, to: loop-b, type: derived_from}\n  - {from: loop-b, to: loop-a, type: derived_from}\n",
    );
    let output = invoke(&["lineage", "wanted", "--format", "json"], directory.path());
    assert_eq!(output.status.code(), Some(0));
    assert!(output_json(&output)["findings"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn cycles_are_reported_by_validate_and_lineage_without_recursing_forever() {
    let directory = project();
    write_manifest(
        directory.path(),
        "version: 1\nartifacts:\n  a: {path: data/events.csv, sha256: 8336d8801f75d65bbb33de833eeca48f8079e90ed50accde75b6641ad81e3486}\n  b: {path: data/events.csv, sha256: 8336d8801f75d65bbb33de833eeca48f8079e90ed50accde75b6641ad81e3486}\nlineage:\n  - {from: a, to: b, type: derived_from}\n  - {from: b, to: a, type: derived_from}\n",
    );
    let validation = invoke(&["validate", "--format", "json"], directory.path());
    assert_eq!(validation.status.code(), Some(1));
    assert_reason(&validation, "lineage_cycle");

    let lineage = invoke(&["lineage", "a"], directory.path());
    assert_eq!(lineage.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&lineage.stdout).contains("CYCLE"));
}

#[test]
fn audit_reports_unknown_unavailable_and_unprovenanced_artifacts() {
    let directory = project();
    write_manifest(
        directory.path(),
        "version: 1\nsources:\n  unknown-source: {status: unknown}\n  offline-source: {status: unavailable}\nartifacts:\n  covered: {path: data/events.csv, sha256: 8336d8801f75d65bbb33de833eeca48f8079e90ed50accde75b6641ad81e3486}\n  orphan: {path: data/events.csv, sha256: 8336d8801f75d65bbb33de833eeca48f8079e90ed50accde75b6641ad81e3486}\nlineage:\n  - {from: unknown-source, to: covered, type: derived_from}\n  - {from: offline-source, to: covered, type: derived_from}\n",
    );
    let output = invoke(&["audit", "--format", "json"], directory.path());
    assert_eq!(output.status.code(), Some(1));
    assert_reason(&output, "source_unknown");
    assert_reason(&output, "source_unavailable");
    assert_reason(&output, "artifact_without_provenance");
}

#[test]
fn equivalent_manifests_produce_identical_json_reports() {
    let first_directory = project();
    let second_directory = project();
    write_manifest(
        first_directory.path(),
        "version: 1\nsources:\n  z-source: {status: unavailable}\n  a-source: {status: unknown}\nartifacts:\n  z-result: {path: data/events.csv, sha256: 8336d8801f75d65bbb33de833eeca48f8079e90ed50accde75b6641ad81e3486}\nlineage:\n  - {from: z-source, to: z-result, type: derived_from}\n  - {from: a-source, to: z-result, type: derived_from}\n",
    );
    write_manifest(
        second_directory.path(),
        "version: 1\nsources:\n  a-source: {status: unknown}\n  z-source: {status: unavailable}\nartifacts:\n  z-result: {path: data/events.csv, sha256: 8336d8801f75d65bbb33de833eeca48f8079e90ed50accde75b6641ad81e3486}\nlineage:\n  - {from: a-source, to: z-result, type: derived_from}\n  - {from: z-source, to: z-result, type: derived_from}\n",
    );
    let first = invoke(&["audit", "--format", "json"], first_directory.path());
    let second = invoke(&["audit", "--format", "json"], second_directory.path());
    assert_eq!(first.status.code(), Some(1));
    assert_eq!(output_json(&first)["command"], "audit");
    assert_eq!(first.stdout, second.stdout);
}

fn example_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join(name)
}

#[test]
fn synthetic_basic_example_validates_and_verifies() {
    let path = example_path("basic-lineage");
    let validation = invoke(&["validate"], &path);
    assert_eq!(validation.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(validation.stdout).unwrap(),
        "PASS manifest is valid\n"
    );

    let verification = invoke(&["verify"], &path);
    assert_eq!(verification.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&verification.stdout).contains("PASS local integrity checks"));
}

#[test]
fn manifest_diff_example_shows_release_metadata_changes() {
    let output = invoke_diff(
        &example_path("manifest-diff/v1.yaml"),
        &example_path("manifest-diff/v2.yaml"),
        None,
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Sources: +1 added, -1 removed, ~1 changed"));
    assert!(text.contains("Artifacts: +0 added, -0 removed, ~2 changed"));
    assert!(text.contains("Edges: +1 added, -1 removed"));
    assert!(text.contains("revision: \"archive-2025-01\" -> \"archive-2026-01\""));
    assert!(text.contains("+ source \"station-catalog\""));
    assert!(text.contains("- source \"retired-station-feed\""));
}

#[test]
fn broken_example_fixtures_demonstrate_integrity_graph_and_path_failures() {
    let integrity = invoke(
        &["verify", "--format", "json"],
        &example_path("invalid-integrity"),
    );
    assert_eq!(integrity.status.code(), Some(1));
    assert_reason(&integrity, "hash_mismatch");

    let graph = invoke(
        &["validate", "--format", "json"],
        &example_path("invalid-graph"),
    );
    assert_eq!(graph.status.code(), Some(1));
    assert_reason(&graph, "lineage_cycle");

    let unsafe_path = invoke(
        &["validate", "--format", "json"],
        &example_path("invalid-path"),
    );
    assert_eq!(unsafe_path.status.code(), Some(2));
    assert_eq!(output_json(&unsafe_path)["error"]["code"], "unsafe_path");
}
