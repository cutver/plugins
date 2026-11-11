//! Cutver plugin that publishes releases and assets to GitHub.
//!
//! Implements `lifecycle.v1` for `on_pre_bump` (fail-safe preflight verification)
//! and `on_post_release` (release creation and asset upload engine).

pub mod changelog;
pub mod checksum;
pub mod client;
pub mod config;
pub mod error;
pub mod post_release;
pub mod pre_bump;
pub mod types;

use cutver_pdk::{
    Capability, PluginInvocation, PluginOperation, PostReleasePayload, PreBumpPayload,
};
use error::PluginError;
use std::path::Path;

#[cfg(target_arch = "wasm32")]
use extism_pdk::{FnResult, plugin_fn};

/// Core invocation dispatcher decoupled from runtime host memory and sandbox.
pub fn invoke_with<C, E, R>(
    input: &str,
    token: Option<&str>,
    client: &C,
    env_lookup: E,
    read_file: R,
) -> Result<String, PluginError>
where
    C: client::GitHubClient + ?Sized,
    E: Fn(&str) -> Option<String>,
    R: Fn(&Path) -> Result<Vec<u8>, std::io::Error>,
{
    let invocation: PluginInvocation =
        serde_json::from_str(input).map_err(PluginError::Envelope)?;

    if invocation.capability != Capability::LifecycleV1 {
        return Err(PluginError::UnsupportedCapability {
            capability: invocation.capability.as_str().to_string(),
        });
    }

    match invocation.operation {
        PluginOperation::OnPreBump => {
            let payload: PreBumpPayload =
                serde_json::from_value(invocation.payload).map_err(|source| {
                    PluginError::Payload {
                        operation: "on_pre_bump".to_string(),
                        source,
                    }
                })?;

            let cfg = config::load_config(&payload.root_dir, &env_lookup)?;
            let response = pre_bump::handle_pre_bump(&payload, &cfg, token, client)?;
            serde_json::to_string(&response).map_err(PluginError::Response)
        }
        PluginOperation::OnPostRelease => {
            let payload: PostReleasePayload =
                serde_json::from_value(invocation.payload).map_err(|source| {
                    PluginError::Payload {
                        operation: "on_post_release".to_string(),
                        source,
                    }
                })?;

            let cfg = config::load_config(&payload.root_dir, &env_lookup)?;
            handle_post_release_and_format(&payload, &cfg, token, client, read_file)
        }
        other => Err(PluginError::UnsupportedOperation {
            operation: other.as_str().to_string(),
        }),
    }
}

fn handle_post_release_and_format<C, R>(
    payload: &PostReleasePayload,
    cfg: &types::PublishConfig,
    token: Option<&str>,
    client: &C,
    read_file: R,
) -> Result<String, PluginError>
where
    C: client::GitHubClient + ?Sized,
    R: Fn(&Path) -> Result<Vec<u8>, std::io::Error>,
{
    let result = post_release::handle_post_release(payload, cfg, token, client, read_file)?;
    if let Some(release) = result {
        serde_json::to_string(&release).map_err(PluginError::Response)
    } else {
        Ok("{}".to_string())
    }
}

/// The plugin's single exported Extism entry point for `wasm32-wasip1`.
#[cfg(target_arch = "wasm32")]
#[plugin_fn]
pub fn invoke(input: String) -> FnResult<String> {
    let token = extism_pdk::config::get("GITHUB_TOKEN")
        .ok()
        .flatten()
        .or_else(|| extism_pdk::config::get("GH_TOKEN").ok().flatten());

    let api_url = extism_pdk::config::get("GITHUB_API_URL").ok().flatten();
    let client = client::ExtismGitHubClient::new(token.clone().unwrap_or_default(), api_url);

    let env_lookup = |key: &str| -> Option<String> { extism_pdk::config::get(key).ok().flatten() };

    invoke_with(&input, token.as_deref(), &client, env_lookup, |p| {
        std::fs::read(p)
    })
    .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use client::MockGitHubClient;

    #[test]
    fn rejects_unsupported_capability() {
        let envelope = r#"{
            "capability": "manifest.v1",
            "operation": "read",
            "payload": {}
        }"#;

        let mock = MockGitHubClient::new();
        let err = invoke_with(
            envelope,
            Some("token"),
            &mock,
            |_| None,
            |_| Err(std::io::Error::new(std::io::ErrorKind::NotFound, "err")),
        )
        .unwrap_err();

        assert!(matches!(err, PluginError::UnsupportedCapability { .. }));
    }

    #[test]
    fn rejects_unsupported_operation() {
        let envelope = r#"{
            "capability": "lifecycle.v1",
            "operation": "read",
            "payload": {}
        }"#;

        let mock = MockGitHubClient::new();
        let err = invoke_with(
            envelope,
            Some("token"),
            &mock,
            |_| None,
            |_| Err(std::io::Error::new(std::io::ErrorKind::NotFound, "err")),
        )
        .unwrap_err();

        assert!(matches!(err, PluginError::UnsupportedOperation { .. }));
    }

    #[test]
    fn dispatches_pre_bump_successfully() {
        let envelope = r#"{
            "capability": "lifecycle.v1",
            "operation": "on_pre_bump",
            "payload": {
                "root_dir": ".",
                "current_version": "0.1.0",
                "next_version": "0.2.0",
                "bump_level": "minor",
                "tag_name": "v0.2.0",
                "dry_run": false
            }
        }"#;

        let mock = MockGitHubClient::new();
        let envs = [("GITHUB_REPOSITORY", "cutver/plugins")];
        let env_lookup = |key: &str| {
            envs.iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v.to_string())
        };

        let response_json = invoke_with(envelope, Some("token_123"), &mock, env_lookup, |_| {
            Err(std::io::Error::new(std::io::ErrorKind::NotFound, "err"))
        })
        .unwrap();

        let resp: cutver_pdk::PreBumpResponse = serde_json::from_str(&response_json).unwrap();
        assert!(resp.allow);
        assert_eq!(resp.reason, None);
    }

    #[test]
    fn dispatches_post_release_successfully() {
        let envelope = r#"{
            "capability": "lifecycle.v1",
            "operation": "on_post_release",
            "payload": {
                "root_dir": ".",
                "version": "0.2.0",
                "tag_name": "v0.2.0",
                "commit_sha": "abcdef1234567890",
                "dry_run": false
            }
        }"#;

        let mock = MockGitHubClient::new();
        let envs = [("GITHUB_REPOSITORY", "cutver/plugins")];
        let env_lookup = |key: &str| {
            envs.iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v.to_string())
        };

        let response_json = invoke_with(envelope, Some("token_123"), &mock, env_lookup, |_| {
            Err(std::io::Error::new(std::io::ErrorKind::NotFound, "err"))
        })
        .unwrap();

        let resp: types::ReleaseResponse = serde_json::from_str(&response_json).unwrap();
        assert_eq!(resp.tag_name, "v0.2.0");
        assert_eq!(mock.created_releases.lock().unwrap().len(), 1);
    }
}
