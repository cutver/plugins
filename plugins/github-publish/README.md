# github-publish (Cutver Plugin)

Official Cutver plugin that automates GitHub release creation from the notes Cutver already rendered, plus release asset publishing.
Built on the sandboxed `wasm32-wasip1` runtime via Extism and `cutver-pdk`.

## Features

- **Preflight Verification (`on_pre_bump`)**:
  - Validates `GITHUB_TOKEN` presence and repository write access.
  - Checks if the release tag (`v{version}`) already exists on GitHub to prevent duplicate tags or failed releases after local mutations.
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
source = "https://github.com/cutver/plugins/releases/download/github-publish-v0.2.0/github-publish.wasm"
hash = "sha256:<sha256_digest>"
capabilities = ["lifecycle.v1"]
events = ["on_pre_bump", "on_post_release"]

[plugins.github-publish.permissions]
network = ["api.github.com", "uploads.github.com"]
env = ["GITHUB_TOKEN"]
filesystem = ["dist"]

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
