# LineageGuard Ubuntu 24 Linux Release Builder Implementation Plan

> **For agentic workers:** This plan is being executed inline under `superpowers:executing-plans`. Steps use checkbox syntax for tracking.

**Goal:** Move Linux release builds to Ubuntu 24.04 while keeping the published GLIBC 2.34 compatibility ceiling and publish the verified result as v0.2.1.

**Architecture:** Build Linux x64 on the supported Ubuntu 24.04 hosted runner. A shared Bash check will inspect the produced ELF for GLIBC requirements above 2.34 and execute it in an Ubuntu 22.04 container before the release artifact is packaged.

**Tech Stack:** Rust/Cargo, GitHub Actions, Bash, `readelf`, Docker.

**Spec:** User request in this session: handle the compatibility migration while preserving the existing Linux binary baseline; prior authorization covers PR, merge after green CI, versioning, and release.

## Global Constraints

- Maximum required GNU libc symbol version remains `GLIBC_2.34`.
- Linux release builds use `ubuntu-24.04`.
- The Linux artifact must start successfully in an `ubuntu:22.04` container.
- The package version and Action binary version must both be `0.2.1` / `v0.2.1`.
- Keep the Windows x64 and macOS ARM64 artifact names and targets unchanged.

## Review Focus

- A new undefined GLIBC symbol above 2.34 must fail CI before packaging.
- The Linux release matrix entry must use Ubuntu 24.04.
- The exact built binary must be run in the older Ubuntu container.
- The published archive and Action examples must use v0.2.1 consistently.
- Windows and macOS release matrix entries must remain intact.

---

### Task 1: Enforce Linux ABI compatibility and release v0.2.1

**Files:**
- Modify: `tests/release_contract.rs`
- Create: `scripts/check-linux-glibc-compatibility.sh`
- Modify: `scripts/test-lineageguard-action.sh`
- Modify: `.github/workflows/ci.yml`
- Modify: `.github/workflows/release.yml`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `action-version.txt`
- Modify: `README.md`
- Create: `docs/releases/v0.2.1.md`

**Interfaces:**
- The compatibility script consumes one path to the just-built Linux binary; it checks the GLIBC symbol ceiling and runs that binary with `--version` in `ubuntu:22.04`.
- CI and release workflows call the same script against their respective Linux build outputs.

- [x] **Step 1: Add failing release contract assertions.** Require v0.2.1 package/Action alignment, Ubuntu 24.04 for the Linux release target, the shared compatibility check in CI and release workflows, and the documented GLIBC 2.34 floor.
- [x] **Step 2: Run the contract test and confirm it fails** because those requirements are not yet present.
- [x] **Step 3: Implement the compatibility script, workflow calls, version bump and lockfile sync, README update, and v0.2.1 release notes.**
- [x] **Step 4: Run the full Rust suite, Action wrapper fixtures, compatibility script against the release binary, YAML parsing, Bash syntax, and `git diff --check`.**
- [x] **Step 5: Commit as `fix: preserve Linux glibc baseline on Ubuntu 24`.**
- [ ] **Step 6: Open the PR, wait for all hosted checks, merge under the user's standing authorization, then publish v0.2.1 after its tag workflow succeeds.**
