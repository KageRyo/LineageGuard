# LineageGuard

LineageGuard is a local command line tool for checking artifact identity, source provenance, and declared upstream lineage. It answers which inputs an artifact names, whether local copies still match their declared SHA-256 digests, and which provenance gaps remain explicit.

LineageGuard reads a `lineage.yaml` manifest and local files only. It does not fetch URLs, validate dataset schemas or rows, transform data, manage workflows, or host a catalog. ReleaseGuard checks dataset structure such as field types, uniqueness, references, timestamp order, and release-file integrity; LineageGuard follows source and artifact identities across declared derivation edges.

## Build

Install Rust 1.85 or newer, then build the standalone binary:

```sh
cargo build --release --locked
./target/release/lineageguard --help
```

The executable needs no Python or Node.js runtime. Validation and verification make no network requests.

## Quick start

```sh
lineageguard validate examples/basic-lineage
lineageguard verify examples/basic-lineage
lineageguard lineage derived-summary-v1 examples/basic-lineage
lineageguard audit examples/basic-lineage
```

Commands accept either a project directory containing `lineage.yaml` or a direct path to a manifest file. `--format json` returns deterministic machine-readable reports.

The basic example contains synthetic source and artifact files. Its report source is pinned to a local snapshot, then feeds a normalized artifact and a derived summary. Two additional sources are explicitly unknown and unavailable. `validate` and `verify` succeed; `audit` exits 1 and reports those deliberate provenance gaps.

## Manifest version 1

The manifest has one integer version, globally unique source and artifact IDs, and directed lineage edges. An edge points from an input to the artifact derived from it. Version 1 supports `derived_from`; its target must be an artifact, while its input may be a source or another artifact.

```yaml
version: 1
sources:
  official-report:
    status: available
    locator: https://example.gov/report
    version: 2026-edition
    revision: archive-copy-1
    retrieved_at: "2026-09-01T10:00:00Z"
    rights: public-domain
    snapshot:
      path: sources/report.pdf
      sha256: 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
      size_bytes: 4096
  unavailable-archive:
    status: unavailable
    locator: https://example.gov/old-report
artifacts:
  normalized-events-v1:
    path: data/events.csv
    sha256: 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
    size_bytes: 512
    version: v1
lineage:
  - from: official-report
    to: normalized-events-v1
    type: derived_from
  - from: unavailable-archive
    to: normalized-events-v1
    type: derived_from
```

Artifact IDs and source IDs share one namespace. Repeated YAML keys, duplicate edges, conflicting digest or size declarations for one path, unknown IDs, unsupported relationship types, malformed digests, and unsupported manifest versions are rejected. A declared local artifact path requires a SHA-256 digest. A source snapshot always requires both a path and a digest; source snapshots are optional because a remote source may be known without a retained local copy.

Source status describes declared availability, independently of local verification:

| Source status | Meaning | `verify` when no snapshot is declared | `audit` |
| --- | --- | --- | --- |
| `available` | The source is declared obtainable. | Reports unverified; does not fetch it. | Reports a gap if no immutable local snapshot is pinned. |
| `unavailable` | The source is known to be unavailable. | Preserves the status without failing integrity checks. | Reports `source_unavailable`. |
| `unknown` | Availability has not been established. | Preserves the status without failing integrity checks. | Reports `source_unknown`. |
| `not_applicable` | The source does not apply to this manifest. | Preserves the status without failing integrity checks. | Does not report an availability gap. |

A local snapshot can still be hash-verified while its source availability remains unknown or unavailable. A verified SHA-256 proves that the bytes match the manifest declaration; it does not prove who published those bytes, that a locator is authentic, or that a transformation can be replayed.

## Commands and results

```sh
lineageguard validate [PATH]
lineageguard verify [PATH]
lineageguard lineage <ARTIFACT_ID> [PATH]
lineageguard audit [PATH]
```

`validate` checks the manifest schema, IDs, references, digest syntax, path safety, duplicate edges, and dependency cycles. It does not require declared files to exist or hash them. `verify` streams local artifact and snapshot bytes through SHA-256 and checks an optional declared size; it never prints file contents or downloads remote locators. Unknown and unavailable sources without snapshots do not count as integrity failures.

`lineage` prints the requested artifact's upstream tree in stable ID order, including source locator, version, revision, availability, and local verification status where declared. JSON output keeps those fields separate. It reports cycles safely and limits integrity findings to the requested upstream chain.

`audit` combines local integrity checks with provenance gaps. It reports artifacts without upstream lineage, unpinned available sources, unknown or unavailable sources, dependency cycles, and mutable path declarations. It does not calculate a quality score.

Exit codes are `0` for a successful command, `1` for an integrity failure, dependency cycle, or audit finding, and `2` for invalid configuration, malformed input, unsafe paths, or execution errors. Explicit unknown and unavailable source states alone do not make `verify` fail; they do make `audit` report an incomplete chain.

All data paths in the manifest must be relative, use `/`, and stay inside the project root after symlink resolution. Absolute paths, Windows drive paths, `.` or `..` components, and backslashes are rejected. An in-root symlink is accepted but reported by `audit` as `mutable_path`; a symlink that resolves outside the root is rejected. A path component named `current` is accepted and reported as mutable by `audit`.

Successful JSON reports use sorted checks and findings, stable object fields, and no generated timestamp. Error reports use stable reason codes such as `invalid_manifest`, `unsafe_path`, and `unknown_artifact`.

## Examples

`examples/basic-lineage` is the valid synthetic example. `examples/invalid-integrity`, `examples/invalid-graph`, and `examples/invalid-path` demonstrate a hash mismatch, a dependency cycle, and a rejected traversal path. Their expected exit codes are 1, 1, and 2 respectively.

## Limits

Version 1 records artifact-level lineage only. It does not map individual records to sources, execute or record transformation code, fetch remote data, enforce immutable storage, authenticate source publishers, sign manifests, or prove reproducibility of a transformation. Source timestamps and rights are preserved as metadata and are not independently interpreted.

## License

LineageGuard is licensed under [Apache-2.0](LICENSE). Third-party dependency license notices are listed in [THIRD-PARTY-LICENSES.txt](THIRD-PARTY-LICENSES.txt).
