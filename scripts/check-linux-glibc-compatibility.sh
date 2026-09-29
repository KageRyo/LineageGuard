#!/usr/bin/env bash
set -euo pipefail

maximum_glibc_version="GLIBC_2.34"

if [[ $# -ne 1 || -z "${1:-}" ]]; then
  echo "Usage: check-linux-glibc-compatibility.sh <linux-binary>" >&2
  exit 2
fi

binary="$1"
if [[ ! -f "$binary" || ! -x "$binary" ]]; then
  echo "Linux binary must be an executable regular file: $binary" >&2
  exit 2
fi
if ! command -v readelf >/dev/null 2>&1; then
  echo "readelf is required to inspect Linux binary GLIBC requirements." >&2
  exit 2
fi
if ! command -v docker >/dev/null 2>&1; then
  echo "Docker is required to smoke-test the binary in Ubuntu 22.04." >&2
  exit 2
fi

if ! version_info="$(readelf --version-info --wide "$binary")"; then
  echo "readelf failed to inspect GLIBC requirements in $binary; refusing to skip the compatibility check." >&2
  exit 1
fi

if ! required_versions="$(
  printf '%s\n' "$version_info" \
    | awk '/Version needs section/ { in_needs = 1; next } /Version definition section/ { in_needs = 0 } in_needs' \
    | grep -oE 'Name: GLIBC_[0-9]+(\.[0-9]+)+' \
    | sed -E 's/^Name: //' \
    | LC_ALL=C sort -Vu
)"; then
  echo "No GLIBC version requirements could be parsed from $binary; refusing to skip the compatibility check." >&2
  exit 1
fi

highest_required_version="$(printf '%s\n' "$required_versions" | tail -n 1)"
highest_supported_version="$(printf '%s\n%s\n' "$maximum_glibc_version" "$highest_required_version" | LC_ALL=C sort -V | tail -n 1)"
if [[ "$highest_supported_version" != "$maximum_glibc_version" ]]; then
  echo "Linux binary requires $highest_required_version, above the supported $maximum_glibc_version baseline." >&2
  exit 1
fi

printf 'Highest required GLIBC symbol version: %s (maximum: %s)\n' \
  "$highest_required_version" "$maximum_glibc_version"

binary_path="$(realpath "$binary")"
docker run --rm --platform linux/amd64 \
  --volume "$binary_path:/lineageguard:ro" \
  ubuntu:22.04 /lineageguard --version
