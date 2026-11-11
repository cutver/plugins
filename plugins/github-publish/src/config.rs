//! Configuration loader for `github-publish`.

use crate::error::PluginError;
use crate::types::PublishConfig;
use std::path::Path;

/// Loads effective configuration by reading `cutver.toml` from `root_dir` if accessible,
/// and applying environment variable overrides.
pub fn load_config<F>(root_dir: &str, env_lookup: F) -> Result<PublishConfig, PluginError>
where
    F: Fn(&str) -> Option<String>,
{
    let mut config = PublishConfig::default();

    // 1. Try reading cutver.toml in root_dir if available, or fallback to current dir
    let cutver_toml_path = Path::new(root_dir).join("cutver.toml");
    let content_opt = std::fs::read_to_string(&cutver_toml_path)
        .or_else(|_| std::fs::read_to_string("cutver.toml"));
    if let Ok(content) = content_opt
        && let Ok(toml_val) = content.parse::<toml::Table>()
        && let Some(plugins_table) = toml_val.get("plugins").and_then(|v| v.as_table())
        && let Some(plugin_table) = plugins_table
            .get("github-publish")
            .and_then(|v| v.as_table())
        && let Some(cfg_table) = plugin_table.get("config").and_then(|v| v.as_table())
    {
        let cfg_val = toml::Value::Table(cfg_table.clone());
        if let Ok(parsed) = cfg_val.try_into::<PublishConfig>() {
            config = parsed;
        }
    }

    // 2. Apply environment variable overrides (Zero Ambient Authority compliant through env_lookup)
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

    if let Some(changelog_env) = env_lookup("GITHUB_PUBLISH_CHANGELOG")
        && !changelog_env.trim().is_empty()
    {
        config.changelog_path = changelog_env.trim().to_string();
    }

    if let Some(title_env) = env_lookup("GITHUB_PUBLISH_TITLE")
        && !title_env.trim().is_empty()
    {
        config.title_template = title_env.trim().to_string();
    }

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_default_when_no_file_or_env() {
        let config = load_config(".", |_| None).unwrap();
        assert_eq!(config.repository, None);
        assert!(config.assets.is_empty());
        assert!(!config.draft);
        assert_eq!(config.changelog_path, "CHANGELOG.md");
    }

    #[test]
    fn applies_env_overrides() {
        let envs = [
            ("GITHUB_REPOSITORY", "cutver/plugins"),
            ("GITHUB_PUBLISH_ASSETS", "dist/app.tar.gz, dist/app.zip"),
            ("GITHUB_PUBLISH_DRAFT", "true"),
            ("GITHUB_PUBLISH_CHECKSUMS", "true"),
        ];

        let config = load_config(".", |key| {
            envs.iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v.to_string())
        })
        .unwrap();

        assert_eq!(config.repository.as_deref(), Some("cutver/plugins"));
        assert_eq!(config.assets, vec!["dist/app.tar.gz", "dist/app.zip"]);
        assert!(config.draft);
        assert_eq!(config.checksums, Some(true));
    }
}
