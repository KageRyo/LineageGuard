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

    assert!(manifest
        .lines()
        .any(|line| line.trim() == "version = \"0.2.0\""));
    assert_eq!(action_version.trim(), "v0.2.0");
    assert!(action.contains("using: composite"));
    assert!(action.contains("default: ."));
    assert!(action.contains("scripts/lineageguard-action.sh"));
    assert!(wrapper.contains("sha256sum --check --strict"));
    assert!(wrapper.contains("for command in validate verify"));
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
