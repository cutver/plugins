#!/usr/bin/env bash
# End-to-end regression test: load the freshly built `github-releases` plugin
# into a real `cutver` binary through the Extism host, then assert the rendered
# changelog and the resulting repository state.
#
# This is the only test that exercises the compiled wasm export
# `#[plugin_fn] pub fn invoke`. The crate's unit tests call
# `render_invocation` directly, so they keep passing even if the export is
# removed or renamed and the plugin becomes unusable. Do not fold this back
# into a unit test.
#
# Input contract: `CUTVER_BIN` must point at a `cutver` binary built with
# `--features plugins` (the WASM engine is optional and off by default). The
# test never skips: a missing plugin-capable binary is a configuration error,
# and a silently skipped integration test reports green while proving nothing.
#
#   cargo build --features plugins
#   CUTVER_BIN=/path/to/cutver bash scripts/e2e.sh

set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

WASM_TARGET="wasm32-wasip1"
PLUGIN_CRATE="github-releases"
PLUGIN_WASM="$REPO_ROOT/target/$WASM_TARGET/release/github_releases.wasm"

# ---------------------------------------------------------------------------
# Input contract
# ---------------------------------------------------------------------------
if [ -z "${CUTVER_BIN:-}" ]; then
  cat >&2 <<'EOF'
error: CUTVER_BIN is not set.

This end-to-end test needs a `cutver` binary compiled with plugin support.
Build one with the `plugins` feature (the WASM engine is optional and off by
default), then point CUTVER_BIN at it:

    git clone --depth 1 https://github.com/cutver/cutver.git
    cd cutver
    cargo build --features plugins
    CUTVER_BIN="$PWD/target/debug/cutver" bash /path/to/plugins/scripts/e2e.sh
EOF
  exit 2
fi

if [ ! -x "$CUTVER_BIN" ]; then
  printf 'error: CUTVER_BIN=%s is not an executable file.\n\n' "$CUTVER_BIN" >&2
  printf 'Build a plugin-capable cutver binary and point CUTVER_BIN at it:\n\n' >&2
  printf '    cargo build --features plugins\n\n' >&2
  exit 2
fi

# ---------------------------------------------------------------------------
# Failure accounting. One run reports every assertion instead of dying on the
# first, and each failure shows what was expected against what was found.
# ---------------------------------------------------------------------------
FAILURES=0

pass() { printf 'ok   - %s\n' "$1"; }

show_file() {
  if [ -f "$1" ]; then
    sed 's/^/    | /' "$1"
  else
    printf '    | (file not found: %s)\n' "$1"
  fi
}

fail() {
  local label="$1"
  shift
  printf 'FAIL - %s\n' "$label"
  while [ "$#" -gt 0 ]; do
    printf '       %s\n' "$1"
    shift
  done
  FAILURES=$((FAILURES + 1))
}

expect_contains() {
  local file="$1" needle="$2" label="$3"
  if [ -f "$file" ] && grep -qF -- "$needle" "$file"; then
    pass "$label"
  else
    fail "$label" "expected to contain: $needle" "file: $file" "found:"
    show_file "$file"
  fi
}

expect_absent() {
  local file="$1" needle="$2" label="$3"
  if [ -f "$file" ] && grep -qF -- "$needle" "$file"; then
    fail "$label" "expected NOT to contain: $needle" "file: $file" "found:"
    show_file "$file"
  else
    pass "$label"
  fi
}

expect_tag() {
  local repo="$1" tag="$2"
  if git -C "$repo" tag --list | grep -qxF -- "$tag"; then
    pass "git tag lists $tag"
  else
    fail "git tag lists $tag" "expected tag: $tag" "found tags:"
    git -C "$repo" tag --list | sed 's/^/    | /'
  fi
}

# ---------------------------------------------------------------------------
# Isolated temporary repositories. The trap removes every directory on exit,
# including on failure.
# ---------------------------------------------------------------------------
TMP_DIRS=()
cleanup() {
  local dir
  for dir in "${TMP_DIRS[@]:-}"; do
    [ -n "$dir" ] && rm -rf -- "$dir"
  done
}
trap cleanup EXIT

# Sets NEW_REPO_DIR and records it for the trap. Assigning a global instead of
# printing the path keeps the directory registered in this shell: a command
# substitution would create the directory in a subshell and leak it on exit.
new_repo() {
  NEW_REPO_DIR="$(mktemp -d "${TMPDIR:-/tmp}/cutver-e2e.XXXXXX")"
  TMP_DIRS+=("$NEW_REPO_DIR")
}

# Seeds a repository that mirrors a real cutver project: a cargo manifest, a
# changelog preamble, and a plugin block pinning `$hash` of `$wasm`.
seed_repo() {
  local repo="$1" hash="$2" wasm="$3"

  git -C "$repo" init -q
  # Local identity only. Never touch the machine's global Git configuration.
  git -C "$repo" config user.email "e2e@example.test"
  git -C "$repo" config user.name "E2E"

  cat > "$repo/Cargo.toml" <<'EOF'
[package]
name = "e2e-demo"
version = "1.0.0"
edition = "2021"
EOF

  cat > "$repo/CHANGELOG.md" <<'EOF'
# Changelog

All notable changes.
EOF

  cat > "$repo/cutver.toml" <<EOF
[project]
name = "e2e-demo"

[[manifest]]
path = "Cargo.toml"
kind = "cargo-package"

[changelog]
path = "CHANGELOG.md"
format = "plugin"
plugin = "github-releases"

[plugins.github-releases]
runtime = "wasm"
source = "$wasm"
hash = "sha256:$hash"
capabilities = ["changelog.v1"]
EOF

  git -C "$repo" add -A
  git -C "$repo" commit -q -m "chore: initial"
  # Annotated tag: a plain `git tag` fails where tag signing is configured.
  git -C "$repo" tag -a v1.0.0 -m "v1.0.0"

  local index message
  index=1
  for message in "feat(cli): add tree view" "fix: repair tag prefix" "feat!: drop legacy config"; do
    printf '%s\n' "$message" > "$repo/e2e-$index.txt"
    git -C "$repo" add -A
    git -C "$repo" commit -q -m "$message"
    index=$((index + 1))
  done
}

# Happy path: the pinned digest is correct, so cutver loads the real wasm,
# renders the changelog through the plugin, bumps the manifest, and tags.
run_positive() {
  local wasm="$1" hash="$2" repo output
  new_repo
  repo="$NEW_REPO_DIR"
  seed_repo "$repo" "$hash" "$wasm"

  # Capture output in a variable rather than a file inside the repo: an
  # untracked file would trip cutver's clean-tree guard before the bump runs.
  if output="$( cd "$repo" && "$CUTVER_BIN" bump minor 2>&1 )"; then
    pass "cutver bump minor succeeded"
  else
    fail "cutver bump minor succeeded" "expected exit 0" "output:"
    printf '%s\n' "$output" | sed 's/^/    | /'
  fi

  expect_contains "$repo/CHANGELOG.md" "## Breaking Changes" "changelog has Breaking Changes section"
  expect_contains "$repo/CHANGELOG.md" "* drop legacy config (" "changelog lists the breaking entry"
  expect_contains "$repo/CHANGELOG.md" "## Features" "changelog has Features section"
  expect_contains "$repo/CHANGELOG.md" "* add tree view (" "changelog lists the feature entry"
  expect_contains "$repo/CHANGELOG.md" "## Bug Fixes" "changelog has Bug Fixes section"
  expect_contains "$repo/CHANGELOG.md" "* repair tag prefix (" "changelog lists the fix entry"
  expect_contains "$repo/CHANGELOG.md" "## Contributors" "changelog has Contributors section"
  expect_contains "$repo/CHANGELOG.md" "* @E2E" "changelog credits the local author"
  expect_absent "$repo/CHANGELOG.md" "**Full Changelog**" "changelog omits Full Changelog (no remote)"
  expect_contains "$repo/CHANGELOG.md" "# Changelog" "changelog header survived"
  expect_contains "$repo/CHANGELOG.md" "All notable changes." "changelog preamble survived"

  expect_contains "$repo/Cargo.toml" 'version = "1.1.0"' "Cargo.toml bumped to 1.1.0"
  expect_tag "$repo" "v1.0.0"
  expect_tag "$repo" "v1.1.0"
}

# Negative control: a deliberately wrong hash must make cutver fail before it
# touches the changelog. Without this, a green run proves the happy path but not
# that the integrity control is wired.
run_negative() {
  local wasm="$1" repo wrong_hash before after output
  wrong_hash="$(printf '%064d' 0)"
  new_repo
  repo="$NEW_REPO_DIR"
  seed_repo "$repo" "$wrong_hash" "$wasm"

  before="$(sha256sum "$repo/CHANGELOG.md" | awk '{print $1}')"
  if output="$( cd "$repo" && "$CUTVER_BIN" bump minor 2>&1 )"; then
    fail "wrong plugin hash is rejected" "expected non-zero exit, got 0" "output:"
    printf '%s\n' "$output" | sed 's/^/    | /'
  else
    pass "wrong plugin hash is rejected (non-zero exit)"
  fi
  after="$(sha256sum "$repo/CHANGELOG.md" | awk '{print $1}')"

  if [ "$before" = "$after" ]; then
    pass "changelog untouched after failed bump"
  else
    fail "changelog untouched after failed bump" "before: $before" "after:  $after" "found:"
    show_file "$repo/CHANGELOG.md"
  fi
}

main() {
  printf 'Building %s for %s...\n' "$PLUGIN_CRATE" "$WASM_TARGET"
  if ! cargo build --release --target "$WASM_TARGET" -p "$PLUGIN_CRATE" --manifest-path "$REPO_ROOT/Cargo.toml"; then
    printf 'error: failed to build the plugin wasm artifact.\n' >&2
    exit 1
  fi

  if [ ! -f "$PLUGIN_WASM" ]; then
    printf 'error: expected wasm artifact not found at %s\n' "$PLUGIN_WASM" >&2
    exit 1
  fi

  local hash
  hash="$(sha256sum "$PLUGIN_WASM" | awk '{print $1}')"
  printf 'Plugin SHA-256: sha256:%s\n\n' "$hash"

  run_positive "$PLUGIN_WASM" "$hash"
  run_negative "$PLUGIN_WASM"

  printf '\n'
  if [ "$FAILURES" -eq 0 ]; then
    printf 'PASS: real-host end-to-end test (plugin %s via %s)\n' "$PLUGIN_CRATE" "$CUTVER_BIN"
    exit 0
  fi
  printf 'FAIL: %d assertion(s) failed (plugin %s via %s)\n' "$FAILURES" "$PLUGIN_CRATE" "$CUTVER_BIN"
  exit 1
}

main "$@"
