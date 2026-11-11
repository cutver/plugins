//! GitHub API client trait and Extism WASM HTTP adapter.

use crate::error::PluginError;
use crate::types::{AssetFile, CreateReleaseRequest, ReleaseResponse, Repository};

/// Trait defining the operations performed against the GitHub REST API.
pub trait GitHubClient {
    /// Validates that the token can access the repository with write permissions.
    fn check_repository_access(&self, repo: &Repository) -> Result<(), PluginError>;

    /// Checks if a Git tag already exists on the remote repository.
    fn tag_exists(&self, repo: &Repository, tag: &str) -> Result<bool, PluginError>;

    /// Creates a new release in the repository.
    fn create_release(
        &self,
        repo: &Repository,
        req: &CreateReleaseRequest,
    ) -> Result<ReleaseResponse, PluginError>;

    /// Uploads an asset file to a release's asset upload URL.
    fn upload_asset(&self, upload_url_template: &str, asset: &AssetFile)
    -> Result<(), PluginError>;
}

/// Strips URI template suffixes like `{?name,label}` from a GitHub upload URL.
pub fn clean_upload_url(template: &str) -> &str {
    if let Some(idx) = template.find('{') {
        &template[..idx]
    } else {
        template
    }
}

/// Default implementation of [`GitHubClient`] using Extism HTTP host calls in WebAssembly.
///
/// Guarded with `#[cfg(target_arch = "wasm32")]` because Extism host functions
/// (`http_request`, `alloc`, `length`, etc.) are runtime imports that only exist
/// inside a WebAssembly host environment.
#[cfg(target_arch = "wasm32")]
#[derive(Debug, Clone)]
pub struct ExtismGitHubClient {
    token: String,
    api_base: String,
}

#[cfg(target_arch = "wasm32")]
impl ExtismGitHubClient {
    /// Creates a new client with the given Bearer token and optional custom API base URL.
    pub fn new(token: String, custom_api_base: Option<String>) -> Self {
        let api_base = custom_api_base
            .unwrap_or_else(|| "https://api.github.com".to_string())
            .trim_end_matches('/')
            .to_string();
        Self { token, api_base }
    }
}

#[cfg(target_arch = "wasm32")]
impl GitHubClient for ExtismGitHubClient {
    fn check_repository_access(&self, repo: &Repository) -> Result<(), PluginError> {
        let url = format!("{}/repos/{}", self.api_base, repo.as_slug());
        let req = extism_pdk::HttpRequest::new(&url)
            .with_method("GET")
            .with_header("User-Agent", "cutver-plugin-github-publish")
            .with_header("Accept", "application/vnd.github+json")
            .with_header("Authorization", format!("Bearer {}", self.token));

        let res = extism_pdk::http::request::<&str>(&req, None).map_err(|e| PluginError::Http {
            url: url.clone(),
            reason: e.to_string(),
        })?;

        let status = res.status_code();
        if status == 200 {
            Ok(())
        } else {
            let msg = String::from_utf8_lossy(&res.body()).to_string();
            Err(PluginError::GitHubApi {
                status,
                endpoint: url,
                message: msg,
            })
        }
    }

    fn tag_exists(&self, repo: &Repository, tag: &str) -> Result<bool, PluginError> {
        let clean_tag = tag.trim_start_matches("refs/tags/");
        let url = format!(
            "{}/repos/{}/git/ref/tags/{}",
            self.api_base,
            repo.as_slug(),
            clean_tag
        );
        let req = extism_pdk::HttpRequest::new(&url)
            .with_method("GET")
            .with_header("User-Agent", "cutver-plugin-github-publish")
            .with_header("Accept", "application/vnd.github+json")
            .with_header("Authorization", format!("Bearer {}", self.token));

        let res = extism_pdk::http::request::<&str>(&req, None).map_err(|e| PluginError::Http {
            url: url.clone(),
            reason: e.to_string(),
        })?;

        match res.status_code() {
            200 => Ok(true),
            404 => Ok(false),
            status => {
                let msg = String::from_utf8_lossy(&res.body()).to_string();
                Err(PluginError::GitHubApi {
                    status,
                    endpoint: url,
                    message: msg,
                })
            }
        }
    }

    fn create_release(
        &self,
        repo: &Repository,
        release_req: &CreateReleaseRequest,
    ) -> Result<ReleaseResponse, PluginError> {
        let url = format!("{}/repos/{}/releases", self.api_base, repo.as_slug());
        let body_json = serde_json::to_string(release_req).map_err(PluginError::Response)?;

        let req = extism_pdk::HttpRequest::new(&url)
            .with_method("POST")
            .with_header("User-Agent", "cutver-plugin-github-publish")
            .with_header("Accept", "application/vnd.github+json")
            .with_header("Content-Type", "application/json")
            .with_header("Authorization", format!("Bearer {}", self.token));

        let res = extism_pdk::http::request::<&str>(&req, Some(&body_json)).map_err(|e| {
            PluginError::Http {
                url: url.clone(),
                reason: e.to_string(),
            }
        })?;

        let status = res.status_code();
        if status == 201 || status == 200 {
            let body_bytes = res.body();
            serde_json::from_slice(&body_bytes).map_err(|e| PluginError::GitHubApi {
                status,
                endpoint: url,
                message: format!("Failed to parse release response JSON: {e}"),
            })
        } else {
            let msg = String::from_utf8_lossy(&res.body()).to_string();
            Err(PluginError::GitHubApi {
                status,
                endpoint: url,
                message: msg,
            })
        }
    }

    fn upload_asset(
        &self,
        upload_url_template: &str,
        asset: &AssetFile,
    ) -> Result<(), PluginError> {
        let base_url = clean_upload_url(upload_url_template);
        let url = format!("{}?name={}", base_url, asset.filename);

        let req = extism_pdk::HttpRequest::new(&url)
            .with_method("POST")
            .with_header("User-Agent", "cutver-plugin-github-publish")
            .with_header("Accept", "application/vnd.github+json")
            .with_header("Content-Type", &asset.content_type)
            .with_header("Authorization", format!("Bearer {}", self.token));

        let res = extism_pdk::http::request::<&[u8]>(&req, Some(&asset.bytes)).map_err(|e| {
            PluginError::Http {
                url: url.clone(),
                reason: e.to_string(),
            }
        })?;

        let status = res.status_code();
        if status == 201 || status == 200 {
            Ok(())
        } else {
            let msg = String::from_utf8_lossy(&res.body()).to_string();
            Err(PluginError::GitHubApi {
                status,
                endpoint: url,
                message: msg,
            })
        }
    }
}

/// In-memory mock implementation of [`GitHubClient`] for unit and integration testing.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Default)]
pub struct MockGitHubClient {
    pub allow_access: bool,
    pub existing_tags: std::collections::HashSet<String>,
    pub created_releases: std::sync::Mutex<Vec<(String, CreateReleaseRequest)>>,
    pub uploaded_assets: std::sync::Mutex<Vec<(String, String, Vec<u8>)>>,
    pub next_release_id: std::sync::atomic::AtomicU64,
}

#[cfg(not(target_arch = "wasm32"))]
impl MockGitHubClient {
    pub fn new() -> Self {
        Self {
            allow_access: true,
            existing_tags: std::collections::HashSet::new(),
            created_releases: std::sync::Mutex::new(Vec::new()),
            uploaded_assets: std::sync::Mutex::new(Vec::new()),
            next_release_id: std::sync::atomic::AtomicU64::new(1),
        }
    }

    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.existing_tags.insert(tag.into());
        self
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl GitHubClient for MockGitHubClient {
    fn check_repository_access(&self, repo: &Repository) -> Result<(), PluginError> {
        if self.allow_access {
            Ok(())
        } else {
            Err(PluginError::GitHubApi {
                status: 403,
                endpoint: format!("https://api.github.com/repos/{}", repo.as_slug()),
                message: "Access forbidden in mock".to_string(),
            })
        }
    }

    fn tag_exists(&self, _repo: &Repository, tag: &str) -> Result<bool, PluginError> {
        Ok(self.existing_tags.contains(tag))
    }

    fn create_release(
        &self,
        repo: &Repository,
        req: &CreateReleaseRequest,
    ) -> Result<ReleaseResponse, PluginError> {
        let id = self
            .next_release_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.created_releases
            .lock()
            .unwrap()
            .push((repo.as_slug(), req.clone()));

        Ok(ReleaseResponse {
            id,
            tag_name: req.tag_name.clone(),
            html_url: format!(
                "https://github.com/{}/releases/tag/{}",
                repo.as_slug(),
                req.tag_name
            ),
            upload_url: format!(
                "https://uploads.github.com/repos/{}/releases/{}/assets{{?name,label}}",
                repo.as_slug(),
                id
            ),
        })
    }

    fn upload_asset(
        &self,
        upload_url_template: &str,
        asset: &AssetFile,
    ) -> Result<(), PluginError> {
        self.uploaded_assets.lock().unwrap().push((
            upload_url_template.to_string(),
            asset.filename.clone(),
            asset.bytes.clone(),
        ));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_github_upload_url_template() {
        let template =
            "https://uploads.github.com/repos/cutver/plugins/releases/1/assets{?name,label}";
        assert_eq!(
            clean_upload_url(template),
            "https://uploads.github.com/repos/cutver/plugins/releases/1/assets"
        );

        let plain = "https://uploads.github.com/repos/cutver/plugins/releases/1/assets";
        assert_eq!(clean_upload_url(plain), plain);
    }

    #[test]
    fn mock_client_records_releases_and_assets() {
        let mock = MockGitHubClient::new().with_tag("v0.1.0");
        let repo = Repository::parse("cutver/plugins").unwrap();

        assert!(mock.check_repository_access(&repo).is_ok());
        assert!(mock.tag_exists(&repo, "v0.1.0").unwrap());
        assert!(!mock.tag_exists(&repo, "v0.2.0").unwrap());

        let req = CreateReleaseRequest {
            tag_name: "v0.2.0".to_string(),
            target_commitish: Some("abcdef1".to_string()),
            name: "v0.2.0".to_string(),
            body: "Notes".to_string(),
            draft: false,
            prerelease: false,
            generate_release_notes: false,
            discussion_category_name: None,
        };

        let res = mock.create_release(&repo, &req).unwrap();
        assert_eq!(res.id, 1);
        assert_eq!(res.tag_name, "v0.2.0");

        let asset = AssetFile {
            path: "dist/app.tar.gz".to_string(),
            filename: "app.tar.gz".to_string(),
            bytes: b"archive bytes".to_vec(),
            content_type: "application/gzip".to_string(),
        };

        mock.upload_asset(&res.upload_url, &asset).unwrap();
        assert_eq!(mock.uploaded_assets.lock().unwrap().len(), 1);
    }
}
