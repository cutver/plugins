# Cutver Plugins

This repository is the official first-party plugin monorepo for
[Cutver](https://github.com/cutver/cutver), the release automation CLI. Each
plugin is a sandboxed [WebAssembly](https://webassembly.org/) module that
extends Cutver with one capability and ships as a versioned `.wasm` artifact on
a GitHub Release.

## Official plugins

| Plugin | Capability | Status |
| --- | --- | --- |
| `github-releases` | `changelog.v1` | Available, v0.1.0 |

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
the compare link. Given a request with one feature and one fix:

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

Empty sections are omitted; the same request always renders the same bytes.

## Install a plugin

Plugins are declared in `cutver.toml`. Each block points at a released `.wasm`
artifact and pins its SHA-256 digest, so Cutver rejects a tampered download.

```toml
[plugins.github-releases]
runtime = "wasm"
source = "https://github.com/cutver/plugins/releases/download/github-releases-v0.1.0/github-releases.wasm"
hash = "sha256:a828cbabd247afdaf78fec358ae5a27271097c7c2ef6fc9ec0fe04af22edf841"
capabilities = ["changelog.v1"]
```

This plugin needs a Cutver newer than `v0.11.0`: the invocation contract changed.

### Verify a release

Check a download against its `.sha256` sidecar before installing it:

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
| `.github/workflows/` | The shared CI, release and end-to-end workflows. |
| `plugins/` | One crate per plugin, added as workspace members. |
| `Cargo.toml` | Workspace membership and shared package metadata. |
| `rust-toolchain.toml` | Pinned channel and WebAssembly target. |

Plugin crates depend on the published [`cutver-pdk`](https://crates.io/crates/cutver-pdk)
crate for the shared wire contract, so the DTOs are defined once for Cutver,
the plugins, and every third-party plugin.

## Why not just use GitHub's generated release notes?

GitHub's `generate-notes` endpoint needs network access and a token, so it
cannot run inside a sealed environment. A Cutver changelog plugin runs
sandboxed with no network and renders deterministically from the
`ChangelogRenderRequest` Cutver supplies, so a project controls formatting
without granting the render step external access.

## Test against a real `cutver` binary

`scripts/e2e.sh` builds the plugin, loads it into a `cutver` binary through
Extism, and asserts the rendered changelog and bumped repository. It needs a
plugin-capable binary, because the WASM engine is an optional feature:

```sh
cargo build --features plugins   # in a cutver checkout
CUTVER_BIN=/path/to/cutver bash scripts/e2e.sh
```

An unset or non-executable `CUTVER_BIN` fails loudly; the test never skips.
`.github/workflows/e2e.yml` runs it against cutver's current `main`.

## Requirements

- Rust 1.99.0 and `wasm32-wasip1`, both pinned in the toolchain file.
- Cutver with plugin support enabled.

## License

MIT. See the license file for the full text.
