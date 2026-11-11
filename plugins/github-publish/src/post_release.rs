//! Post-release execution engine (`on_post_release` lifecycle event).

use crate::changelog::extract_release_notes;
use crate::checksum::{generate_checksums_file, sha256_digest};
use crate::client::GitHubClient;
use crate::error::PluginError;
use crate::types::{
    AssetFile, CreateReleaseRequest, PublishConfig, ReleaseResponse, Repository,
    format_release_title, is_prerelease,
};
use cutver_pdk::PostReleasePayload;
use std::path::{Path, PathBuf};

/// Resolves the MIME content-type string for a given filename.
pub fn guess_content_type(filename: &str) -> &'static str {
    let lower = filename.to_lowercase();
    if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
        "application/gzip"
    } else if lower.ends_with(".zip") {
        "application/zip"
    } else if lower.ends_with(".wasm") {
        "application/wasm"
    } else if lower.ends_with(".json") {
        "application/json"
    } else if lower.ends_with(".txt")
        || lower.ends_with(".md")
        || lower.ends_with(".sha256")
        || lower.ends_with(".sha512")
    {
        "text/plain; charset=utf-8"
    } else {
        "application/octet-stream"
    }
}

/// Executes post-release GitHub publishing: creates release, attaches notes, and uploads assets.
pub fn handle_post_release<C: GitHubClient + ?Sized, R>(
    payload: &PostReleasePayload,
    config: &PublishConfig,
    token: Option<&str>,
    client: &C,
    read_file: R,
) -> Result<Option<ReleaseResponse>, PluginError>
where
    R: Fn(&Path) -> Result<Vec<u8>, std::io::Error>,
{
    // 1. Verify credentials and target repository
    let _token = match token {
        Some(t) if !t.trim().is_empty() => t.trim(),
        _ => return Err(PluginError::MissingToken),
    };

    let repo_slug = match &config.repository {
        Some(slug) if !slug.trim().is_empty() => slug.trim(),
        _ => return Err(PluginError::MissingRepository),
    };

    let repo = Repository::parse(repo_slug)?;

    // 2. Short-circuit on dry run
    if payload.dry_run {
        return Ok(None);
    }

    // 3. Extract or generate release notes
    let changelog_full_path = Path::new(&payload.root_dir).join(&config.changelog_path);
    let extracted_notes = if let Ok(bytes) =
        read_file(&changelog_full_path).or_else(|_| read_file(Path::new(&config.changelog_path)))
    {
        if let Ok(content) = String::from_utf8(bytes) {
            extract_release_notes(&content, &payload.version)
        } else {
            None
        }
    } else {
        None
    };

    let (body, generate_notes) = match extracted_notes {
        Some(notes) => (notes, false),
        None if config.generate_release_notes => (String::new(), true),
        None => (
            format!("Release {} ({})", payload.version, payload.tag_name),
            false,
        ),
    };

    // 4. Calculate release parameters
    let title = format_release_title(&config.title_template, &payload.tag_name, &payload.version);
    let prerelease = config
        .prerelease
        .unwrap_or_else(|| is_prerelease(&payload.version));

    let release_req = CreateReleaseRequest {
        tag_name: payload.tag_name.clone(),
        target_commitish: payload.commit_sha.clone(),
        name: title,
        body,
        draft: config.draft,
        prerelease,
        generate_release_notes: generate_notes,
        discussion_category_name: config.discussion_category_name.clone(),
    };

    // 5. Create release on GitHub
    let release_res = client.create_release(&repo, &release_req)?;

    // 6. Collect and upload assets
    let mut checksum_entries: Vec<(String, String)> = Vec::new();

    for asset_pattern in &config.assets {
        let asset_path = resolve_asset_path(&payload.root_dir, asset_pattern);
        let bytes = read_file(&asset_path).map_err(|err| PluginError::AssetRead {
            path: asset_path.display().to_string(),
            reason: err.to_string(),
        })?;

        let filename = asset_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(asset_pattern)
            .to_string();

        let digest = sha256_digest(&bytes);
        checksum_entries.push((filename.clone(), digest));

        let asset_file = AssetFile {
            path: asset_path.display().to_string(),
            filename,
            content_type: guess_content_type(asset_pattern).to_string(),
            bytes,
        };

        client.upload_asset(&release_res.upload_url, &asset_file)?;
    }

    // 7. Generate and upload checksums.txt if requested
    let should_upload_checksums = config.checksums.unwrap_or(!checksum_entries.is_empty());

    if should_upload_checksums && !checksum_entries.is_empty() {
        let entries_ref: Vec<(&str, &str)> = checksum_entries
            .iter()
            .map(|(f, h)| (f.as_str(), h.as_str()))
            .collect();
        let checksums_content = generate_checksums_file(&entries_ref);

        let checksum_asset = AssetFile {
            path: "checksums.txt".to_string(),
            filename: "checksums.txt".to_string(),
            content_type: "text/plain; charset=utf-8".to_string(),
            bytes: checksums_content.into_bytes(),
        };

        client.upload_asset(&release_res.upload_url, &checksum_asset)?;
    }

    Ok(Some(release_res))
}

fn resolve_asset_path(root_dir: &str, pattern: &str) -> PathBuf {
    let direct = PathBuf::from(pattern);
    if direct.is_file() || direct.is_absolute() {
        direct
    } else {
        Path::new(root_dir).join(pattern)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::MockGitHubClient;

    fn sample_payload(dry_run: bool) -> PostReleasePayload {
        PostReleasePayload {
            root_dir: ".".to_string(),
            version: "0.2.0".to_string(),
            tag_name: "v0.2.0".to_string(),
            commit_sha: "c0ffee1234567890".to_string(),
            dry_run,
        }
    }

    #[test]
    fn dry_run_does_not_call_create_release() {
        let payload = sample_payload(true);
        let config = PublishConfig {
            repository: Some("cutver/plugins".to_string()),
            ..Default::default()
        };
        let mock = MockGitHubClient::new();

        let result = handle_post_release(&payload, &config, Some("token"), &mock, |_| {
            Err(std::io::Error::new(std::io::ErrorKind::NotFound, "none"))
        })
        .unwrap();

        assert_eq!(result, None);
        assert_eq!(mock.created_releases.lock().unwrap().len(), 0);
    }

    #[test]
    fn creates_release_with_extracted_changelog_and_assets() {
        let payload = sample_payload(false);
        let config = PublishConfig {
            repository: Some("cutver/plugins".to_string()),
            assets: vec!["dist/app.tar.gz".to_string()],
            checksums: Some(true),
            ..Default::default()
        };
        let mock = MockGitHubClient::new();

        let changelog = "## [0.2.0] - 2026-11-01\n\n### Added\n- Support plugins";
        let file_reader = move |path: &Path| -> Result<Vec<u8>, std::io::Error> {
            let path_str = path.to_string_lossy();
            if path_str.contains("CHANGELOG.md") {
                Ok(changelog.as_bytes().to_vec())
            } else if path_str.contains("dist") {
                Ok(b"tarball binary content".to_vec())
            } else {
                Err(std::io::Error::new(std::io::ErrorKind::NotFound, "missing"))
            }
        };

        let result =
            handle_post_release(&payload, &config, Some("token"), &mock, file_reader).unwrap();

        assert!(result.is_some());
        let release = result.unwrap();
        assert_eq!(release.tag_name, "v0.2.0");

        // Verify created release request
        let releases = mock.created_releases.lock().unwrap();
        assert_eq!(releases.len(), 1);
        let (repo_slug, req) = &releases[0];
        assert_eq!(repo_slug, "cutver/plugins");
        assert_eq!(req.name, "v0.2.0");
        assert_eq!(req.target_commitish, "c0ffee1234567890");
        assert_eq!(req.body, "### Added\n- Support plugins");
        assert!(!req.prerelease);

        // Verify uploaded assets (app.tar.gz and checksums.txt)
        let assets = mock.uploaded_assets.lock().unwrap();
        assert_eq!(assets.len(), 2);
        assert_eq!(assets[0].1, "app.tar.gz");
        assert_eq!(assets[0].2, b"tarball binary content");
        assert_eq!(assets[1].1, "checksums.txt");
        let checksums_str = String::from_utf8(assets[1].2.clone()).unwrap();
        assert!(checksums_str.contains("app.tar.gz"));
    }

    #[test]
    fn automatically_marks_prerelease() {
        let mut payload = sample_payload(false);
        payload.version = "1.0.0-rc.1".to_string();
        payload.tag_name = "v1.0.0-rc.1".to_string();

        let config = PublishConfig {
            repository: Some("cutver/plugins".to_string()),
            ..Default::default()
        };
        let mock = MockGitHubClient::new();

        handle_post_release(&payload, &config, Some("token"), &mock, |_| {
            Err(std::io::Error::new(std::io::ErrorKind::NotFound, "none"))
        })
        .unwrap();

        let releases = mock.created_releases.lock().unwrap();
        assert!(releases[0].1.prerelease);
    }
}
