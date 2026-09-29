#!/usr/bin/env bash
set -euo pipefail
export LC_ALL=C

if [[ $# -lt 1 || $# -gt 2 || -z "${1:-}" ]]; then
  echo "Usage: lineageguard-action.sh <project-path> [test-release-file-url]" >&2
  exit 2
fi

dataset_path="$1"
if [[ "$dataset_path" == /* ]]; then
  echo "LineageGuard input path must be relative to the GitHub workspace: $dataset_path" >&2
  exit 2
fi
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
action_root="$(cd -- "$script_dir/.." && pwd -P)"
release_version="$(<"$action_root/action-version.txt")"

if [[ ! "$release_version" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Invalid Action binary version in action-version.txt: $release_version" >&2
  exit 2
fi

if [[ "${RUNNER_OS:-}" != Linux || "${RUNNER_ARCH:-}" != X64 ]]; then
  echo "LineageGuard Action supports Linux x86_64 runners; got ${RUNNER_OS:-unknown}/${RUNNER_ARCH:-unknown}." >&2
  exit 2
fi

if [[ $# -eq 2 ]]; then
  release_base_url="$2"
  if [[ "$release_base_url" != file:///* ]]; then
    echo "The optional test release URL must use file://." >&2
    exit 2
  fi
  curl_options=(--proto '=file' --fail --silent --show-error)
else
  release_base_url="https://github.com/KageRyo/LineageGuard/releases/download/$release_version"
  curl_options=(--proto '=https' --proto-redir '=https' --fail --silent --show-error --location --connect-timeout 20 --max-time 120)
fi

archive="lineageguard-${release_version}-linux-x86_64.tar.gz"
temp_parent="${RUNNER_TEMP:-${TMPDIR:-/tmp}}"
mkdir -p -- "$temp_parent"
tool_dir="$(mktemp -d "${temp_parent%/}/lineageguard-action.XXXXXX")"
trap 'rm -rf -- "$tool_dir"' EXIT

curl "${curl_options[@]}" --output "$tool_dir/$archive" "${release_base_url%/}/$archive"
curl "${curl_options[@]}" --output "$tool_dir/SHA256SUMS" "${release_base_url%/}/SHA256SUMS"

selected_checksum="$tool_dir/selected.sha256"
: > "$selected_checksum"
matches=0
while IFS= read -r line; do
  [[ "$line" == *"  "* ]] || continue
  checksum="${line%%  *}"
  filename="${line#*  }"
  if [[ "$filename" == "$archive" ]]; then
    if [[ ${#checksum} -ne 64 || "$checksum" == *[!0-9a-fA-F]* ]]; then
      echo "Invalid SHA-256 entry for $archive." >&2
      exit 1
    fi
    matches=$((matches + 1))
    printf '%s  %s\n' "$checksum" "$archive" >> "$selected_checksum"
  fi
done < "$tool_dir/SHA256SUMS"

if [[ "$matches" -ne 1 ]]; then
  echo "Expected exactly one SHA256SUMS entry for $archive; found $matches." >&2
  exit 1
fi

if ! (cd -- "$tool_dir" && sha256sum --check --strict selected.sha256); then
  echo "LineageGuard archive checksum verification failed." >&2
  exit 1
fi

tar -tzf "$tool_dir/$archive" > "$tool_dir/archive-files.txt"
sort "$tool_dir/archive-files.txt" > "$tool_dir/archive-files.sorted"
printf '%s\n' LICENSE THIRD-PARTY-LICENSES.txt lineageguard | sort > "$tool_dir/expected-files.txt"
if ! diff -u "$tool_dir/expected-files.txt" "$tool_dir/archive-files.sorted"; then
  echo "Unexpected files in LineageGuard archive." >&2
  exit 1
fi

tar -xzf "$tool_dir/$archive" -C "$tool_dir" -- lineageguard
if [[ ! -f "$tool_dir/lineageguard" || -L "$tool_dir/lineageguard" ]]; then
  echo "LineageGuard executable is missing or is not a regular file." >&2
  exit 1
fi
chmod 0755 "$tool_dir/lineageguard"

workspace="${GITHUB_WORKSPACE:?GITHUB_WORKSPACE must be set}"
if [[ ! -d "$workspace" ]]; then
  echo "GitHub workspace directory does not exist: $workspace" >&2
  exit 2
fi
workspace="$(cd -- "$workspace" && pwd -P)"
dataset_real="$(realpath -e -- "$workspace/$dataset_path")" || {
  echo "LineageGuard input path does not exist in the workspace: $dataset_path" >&2
  exit 2
}
case "$dataset_real" in
  "$workspace"|"$workspace"/*) ;;
  *)
    echo "LineageGuard input path resolves outside the GitHub workspace: $dataset_path" >&2
    exit 2
    ;;
esac

cd -- "$workspace"
for command in validate verify; do
  set +e
  "$tool_dir/lineageguard" "$command" -- "$dataset_path"
  command_status=$?
  set -e
  if [[ "$command_status" -ne 0 ]]; then
    exit "$command_status"
  fi
done
