//! Pre-bump validation handler (`on_pre_bump` lifecycle event).

use crate::client::GitHubClient;
use crate::error::PluginError;
use crate::types::{PublishConfig, Repository};
use cutver_pdk::{PreBumpPayload, PreBumpResponse};

/// Validates GitHub credentials, repository access, and tag uniqueness before any file mutations.
pub fn handle_pre_bump<C: GitHubClient + ?Sized>(
    payload: &PreBumpPayload,
    config: &PublishConfig,
    token: Option<&str>,
    client: &C,
) -> Result<PreBumpResponse, PluginError> {
    // 1. Verify token presence
    let token = match token {
        Some(t) if !t.trim().is_empty() => t.trim(),
        _ => {
            return Ok(PreBumpResponse {
                allow: false,
                reason: Some(
                    "Missing GitHub token: ensure GITHUB_TOKEN or GH_TOKEN is declared in [plugins.github-publish.permissions.env] and set in the environment".to_string(),
                ),
            });
        }
    };
    let _ = token;

    // 2. Resolve repository slug
    let repo_slug = match &config.repository {
        Some(slug) if !slug.trim().is_empty() => slug.trim(),
        _ => {
            return Ok(PreBumpResponse {
                allow: false,
                reason: Some(
                    "Missing GitHub repository: configure `repository = \"owner/repo\"` in cutver.toml or set GITHUB_REPOSITORY in the environment".to_string(),
                ),
            });
        }
    };

    let repo = match Repository::parse(repo_slug) {
        Ok(r) => r,
        Err(e) => {
            return Ok(PreBumpResponse {
                allow: false,
                reason: Some(format!("Invalid repository slug `{repo_slug}`: {e}")),
            });
        }
    };

    // 3. Preflight repository access check
    if let Err(err) = client.check_repository_access(&repo) {
        return Ok(PreBumpResponse {
            allow: false,
            reason: Some(format!(
                "Repository access check failed for `{}`: {err}",
                repo.as_slug()
            )),
        });
    }

    // 4. Preflight tag collision check
    match client.tag_exists(&repo, &payload.tag_name) {
        Ok(true) => {
            return Ok(PreBumpResponse {
                allow: false,
                reason: Some(format!(
                    "Release tag `{}` already exists on remote repository `{}`; bump aborted to prevent collision",
                    payload.tag_name,
                    repo.as_slug()
                )),
            });
        }
        Ok(false) => {}
        Err(err) => {
            return Ok(PreBumpResponse {
                allow: false,
                reason: Some(format!(
                    "Failed to verify remote tag `{}` in `{}`: {err}",
                    payload.tag_name,
                    repo.as_slug()
                )),
            });
        }
    }

    // All preflight checks passed cleanly
    Ok(PreBumpResponse {
        allow: true,
        reason: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::MockGitHubClient;

    fn sample_payload(tag_name: &str, dry_run: bool) -> PreBumpPayload {
        PreBumpPayload {
            root_dir: ".".to_string(),
            current_version: "0.1.0".to_string(),
            next_version: "0.2.0".to_string(),
            bump_level: "minor".to_string(),
            tag_name: tag_name.to_string(),
            dry_run,
        }
    }

    fn sample_config(repo: Option<&str>) -> PublishConfig {
        PublishConfig {
            repository: repo.map(String::from),
            ..Default::default()
        }
    }

    #[test]
    fn rejects_when_token_is_missing() {
        let payload = sample_payload("v0.2.0", false);
        let config = sample_config(Some("cutver/plugins"));
        let mock = MockGitHubClient::new();

        let res = handle_pre_bump(&payload, &config, None, &mock).unwrap();
        assert!(!res.allow);
        assert!(res.reason.unwrap().contains("Missing GitHub token"));
    }

    #[test]
    fn rejects_when_token_is_empty_string() {
        let payload = sample_payload("v0.2.0", false);
        let config = sample_config(Some("cutver/plugins"));
        let mock = MockGitHubClient::new();

        let res = handle_pre_bump(&payload, &config, Some("   "), &mock).unwrap();
        assert!(!res.allow);
        assert!(res.reason.unwrap().contains("Missing GitHub token"));
    }

    #[test]
    fn rejects_when_repository_is_missing() {
        let payload = sample_payload("v0.2.0", false);
        let config = sample_config(None);
        let mock = MockGitHubClient::new();

        let res = handle_pre_bump(&payload, &config, Some("token_123"), &mock).unwrap();
        assert!(!res.allow);
        assert!(res.reason.unwrap().contains("Missing GitHub repository"));
    }

    #[test]
    fn rejects_when_repository_format_is_invalid() {
        let payload = sample_payload("v0.2.0", false);
        let config = sample_config(Some("invalid-repo-without-slash"));
        let mock = MockGitHubClient::new();

        let res = handle_pre_bump(&payload, &config, Some("token_123"), &mock).unwrap();
        assert!(!res.allow);
        assert!(res.reason.unwrap().contains("Invalid repository slug"));
    }

    #[test]
    fn rejects_when_repository_access_fails() {
        let payload = sample_payload("v0.2.0", false);
        let config = sample_config(Some("cutver/plugins"));
        let mut mock = MockGitHubClient::new();
        mock.allow_access = false;

        let res = handle_pre_bump(&payload, &config, Some("token_123"), &mock).unwrap();
        assert!(!res.allow);
        assert!(
            res.reason
                .unwrap()
                .contains("Repository access check failed")
        );
    }

    #[test]
    fn rejects_when_tag_already_exists() {
        let payload = sample_payload("v0.2.0", false);
        let config = sample_config(Some("cutver/plugins"));
        let mock = MockGitHubClient::new().with_tag("v0.2.0");

        let res = handle_pre_bump(&payload, &config, Some("token_123"), &mock).unwrap();
        assert!(!res.allow);
        assert!(
            res.reason
                .unwrap()
                .contains("already exists on remote repository")
        );
    }

    #[test]
    fn allows_bump_when_all_checks_pass() {
        let payload = sample_payload("v0.2.0", false);
        let config = sample_config(Some("cutver/plugins"));
        let mock = MockGitHubClient::new().with_tag("v0.1.0");

        let res = handle_pre_bump(&payload, &config, Some("token_123"), &mock).unwrap();
        assert!(res.allow);
        assert_eq!(res.reason, None);
    }
}
