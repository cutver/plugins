# github-publish (Cutver Plugin)

Official Cutver plugin that automates GitHub release creation from the notes Cutver already rendered, plus release asset publishing.
Built on the sandboxed `wasm32-wasip1` runtime via Extism and `cutver-pdk`.

## Features

- **Preflight Verification (`on_pre_bump`)**:
  - Validates `GITHUB_TOKEN` presence and repository write access.
  - Checks if the release tag (`v{version}`) already exists on GitHub to prevent duplicate tags or failed releases after local mutations.
  - Refuses to run when the plugin configuration is unreadable, malformed or carries an unknown key (`src/config.rs`), so nothing is mutated on the strength of a typo.
- **Release Notes from the Host (`on_post_release`)**:
  - Publishes the `notes` entry Cutver rendered from the project's changelog template, verbatim and heading included. The plugin never re-reads the working tree, so what the changelog says is exactly what the release says.
  - Fails closed with an explicit error when the host supplies no entry, instead of inventing a placeholder body. Cutver only renders an entry when `cutver.toml` declares `[changelog] path`.
  - `generate_release_notes = true` asks GitHub to generate the body from pull requests instead, for projects that keep no changelog. Requires `cutver-pdk` 0.1.1 or newer.
- **SemVer Prerelease Detection**:
  - Automatically identifies prerelease tags (e.g. `1.0.0-rc.1`, `0.2.0-beta`) and sets `prerelease = true`.
- **Release Asset Uploads with Checksums**:
  - Uploads compiled binaries, packages, and tarballs to GitHub Release assets.
  - Automatically calculates SHA-256 for all uploaded assets and generates/uploads a GNU/BSD-compatible `checksums.txt` sidecar.
- **Dry-Run Support**:
  - Full simulation support with `cutver bump --dry-run`: preflight validation runs without mutating any remote resources.

---

## Configuration

Add the plugin to `cutver.toml`:

```toml
[plugins.github-publish]
runtime = "wasm"
source = "https://github.com/cutver/plugins/releases/download/github-publish-v0.2.1/github-publish.wasm"
hash = "sha256:<sha256_digest>"
capabilities = ["lifecycle.v1"]
events = ["on_pre_bump", "on_post_release"]

[plugins.github-publish.permissions]
network = ["api.github.com", "uploads.github.com"]
env = ["GITHUB_TOKEN"]
# The workspace `cutver.toml` must be readable: that is where this block lives and
# where the plugin reads it. Cutver denies an ungranted path at runtime instead of
# warning, so a grant limited to "dist" breaks every release.
filesystem = ["cutver.toml", "dist"]

[plugins.github-publish.config]
# GitHub repository slug. If omitted, read from GITHUB_REPOSITORY environment variable.
repository = "owner/repo"

# Asset files or patterns to upload
assets = ["dist/*.tar.gz", "dist/*.zip"]

# Automatically compute SHA-256 and attach `checksums.txt` (default: true when assets are configured)
checksums = true

# Create release as draft for manual review before publishing (default: false)
draft = false

# Override prerelease flag (default: auto-detected from version string)
# prerelease = false

# Let GitHub generate the release body from pull requests when Cutver supplies no
# rendered entry (default: false)
generate_release_notes = false

# Release title template (default: "{tag}")
title_template = "Release {version}"

# Optional GitHub Discussion category to link with this release
# discussion_category_name = "Announcements"
```

### Configuration errors abort the release

Configuration is loaded at the start of `on_pre_bump` and again at the start of
`on_post_release`, and any problem aborts the invocation before a single release or asset
is created. `GITHUB_REPOSITORY` cannot paper over the failure: environment overrides are
applied on top of a configuration that already loaded successfully.

Reading that file is a capability, not a courtesy: the plugin reads its own block from
`<root_dir>/cutver.toml`, so `filesystem` in `[plugins.github-publish.permissions]` has to
include `cutver.toml`. Cutver denies an ungranted path at runtime instead of warning, so a
grant limited to `dist` breaks every release.

| Situation | Result |
| --- | --- |
| `cutver.toml` does not exist | defaults, then environment overrides |
| no `[plugins]`, `[plugins.github-publish]` or `config` key | defaults, then environment overrides |
| `plugins`, `plugins.github-publish` or `config` is present but not a table | error naming that key |
| the file cannot be read, or is not valid UTF-8 | error naming the file |
| the file is not valid TOML | error naming the file and the syntax position |
| a key has the wrong type, or a key is not recognised | error naming the key and the file |
| `changelog_path` is present | error pointing at `[changelog] path` in `cutver.toml` |

Unknown keys are rejected rather than ignored, so `checksum = true` (instead of
`checksums = true`) stops the release and names the offending key.

### Removed keys

`github-publish` 0.2.0 removed the `changelog_path` configuration key and the
`GITHUB_PUBLISH_CHANGELOG` environment variable. The plugin no longer reads a changelog
from the working tree: Cutver renders the entry for `[changelog] path` declared in
`cutver.toml` and passes it as `PostReleasePayload.notes`, which the plugin publishes
verbatim. A leftover `changelog_path` key now fails with a dedicated migration error
naming `[changelog] path`, not with a generic unknown-field message.

---

## Environment Variable Overrides

The plugin can also read configuration from standard CI environment variables declared in `permissions.env`:

| Environment Variable | Description |
| --- | --- |
| `GITHUB_TOKEN` or `GH_TOKEN` | GitHub personal access token or `GITHUB_TOKEN` secret. |
| `GITHUB_REPOSITORY` | Repository slug (`owner/repo`). |
| `GITHUB_PUBLISH_ASSETS` | Comma-separated list of asset paths. |
| `GITHUB_PUBLISH_CHECKSUMS` | Set to `"true"` or `"false"`. |
| `GITHUB_PUBLISH_DRAFT` | Set to `"true"` or `"false"`. |
| `GITHUB_PUBLISH_TITLE` | Release title template. |

---

## Building from Source

```bash
# Build wasm module
cargo build --release --target wasm32-wasip1 -p github-publish

# Run test suite
cargo test -p github-publish
```
