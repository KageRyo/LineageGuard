use std::fs;
use std::path::Path;

fn read_repo_file(path: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

#[test]
fn action_version_matches_package_and_runs_the_expected_checks() {
    let manifest = read_repo_file("Cargo.toml");
    let action_version = read_repo_file("action-version.txt");
    let action = read_repo_file("action.yml");
    let wrapper = read_repo_file("scripts/lineageguard-action.sh");
    let readme = read_repo_file("README.md");

    assert!(manifest
        .lines()
        .any(|line| line.trim() == "version = \"0.3.0\""));
    assert_eq!(action_version.trim(), "v0.3.0");
    assert!(action.contains("using: composite"));
    assert!(action.contains("default: ."));
    assert!(action.contains("scripts/lineageguard-action.sh"));
    assert!(wrapper.contains("sha256sum --check --strict"));
    assert!(wrapper.contains("for command in validate verify"));
    for required in [
        "lineageguard-v0.3.0-linux-x86_64.tar.gz",
        "lineageguard-v0.3.0-windows-x86_64.zip",
        "lineageguard-v0.3.0-macos-aarch64.tar.gz",
        "uses: KageRyo/LineageGuard@v0.3.0",
    ] {
        assert!(readme.contains(required), "README lacks {required:?}");
    }
}

#[test]
fn manifest_diff_cli_and_v0_3_0_example_are_documented() {
    let readme = read_repo_file("README.md");
    let release_notes = read_repo_file("docs/releases/v0.3.0.md");

    assert!(readme.contains("lineageguard diff OLD NEW"));
    assert!(readme.contains("examples/manifest-diff/v1.yaml"));
    assert!(readme.contains("--format json"));
    assert!(readme.contains("changed manifest version"));
    assert!(release_notes.contains("lineageguard diff"));
    assert!(release_notes.contains("sources, artifacts, and lineage edges"));
}

#[test]
fn linux_release_builder_preserves_the_glibc_234_baseline_on_ubuntu_24() {
    let readme = read_repo_file("README.md");
    let release: serde_json::Value =
        yaml_serde::from_str(&read_repo_file(".github/workflows/release.yml"))
            .expect("release workflow must parse as YAML");
    let linux_build = release["jobs"]["build"]["strategy"]["matrix"]["include"]
        .as_array()
        .expect("release matrix must be an array")
        .iter()
        .find(|entry| entry["target"] == "x86_64-unknown-linux-gnu")
        .expect("release matrix must include Linux x64");
    assert_eq!(linux_build["os"], "ubuntu-24.04");

    let release_steps = release["jobs"]["build"]["steps"]
        .as_array()
        .expect("release build steps must be an array");
    assert!(release_steps.iter().any(|step| {
        step["name"] == "Check Linux GLIBC compatibility"
            && step["run"]
                .as_str()
                .is_some_and(|run| run.contains("scripts/check-linux-glibc-compatibility.sh"))
    }));

    let ci: serde_json::Value = yaml_serde::from_str(&read_repo_file(".github/workflows/ci.yml"))
        .expect("CI workflow must parse as YAML");
    let ci_steps = ci["jobs"]["rust"]["steps"]
        .as_array()
        .expect("Rust CI steps must be an array");
    assert!(ci_steps.iter().any(|step| {
        step["name"] == "Check Linux GLIBC compatibility"
            && step["run"]
                .as_str()
                .is_some_and(|run| run.contains("scripts/check-linux-glibc-compatibility.sh"))
    }));

    let compatibility_check = read_repo_file("scripts/check-linux-glibc-compatibility.sh");
    assert!(compatibility_check.contains("GLIBC_2.34"));
    assert!(compatibility_check.contains("ubuntu:22.04"));
    assert!(readme.contains("glibc 2.34 or newer"));

    let release_notes = read_repo_file("docs/releases/v0.2.1.md");
    assert!(release_notes.contains("Ubuntu 24.04"));
    assert!(release_notes.contains("glibc 2.34 or newer"));
}

#[test]
fn release_workflow_builds_the_three_documented_archives_and_protects_published_assets() {
    let workflow = read_repo_file(".github/workflows/release.yml");

    for required in [
        "x86_64-unknown-linux-gnu",
        "linux-x86_64",
        "x86_64-pc-windows-msvc",
        "windows-x86_64",
        "aarch64-apple-darwin",
        "macos-aarch64",
        "Test and build Unix",
        "Test and build Windows",
        "if: runner.os != 'Windows'",
        "if: runner.os == 'Windows'",
        "lineageguard-${RELEASE_TAG}-${ASSET}.tar.gz",
        "lineageguard-{0}-{1}.zip",
        "SHA256SUMS",
        "Refusing to replace assets on a published release.",
        "gh release create",
        "--draft",
    ] {
        assert!(
            workflow.contains(required),
            "release workflow lacks {required:?}"
        );
    }
}

#[test]
fn action_and_workflow_metadata_parse_as_yaml() {
    for path in [
        "action.yml",
        ".github/workflows/ci.yml",
        ".github/workflows/release.yml",
    ] {
        let source = read_repo_file(path);
        let _: serde_json::Value = yaml_serde::from_str(&source)
            .unwrap_or_else(|error| panic!("invalid YAML in {path}: {error}"));
    }
}
