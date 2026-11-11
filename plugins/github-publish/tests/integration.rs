use cutver_pdk::PreBumpResponse;
use github_publish::client::MockGitHubClient;
use github_publish::invoke_with;
use github_publish::types::ReleaseResponse;
use std::collections::HashMap;
use std::path::Path;

#[test]
fn end_to_end_pre_bump_and_post_release_lifecycle() {
    let mock = MockGitHubClient::new().with_tag("v0.1.0");
    let mut envs = HashMap::new();
    envs.insert("GITHUB_REPOSITORY".to_string(), "cutver/cutver".to_string());
    envs.insert(
        "GITHUB_PUBLISH_ASSETS".to_string(),
        "dist/cutver-x86_64.tar.gz,dist/cutver-aarch64.tar.gz".to_string(),
    );
    envs.insert("GITHUB_PUBLISH_CHECKSUMS".to_string(), "true".to_string());

    let env_lookup = |key: &str| envs.get(key).cloned();

    // 1. Phase 1: Pre-bump preflight check
    let pre_bump_envelope = r#"{
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

    let pre_res_str = invoke_with(
        pre_bump_envelope,
        Some("gho_test_token_123"),
        &mock,
        env_lookup,
        |_| Err(std::io::Error::new(std::io::ErrorKind::NotFound, "err")),
    )
    .expect("pre-bump invocation should succeed");

    let pre_res: PreBumpResponse = serde_json::from_str(&pre_res_str).unwrap();
    assert!(pre_res.allow, "preflight check must allow bump");
    assert_eq!(pre_res.reason, None);

    // 2. Phase 2: Post-release execution
    let changelog_fixture = r#"# Changelog

## [0.2.0] - 2026-11-11

### Features
- Added official github-publish plugin (#50)
- Sandboxed asset upload with automatic checksums

### Bug Fixes
- Fixed preflight verification on Windows MSVC

## [0.1.0] - 2026-10-01
- Initial version
"#;

    let fs_mock = move |p: &Path| -> Result<Vec<u8>, std::io::Error> {
        let path_str = p.to_string_lossy();
        if path_str.contains("CHANGELOG.md") {
            Ok(changelog_fixture.as_bytes().to_vec())
        } else if path_str.contains("x86_64") {
            Ok(b"x86_64 binary bytes".to_vec())
        } else if path_str.contains("aarch64") {
            Ok(b"aarch64 binary bytes".to_vec())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "not found",
            ))
        }
    };

    let post_release_envelope = r#"{
        "capability": "lifecycle.v1",
        "operation": "on_post_release",
        "payload": {
            "root_dir": ".",
            "version": "0.2.0",
            "tag_name": "v0.2.0",
            "commit_sha": "f00ba41234567890",
            "dry_run": false
        }
    }"#;

    let post_res_str = invoke_with(
        post_release_envelope,
        Some("gho_test_token_123"),
        &mock,
        env_lookup,
        fs_mock,
    )
    .expect("post-release invocation should succeed");

    let post_res: ReleaseResponse = serde_json::from_str(&post_res_str).unwrap();
    assert_eq!(post_res.tag_name, "v0.2.0");
    assert_eq!(
        post_res.html_url,
        "https://github.com/cutver/cutver/releases/tag/v0.2.0"
    );

    // Verify release created on mock
    let created = mock.created_releases.lock().unwrap();
    assert_eq!(created.len(), 1);
    let (repo, req) = &created[0];
    assert_eq!(repo, "cutver/cutver");
    assert_eq!(req.tag_name, "v0.2.0");
    assert_eq!(req.name, "v0.2.0");
    assert!(
        req.body
            .contains("Added official github-publish plugin (#50)")
    );
    assert!(
        req.body
            .contains("Fixed preflight verification on Windows MSVC")
    );
    assert!(!req.draft);
    assert!(!req.prerelease);

    // Verify assets uploaded: 2 binaries + 1 checksums.txt
    let uploaded = mock.uploaded_assets.lock().unwrap();
    assert_eq!(uploaded.len(), 3);
    assert_eq!(uploaded[0].1, "cutver-x86_64.tar.gz");
    assert_eq!(uploaded[1].1, "cutver-aarch64.tar.gz");
    assert_eq!(uploaded[2].1, "checksums.txt");

    // Checksums file verification
    let checksums_body = String::from_utf8(uploaded[2].2.clone()).unwrap();
    assert!(checksums_body.contains("cutver-x86_64.tar.gz"));
    assert!(checksums_body.contains("cutver-aarch64.tar.gz"));
}

#[test]
fn pre_bump_collision_stops_release() {
    let mock = MockGitHubClient::new().with_tag("v0.2.0");
    let mut envs = HashMap::new();
    envs.insert("GITHUB_REPOSITORY".to_string(), "cutver/cutver".to_string());

    let pre_bump_envelope = r#"{
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

    let res_str = invoke_with(
        pre_bump_envelope,
        Some("gho_test_token_123"),
        &mock,
        |k| envs.get(k).cloned(),
        |_| Err(std::io::Error::new(std::io::ErrorKind::NotFound, "err")),
    )
    .unwrap();

    let res: PreBumpResponse = serde_json::from_str(&res_str).unwrap();
    assert!(!res.allow);
    assert!(res.reason.unwrap().contains("already exists on remote"));
}

#[test]
fn dry_run_skips_mutating_api_calls() {
    let mock = MockGitHubClient::new();
    let mut envs = HashMap::new();
    envs.insert("GITHUB_REPOSITORY".to_string(), "cutver/cutver".to_string());

    let post_release_envelope = r#"{
        "capability": "lifecycle.v1",
        "operation": "on_post_release",
        "payload": {
            "root_dir": ".",
            "version": "0.2.0",
            "tag_name": "v0.2.0",
            "commit_sha": "f00ba41234567890",
            "dry_run": true
        }
    }"#;

    let res_str = invoke_with(
        post_release_envelope,
        Some("gho_test_token_123"),
        &mock,
        |k| envs.get(k).cloned(),
        |_| Err(std::io::Error::new(std::io::ErrorKind::NotFound, "err")),
    )
    .unwrap();

    assert_eq!(res_str, "{}");
    assert_eq!(mock.created_releases.lock().unwrap().len(), 0);
    assert_eq!(mock.uploaded_assets.lock().unwrap().len(), 0);
}
