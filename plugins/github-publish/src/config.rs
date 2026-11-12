//! Configuration loader for `github-publish`.

use crate::error::PluginError;
use crate::types::PublishConfig;
use std::path::Path;

/// Plugin key inside `cutver.toml`'s `[plugins]` table.
const PLUGIN_KEY: &str = "github-publish";

/// Key holding the plugin configuration block.
const CONFIG_KEY: &str = "config";

/// Dotted path of the plugin's configuration block.
const CONFIG_PREFIX: &str = "plugins.github-publish";

/// Configuration keys retired from the plugin surface, each paired with the
/// migration remedy. Table-driven so a new removal is one line and every retired
/// key is visible in a single place.
const REMOVED_KEYS: &[(&str, &str)] = &[(
    "changelog_path",
    "release notes now come from the entry Cutver renders for `[changelog] path` in cutver.toml and sends as `PostReleasePayload.notes`",
)];

/// Loads effective configuration from `<root_dir>/cutver.toml` through the
/// injected `read_file`, then applies environment overrides.
///
/// A missing file means "no configuration" and falls back to defaults. Every
/// other outcome — unreadable, non-UTF-8, malformed TOML, wrong shape, unknown
/// key, invalid value — is an error, so a typo can never be published over.
pub fn load_config<F, R>(
    root_dir: &str,
    env_lookup: F,
    read_file: R,
) -> Result<PublishConfig, PluginError>
where
    F: Fn(&str) -> Option<String>,
    R: Fn(&Path) -> Result<Vec<u8>, std::io::Error>,
{
    let path = Path::new(root_dir).join("cutver.toml");
    let mut config = match read_optional_table(&path, &read_file)? {
        Some(table) => config_from_table(&path, table)?,
        None => PublishConfig::default(),
    };
    apply_env_overrides(&mut config, &env_lookup);
    Ok(config)
}

/// Reads `path` and returns the `[plugins.github-publish.config]` table, or
/// `None` when the file or that section is absent.
fn read_optional_table<R>(path: &Path, read_file: &R) -> Result<Option<toml::Table>, PluginError>
where
    R: Fn(&Path) -> Result<Vec<u8>, std::io::Error>,
{
    let Some(content) = read_optional_text(path, read_file)? else {
        return Ok(None);
    };
    let document: toml::Table = content.parse().map_err(|source| PluginError::ConfigParse {
        path: path.display().to_string(),
        source,
    })?;

    let Some(plugins) = table_entry(&document, "", "plugins", path)? else {
        return Ok(None);
    };
    let Some(plugin) = table_entry(plugins, "plugins", PLUGIN_KEY, path)? else {
        return Ok(None);
    };
    let Some(config) = table_entry(plugin, CONFIG_PREFIX, CONFIG_KEY, path)? else {
        return Ok(None);
    };

    Ok(Some(config.clone()))
}

/// Resolves `key` inside `parent`: `Ok(None)` when the key is absent, an error when
/// it is present but not a table. An absent key means "no configuration"; a
/// present key with the wrong shape is a defect that must not be published over.
fn table_entry<'a>(
    parent: &'a toml::Table,
    prefix: &str,
    key: &str,
    path: &Path,
) -> Result<Option<&'a toml::Table>, PluginError> {
    let Some(value) = parent.get(key) else {
        return Ok(None);
    };
    let Some(table) = value.as_table() else {
        return Err(PluginError::ConfigShape {
            path: path.display().to_string(),
            key: join_key(prefix, key),
        });
    };
    Ok(Some(table))
}

/// Joins a dotted key prefix with `key`, skipping the prefix when it is empty.
fn join_key(prefix: &str, key: &str) -> String {
    if prefix.is_empty() {
        key.to_string()
    } else {
        format!("{prefix}.{key}")
    }
}

/// Reads `path` as UTF-8 text, treating only a missing file as "no configuration".
fn read_optional_text<R>(path: &Path, read_file: &R) -> Result<Option<String>, PluginError>
where
    R: Fn(&Path) -> Result<Vec<u8>, std::io::Error>,
{
    let bytes = match read_file(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(PluginError::ConfigUnreadable {
                path: path.display().to_string(),
                reason: error.to_string(),
            });
        }
    };

    String::from_utf8(bytes)
        .map(Some)
        .map_err(|error| PluginError::ConfigUnreadable {
            path: path.display().to_string(),
            reason: format!("the file is not valid UTF-8 ({error})"),
        })
}

/// Deserializes the plugin configuration table into [`PublishConfig`].
fn config_from_table(path: &Path, table: toml::Table) -> Result<PublishConfig, PluginError> {
    reject_removed_keys(path, &table)?;
    table
        .try_into::<PublishConfig>()
        .map_err(|source| PluginError::ConfigInvalid {
            path: path.display().to_string(),
            source,
        })
}

/// Rejects configuration keys retired in an earlier plugin version.
fn reject_removed_keys(path: &Path, table: &toml::Table) -> Result<(), PluginError> {
    let Some((key, remedy)) = REMOVED_KEYS
        .iter()
        .find(|(key, _)| table.contains_key(*key))
    else {
        return Ok(());
    };

    Err(PluginError::ConfigKeyRemoved {
        path: path.display().to_string(),
        key: (*key).to_string(),
        remedy: (*remedy).to_string(),
    })
}

/// Applies environment overrides on top of the file configuration.
fn apply_env_overrides<F>(config: &mut PublishConfig, env_lookup: &F)
where
    F: Fn(&str) -> Option<String>,
{
    if let Some(repo) = env_lookup("GITHUB_REPOSITORY")
        && !repo.trim().is_empty()
    {
        config.repository = Some(repo.trim().to_string());
    }

    if let Some(assets_env) = env_lookup("GITHUB_PUBLISH_ASSETS") {
        let items: Vec<String> = assets_env
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if !items.is_empty() {
            config.assets = items;
        }
    }

    if let Some(draft_env) = env_lookup("GITHUB_PUBLISH_DRAFT") {
        config.draft = draft_env.trim().eq_ignore_ascii_case("true") || draft_env.trim() == "1";
    }

    if let Some(chk_env) = env_lookup("GITHUB_PUBLISH_CHECKSUMS") {
        config.checksums =
            Some(chk_env.trim().eq_ignore_ascii_case("true") || chk_env.trim() == "1");
    }

    if let Some(title_env) = env_lookup("GITHUB_PUBLISH_TITLE")
        && !title_env.trim().is_empty()
    {
        config.title_template = title_env.trim().to_string();
    }
}

#[cfg(test)]
mod tests;
