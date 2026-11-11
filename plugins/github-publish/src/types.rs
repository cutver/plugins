//! Data Transfer Objects and configuration structures for `github-publish`.

use serde::{Deserialize, Serialize};

/// Canonical representation of a GitHub repository (`owner/repo`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repository {
    pub owner: String,
    pub repo: String,
}

impl Repository {
    /// Parses a slug like `"owner/repo"` into a structured [`Repository`].
    pub fn parse(slug: &str) -> Result<Self, crate::error::PluginError> {
        let parts: Vec<&str> = slug.trim().split('/').collect();
        if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
            return Err(crate::error::PluginError::InvalidRepository(
                slug.to_string(),
            ));
        }
        Ok(Self {
            owner: parts[0].to_string(),
            repo: parts[1].to_string(),
        })
    }

    /// Formats as `"owner/repo"`.
    pub fn as_slug(&self) -> String {
        format!("{}/{}", self.owner, self.repo)
    }
}

/// Request body for creating a GitHub Release (`POST /repos/{owner}/{repo}/releases`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateReleaseRequest {
    /// Tag name to target or create (e.g. `"v1.2.0"`).
    pub tag_name: String,
    /// Specifies the commitish value that determines where the Git tag is created from.
    pub target_commitish: String,
    /// The name of the release.
    pub name: String,
    /// Text describing the contents of the tag.
    pub body: String,
    /// `true` to create an unpublished release.
    pub draft: bool,
    /// `true` to identify the release as a prerelease.
    pub prerelease: bool,
    /// Whether to automatically generate the name and body for this release.
    #[serde(default)]
    pub generate_release_notes: bool,
    /// If specified, also creates a discussion in the specified category.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discussion_category_name: Option<String>,
}

/// Relevant fields returned by GitHub when a release is created or fetched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseResponse {
    pub id: u64,
    pub tag_name: String,
    pub html_url: String,
    pub upload_url: String,
}

/// An asset to be uploaded to a release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetFile {
    /// Local path to the asset file.
    pub path: String,
    /// File name to display in the release (e.g. `"cutver-v0.1.0-x86_64.tar.gz"`).
    pub filename: String,
    /// File raw content bytes.
    pub bytes: Vec<u8>,
    /// Content-Type MIME string.
    pub content_type: String,
}

/// Structured configuration for `github-publish`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishConfig {
    /// GitHub repository slug (`"owner/repo"`). If omitted, read from `GITHUB_REPOSITORY`.
    #[serde(default)]
    pub repository: Option<String>,

    /// List of file paths or globs to upload as release assets.
    #[serde(default)]
    pub assets: Vec<String>,

    /// Whether to compute SHA-256 for all uploaded assets and attach a `checksums.txt`.
    /// Defaults to `true` if `assets` is non-empty.
    #[serde(default)]
    pub checksums: Option<bool>,

    /// Explicit override for draft release status. Defaults to `false`.
    #[serde(default)]
    pub draft: bool,

    /// Explicit override for prerelease. If `None`, automatically detected from SemVer.
    #[serde(default)]
    pub prerelease: Option<bool>,

    /// Fallback to GitHub's auto-generated release notes when changelog body is empty.
    #[serde(default)]
    pub generate_release_notes: bool,

    /// Path to changelog file relative to workspace root. Defaults to `"CHANGELOG.md"`.
    #[serde(default = "default_changelog_path")]
    pub changelog_path: String,

    /// Release title template (e.g. `"{tag}"` or `"Release {version}"`).
    /// Defaults to `"{tag}"`.
    #[serde(default = "default_title_template")]
    pub title_template: String,

    /// Discussion category name to create release discussion.
    #[serde(default)]
    pub discussion_category_name: Option<String>,
}

impl Default for PublishConfig {
    fn default() -> Self {
        Self {
            repository: None,
            assets: Vec::new(),
            checksums: None,
            draft: false,
            prerelease: None,
            generate_release_notes: false,
            changelog_path: default_changelog_path(),
            title_template: default_title_template(),
            discussion_category_name: None,
        }
    }
}

fn default_changelog_path() -> String {
    "CHANGELOG.md".to_string()
}

fn default_title_template() -> String {
    "{tag}".to_string()
}

/// Detects if a SemVer string represents a prerelease (e.g. `"1.0.0-rc.1"`, `"0.2.0-beta"`).
pub fn is_prerelease(version: &str) -> bool {
    let clean = version.trim().trim_start_matches('v');
    // A standard SemVer with prerelease contains a hyphen after the PATCH number.
    if let Some((_core, pre)) = clean.split_once('-') {
        !pre.is_empty()
    } else {
        false
    }
}

/// Formats the release title substituting `{tag}` and `{version}`.
pub fn format_release_title(template: &str, tag_name: &str, version: &str) -> String {
    template
        .replace("{tag}", tag_name)
        .replace("{version}", version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_repository_slug() {
        let repo = Repository::parse("cutver/plugins").unwrap();
        assert_eq!(repo.owner, "cutver");
        assert_eq!(repo.repo, "plugins");
        assert_eq!(repo.as_slug(), "cutver/plugins");
    }

    #[test]
    fn rejects_invalid_repository_slug() {
        assert!(Repository::parse("").is_err());
        assert!(Repository::parse("cutver").is_err());
        assert!(Repository::parse("cutver/plugins/extra").is_err());
        assert!(Repository::parse("cutver/").is_err());
        assert!(Repository::parse("/plugins").is_err());
    }

    #[test]
    fn prerelease_detection() {
        assert!(!is_prerelease("1.0.0"));
        assert!(!is_prerelease("v0.13.0"));
        assert!(is_prerelease("1.0.0-alpha.1"));
        assert!(is_prerelease("v0.14.0-rc.2"));
        assert!(is_prerelease("0.1.0-beta"));
    }

    #[test]
    fn format_title_substitution() {
        assert_eq!(format_release_title("{tag}", "v1.0.0", "1.0.0"), "v1.0.0");
        assert_eq!(
            format_release_title("Release {version} ({tag})", "v1.0.0", "1.0.0"),
            "Release 1.0.0 (v1.0.0)"
        );
    }
}
