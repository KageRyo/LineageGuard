#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"
action_script="$repo_root/scripts/lineageguard-action.sh"
tmp_parent="${RUNNER_TEMP:-${TMPDIR:-/tmp}}"
mkdir -p -- "$tmp_parent"
test_root="$(mktemp -d "${tmp_parent%/}/lineageguard-action-tests.XXXXXX")"
trap 'rm -rf -- "$test_root"' EXIT

release_version="$(<"$repo_root/action-version.txt")"
archive_name="lineageguard-${release_version}-linux-x86_64.tar.gz"

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

fixture() {
  local name="$1"
  local checksum_mode="${2:-valid}"
  local archive_mode="${3:-valid}"
  local root="$test_root/$name"
  local package="$root/package"
  local release="$root/release"
  mkdir -p -- "$package" "$release" "$root/workspace/dataset with spaces" "$root/tmp"

  if [[ "$archive_mode" != missing-binary && "$archive_mode" != symlink-binary ]]; then
    cat > "$package/lineageguard" <<'BINARY'
#!/usr/bin/env bash
set -euo pipefail
printf '%s %s %s\n' "$1" "${2:-}" "${3:-}" >> "$CALL_LOG"
case "$1" in
  validate) exit "${VALIDATE_EXIT_CODE:-0}" ;;
  verify) exit "${VERIFY_EXIT_CODE:-0}" ;;
  *) exit 91 ;;
esac
BINARY
    chmod 0755 "$package/lineageguard"
  elif [[ "$archive_mode" == symlink-binary ]]; then
    ln -s /bin/true "$package/lineageguard"
  fi
  printf 'Apache License\n' > "$package/LICENSE"
  printf 'Third party notices\n' > "$package/THIRD-PARTY-LICENSES.txt"
  if [[ "$archive_mode" == extra-file ]]; then
    printf 'unexpected\n' > "$package/README.txt"
  fi

  local members=(LICENSE THIRD-PARTY-LICENSES.txt)
  if [[ "$archive_mode" != missing-binary ]]; then
    members=(lineageguard "${members[@]}")
  fi
  if [[ "$archive_mode" == extra-file ]]; then
    members+=(README.txt)
  fi
  tar -czf "$release/$archive_name" -C "$package" "${members[@]}"

  case "$checksum_mode" in
    missing) : > "$release/SHA256SUMS" ;;
    duplicate)
      local checksum_line
      checksum_line="$(cd -- "$release" && sha256sum "$archive_name")"
      printf '%s\n%s\n' "$checksum_line" "$checksum_line" > "$release/SHA256SUMS"
      ;;
    malformed) printf 'not-a-digest  %s\n' "$archive_name" > "$release/SHA256SUMS" ;;
    mismatch) printf '%064d  %s\n' 0 "$archive_name" > "$release/SHA256SUMS" ;;
    corrupt-archive) (cd -- "$release" && sha256sum "$archive_name" > SHA256SUMS) ;;
    valid) (cd -- "$release" && sha256sum "$archive_name" > SHA256SUMS) ;;
    *) fail "unknown checksum fixture: $checksum_mode" ;;
  esac

  if [[ "$checksum_mode" == corrupt-archive ]]; then
    printf 'corruption' >> "$release/$archive_name"
  fi
  printf 'version: 1\n' > "$root/workspace/dataset with spaces/lineage.yaml"
  printf '%s\n' "$root"
}

run_action() {
  local root="$1"
  local runner_os="${2:-Linux}"
  local runner_arch="${3:-X64}"
  local validate_code="${4:-0}"
  local verify_code="${5:-0}"
  local requested_path="${6:-dataset with spaces}"
  : > "$root/calls.log"
  set +e
  env \
    RUNNER_OS="$runner_os" \
    RUNNER_ARCH="$runner_arch" \
    GITHUB_WORKSPACE="$root/workspace" \
    RUNNER_TEMP="$root/tmp" \
    CALL_LOG="$root/calls.log" \
    VALIDATE_EXIT_CODE="$validate_code" \
    VERIFY_EXIT_CODE="$verify_code" \
    bash "$action_script" "$requested_path" "file://$root/release" \
      > "$root/stdout" 2> "$root/stderr"
  ACTION_STATUS=$?
  set -e
}

expect_status() {
  local expected="$1"
  [[ "$ACTION_STATUS" == "$expected" ]] || fail "expected status $expected, got $ACTION_STATUS"
}

expect_no_binary_call() {
  [[ ! -s "$1/calls.log" ]] || fail "binary ran for invalid release fixture $1"
}

root="$(fixture success)"
run_action "$root"
expect_status 0
printf 'validate -- dataset with spaces\nverify -- dataset with spaces\n' > "$root/expected.log"
cmp -s "$root/expected.log" "$root/calls.log" || fail "Action did not run validate then verify with the requested path"

root="$(fixture validate-failure)"
run_action "$root" Linux X64 1 0
expect_status 1
printf 'validate -- dataset with spaces\n' > "$root/expected.log"
cmp -s "$root/expected.log" "$root/calls.log" || fail "verify ran after validation failed"

root="$(fixture verify-failure)"
run_action "$root" Linux X64 0 1
expect_status 1
[[ "$(wc -l < "$root/calls.log")" -eq 2 ]] || fail "verify failure did not preserve both command calls"

root="$(fixture unsupported-runner)"
run_action "$root" Windows X64
expect_status 2
expect_no_binary_call "$root"

root="$(fixture absolute-path)"
run_action "$root" Linux X64 0 0 "/dataset with spaces"
expect_status 2
expect_no_binary_call "$root"

for mode in missing duplicate malformed mismatch corrupt-archive; do
  root="$(fixture checksum-$mode "$mode")"
  run_action "$root"
  expect_status 1
  expect_no_binary_call "$root"
done

for mode in extra-file missing-binary symlink-binary; do
  root="$(fixture archive-$mode valid "$mode")"
  run_action "$root"
  expect_status 1
  expect_no_binary_call "$root"
done

echo "PASS LineageGuard Action wrapper fixtures"
