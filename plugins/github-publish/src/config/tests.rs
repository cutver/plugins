//! Unit tests for the configuration loader.
//!
//! Every test injects a fake reader: configuration loading performs no ambient
//! filesystem access, so no test touches the real working tree.

use super::*;
use std::cell::RefCell;
use std::io::{Error, ErrorKind};
use std::path::PathBuf;

const ROOT: &str = "workspace-root";

const VALID_CONFIG: &str = concat!(
    "[plugins.github-publish]\n",
    "runtime = \"wasm\"\n",
    "\n",
    "[plugins.github-publish.config]\n",
    "repository = \"cutver/plugins\"\n",
    "assets = [\"dist/app.tar.gz\"]\n",
    "draft = true\n",
);

fn config_path() -> PathBuf {
    Path::new(ROOT).join("cutver.toml")
}

fn missing_reader(_: &Path) -> Result<Vec<u8>, Error> {
    Err(Error::new(ErrorKind::NotFound, "no cutver.toml"))
}

fn reader_with(body: &'static str) -> impl Fn(&Path) -> Result<Vec<u8>, Error> {
    move |_| Ok(body.as_bytes().to_vec())
}

#[test]
fn absent_configuration_falls_back_to_defaults() {
    let config = load_config(ROOT, |_| None, missing_reader).unwrap();
    assert_eq!(config, PublishConfig::default());
}

#[test]
fn missing_config_section_falls_back_to_defaults() {
    let bodies = [
        "[workspace]\nname = \"cutver\"\n",
        "[plugins]\n",
        "[plugins.github-publish]\nruntime = \"wasm\"\n",
    ];

    for body in bodies {
        let config = load_config(ROOT, |_| None, reader_with(body)).unwrap();
        assert_eq!(
            config,
            PublishConfig::default(),
            "body `{body}` must fall back to defaults"
        );
    }
}

#[test]
fn unreadable_configuration_fails_closed() {
    let err = load_config(
        ROOT,
        |_| None,
        |_: &Path| Err(Error::new(ErrorKind::PermissionDenied, "permission denied")),
    )
    .unwrap_err();

    let message = err.to_string();
    assert!(
        matches!(err, PluginError::ConfigUnreadable { .. }),
        "expected ConfigUnreadable, got: {message}"
    );
    assert!(
        message.contains("cutver.toml"),
        "message must name the file: {message}"
    );
    assert!(
        message.contains("permission denied"),
        "message must keep the cause: {message}"
    );
}

#[test]
fn non_utf8_configuration_fails_closed() {
    let err = load_config(ROOT, |_| None, |_: &Path| Ok(vec![0xff, 0xfe, 0x00])).unwrap_err();

    let message = err.to_string();
    assert!(
        matches!(err, PluginError::ConfigUnreadable { .. }),
        "expected ConfigUnreadable, got: {message}"
    );
    assert!(
        message.contains("not valid UTF-8"),
        "message must say the file is not UTF-8: {message}"
    );
    assert!(
        message.contains("cutver.toml"),
        "message must name the file: {message}"
    );
}

#[test]
fn malformed_toml_fails_closed() {
    let body = "[plugins.github-publish.config\nassets = [\"dist\"]\n";
    let err = load_config(ROOT, |_| None, reader_with(body)).unwrap_err();

    let message = err.to_string();
    assert!(
        matches!(err, PluginError::ConfigParse { .. }),
        "expected ConfigParse, got: {message}"
    );
    assert!(
        message.contains("cutver.toml"),
        "message must name the file: {message}"
    );
}

#[test]
fn type_mismatch_fails_closed_instead_of_defaulting() {
    let body = "[plugins.github-publish.config]\nassets = \"dist/app.tar.gz\"\n";
    // GITHUB_REPOSITORY would refill `repository` if the whole block were
    // silently dropped, which is exactly the fail-open bug being closed.
    let env_lookup = |key: &str| (key == "GITHUB_REPOSITORY").then(|| "cutver/plugins".into());
    let err = load_config(ROOT, env_lookup, reader_with(body)).unwrap_err();

    let message = err.to_string();
    assert!(
        matches!(err, PluginError::ConfigInvalid { .. }),
        "expected ConfigInvalid, got: {message}"
    );
    assert!(
        message.contains("assets"),
        "message must name the key: {message}"
    );
}

#[test]
fn unknown_key_fails_closed_naming_the_key() {
    let body = "[plugins.github-publish.config]\nchecksum = true\n";
    let err = load_config(ROOT, |_| None, reader_with(body)).unwrap_err();

    let message = err.to_string();
    assert!(
        matches!(err, PluginError::ConfigInvalid { .. }),
        "expected ConfigInvalid, got: {message}"
    );
    assert!(
        message.contains("checksum"),
        "message must name the typo: {message}"
    );
    assert!(
        message.contains("cutver.toml"),
        "message must name the file: {message}"
    );
}

#[test]
fn removed_changelog_path_reports_the_migration() {
    let body = "[plugins.github-publish.config]\nchangelog_path = \"CHANGELOG.md\"\n";
    let err = load_config(ROOT, |_| None, reader_with(body)).unwrap_err();

    let message = err.to_string();
    assert!(
        matches!(err, PluginError::ConfigKeyRemoved { .. }),
        "expected ConfigKeyRemoved, got: {message}"
    );
    assert!(
        message.contains("changelog_path"),
        "message must name the removed key: {message}"
    );
    assert!(
        message.contains("[changelog] path"),
        "message must point at the migration target: {message}"
    );
    assert!(
        !message.contains("unknown field"),
        "a removed key must not surface as a generic unknown-field error: {message}"
    );
}

#[test]
fn removed_key_wins_over_an_unknown_key_and_a_bad_type() {
    let body = concat!(
        "[plugins.github-publish.config]\n",
        "changelog_path = \"CHANGELOG.md\"\n",
        "checksum = true\n",
        "assets = \"dist/app.tar.gz\"\n",
    );
    let err = load_config(ROOT, |_| None, reader_with(body)).unwrap_err();

    let message = err.to_string();
    assert!(
        matches!(err, PluginError::ConfigKeyRemoved { .. }),
        "the migration message must win over the generic errors, got: {message}"
    );
    assert!(
        message.contains("[changelog] path"),
        "message must point at the migration target: {message}"
    );
}

#[test]
fn config_table_must_be_a_table() {
    let body = "[plugins.github-publish]\nconfig = 5\n";
    let err = load_config(ROOT, |_| None, reader_with(body)).unwrap_err();

    let message = err.to_string();
    assert!(
        matches!(err, PluginError::ConfigShape { .. }),
        "expected ConfigShape, got: {message}"
    );
    assert!(
        message.contains("plugins.github-publish.config"),
        "message must name the key: {message}"
    );
}

#[test]
fn plugins_scalar_fails_closed() {
    let body = "plugins = 3\n";
    let err = load_config(ROOT, |_| None, reader_with(body)).unwrap_err();

    let message = err.to_string();
    assert!(
        matches!(err, PluginError::ConfigShape { .. }),
        "expected ConfigShape, got: {message}"
    );
    assert!(
        message.contains("`plugins`"),
        "message must name the key: {message}"
    );
}

#[test]
fn plugin_entry_scalar_fails_closed() {
    let body = "[plugins]\ngithub-publish = 3\n";
    let err = load_config(ROOT, |_| None, reader_with(body)).unwrap_err();

    let message = err.to_string();
    assert!(
        matches!(err, PluginError::ConfigShape { .. }),
        "expected ConfigShape, got: {message}"
    );
    assert!(
        message.contains("plugins.github-publish"),
        "message must name the key: {message}"
    );
}

#[test]
fn reads_only_the_payload_root_and_never_the_process_cwd() {
    // Records every requested path, including on the error path: a fallback to a
    // process-cwd `cutver.toml` only ever shows up as a second request.
    for kind in [ErrorKind::NotFound, ErrorKind::PermissionDenied] {
        let requested = RefCell::new(Vec::new());
        let read_file = |path: &Path| {
            requested.borrow_mut().push(path.to_path_buf());
            Err(Error::new(kind, "unreadable"))
        };

        let result = load_config(ROOT, |_| None, read_file);

        assert_eq!(
            requested.into_inner(),
            vec![config_path()],
            "a {kind:?} read must request exactly the payload root"
        );

        // NotFound is "no configuration"; every other read error is fatal.
        match kind {
            ErrorKind::NotFound => assert_eq!(result.unwrap(), PublishConfig::default()),
            _ => assert!(
                matches!(result, Err(PluginError::ConfigUnreadable { .. })),
                "a fatal read must fail closed"
            ),
        }
    }
}

#[test]
fn loads_a_valid_configuration() {
    let config = load_config(ROOT, |_| None, reader_with(VALID_CONFIG)).unwrap();

    assert_eq!(config.repository.as_deref(), Some("cutver/plugins"));
    assert_eq!(config.assets, vec!["dist/app.tar.gz"]);
    assert!(config.draft);
    assert_eq!(config.title_template, "{tag}");
}

#[test]
fn applies_env_overrides() {
    let envs = [
        ("GITHUB_REPOSITORY", "cutver/plugins"),
        ("GITHUB_PUBLISH_ASSETS", "dist/app.tar.gz, dist/app.zip"),
        ("GITHUB_PUBLISH_DRAFT", "true"),
        ("GITHUB_PUBLISH_CHECKSUMS", "true"),
    ];

    let config = load_config(
        ROOT,
        |key| {
            envs.iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v.to_string())
        },
        missing_reader,
    )
    .unwrap();

    assert_eq!(config.repository.as_deref(), Some("cutver/plugins"));
    assert_eq!(config.assets, vec!["dist/app.tar.gz", "dist/app.zip"]);
    assert!(config.draft);
    assert_eq!(config.checksums, Some(true));
}
