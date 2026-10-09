# Cutver Plugins

This repository is the official first-party plugin monorepo for
[Cutver](https://github.com/cutver/cutver), the release automation CLI. Each
plugin is a sandboxed [WebAssembly](https://webassembly.org/) module that
extends Cutver with one capability, such as rendering release notes or reading
an alternate manifest format. It is for Cutver users who need behavior the core
binary does not ship, and for contributors who publish a plugin as a versioned
`.wasm` artifact attached to a GitHub Release.

## Official plugins

| Plugin | Capability | Status |
| --- | --- | --- |
| `github-releases` | `changelog.v1` | Available |

The list grows as plugins ship. A plugin declares exactly the capabilities it
implements, so Cutver can route work without hardcoding plugin names.

## What a plugin can do

Every plugin declares one or more capability domains. The domain fixes the
operation Cutver calls and the data it passes.

| Capability | Operation | Purpose |
| --- | --- | --- |
| `changelog.v1` | `render` | Format release notes from the request Cutver supplies. |
| `manifest.v1` | `read`, `write` | Read or update a project manifest format. |
| `lifecycle.v1` | hooks | Run transactional work around bump and release. |
| `versioning.v1` | `compute` | Derive a version with a custom scheme. |

### Example: notes from `github-releases`

The plugin groups commits into fixed sections, credits contributors, and appends
the compare link. Given a request with one feature and one fix, and no breaking
changes:

```markdown
## Features

* feat(cli): add tree view (a1b2c3d) in #42

## Bug Fixes

* fix: correct tag prefix (b2c3d4e) in #43

## Contributors

* @Alice
* @Bob

## New Contributors

* @Bob

**Full Changelog**: https://github.com/cutver/cutver/compare/v1.1.0...v1.2.0
```

Sections with no entries are omitted, and the same request always renders the
same bytes.

## Install a plugin

Plugins are declared in `cutver.toml`. Each block points at a released `.wasm`
artifact and pins its SHA-256 digest, so Cutver rejects a tampered download.

```toml
[plugins.github-releases]
runtime = "wasm"
source = "https://github.com/cutver/plugins/releases/download/github-releases-v0.1.0/github-releases.wasm"
hash = "sha256:<digest>"
capabilities = ["changelog.v1"]
```

Replace `<digest>` with the value from the matching `.sha256` file attached to
the same release. Because the plugin declares `changelog.v1`, Cutver routes
release note rendering to it; no other configuration is required.

CI builds the crate to `github_releases.wasm` (Cargo uses underscores) and
uploads it as the `github-releases.wasm` asset, so the URL above is stable.

### Verify a release

The `.sha256` sidecar holds the digest in the form Cutver expects. To check a
download independently before installing it:

```sh
printf '%s  %s\n' "$(sed 's/^sha256://' github-releases.wasm.sha256)" \
  github-releases.wasm | sha256sum -c -
```

## Add a plugin

1. Create the crate under `plugins/<plugin>` and add it as a workspace member in
   the root manifest.
2. Keep the crate name equal to the plugin directory name, so CI can build it
   with `-p <plugin>`.
3. Compile for `wasm32-wasip1`. The pinned toolchain already carries the target.
   Cargo names the library target with underscores, so the `github-releases`
   crate builds `target/wasm32-wasip1/release/github_releases.wasm`.
4. Tag a release as `<plugin>-v<major>.<minor>.<patch>`, for example
   `github-releases-v0.1.0`.

A release requires no per-plugin workflow. The shared workflow reads the plugin
name from the tag, verifies that `plugins/<plugin>` exists, and fails early with
a clear message when it does not. It then installs the WebAssembly target,
builds the crate, hashes the binary, and attaches the `.wasm` and its `.sha256`
sidecar to the GitHub Release.

### Contributor checklist

- [ ] The plugin directory name matches the crate name.
- [ ] The tag follows `<plugin>-v<semver>`.
- [ ] The release page carries both the `.wasm` and the `.sha256` sidecar.
- [ ] The plugin block in your own `cutver.toml` uses the released digest.

## Repository layout

| Path | Contents |
| --- | --- |
| `.github/workflows/` | The shared release workflow. |
| `plugins/` | One crate per plugin, added as workspace members. |
| `Cargo.toml` | Workspace membership and shared package metadata. |
| `rust-toolchain.toml` | Pinned channel and WebAssembly target. |

Plugin crates depend on the published [`cutver-pdk`](https://crates.io/crates/cutver-pdk)
crate for the shared wire contract, so the DTOs are defined once for Cutver,
the plugins, and every third-party plugin.

## Why not just use GitHub's generated release notes?

GitHub's `generate-notes` endpoint requires network access and a token, so it
cannot run inside a sealed environment, and its output follows GitHub's own
formatting. A Cutver changelog plugin runs sandboxed with no network and renders
deterministically from the `ChangelogRenderRequest` that Cutver already
supplies: the release version, tag, date, commit list, and contributors. The
same input always produces the same notes, and a project controls grouping and
formatting without granting the render step external access.

## Requirements

- Rust toolchain version 1.99.0 or newer.
- The `wasm32-wasip1` compilation target.
- Cutver with plugin support enabled.

## License

MIT. See the license file for the full text.
