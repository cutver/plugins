//! Post-release execution engine (`on_post_release` lifecycle event).

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

    // 3. Publish exactly the entry the host already rendered from the project's
    //    changelog template. The plugin never re-derives a body from the disk nor
    //    invents a placeholder: `notes` is the single source of the release entry,
    //    and its absence is a fail-closed error.
    let (body, generate_notes) = match payload.notes.as_deref().map(str::trim) {
        Some(notes) if !notes.is_empty() => (notes.to_string(), false),
        // Asking GitHub to generate the body is an explicit user decision, not a
        // body the plugin invented on its own.
        _ if config.generate_release_notes => (String::new(), true),
        _ => return Err(PluginError::MissingReleaseNotes),
    };

    // 4. Calculate release parameters
    let title = format_release_title(&config.title_template, &payload.tag_name, &payload.version);
    let prerelease = config
        .prerelease
        .unwrap_or_else(|| is_prerelease(&payload.version));

    let target_commitish = config.target_commitish.clone();

    let release_req = CreateReleaseRequest {
        tag_name: payload.tag_name.clone(),
        target_commitish,
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

    /// The entry Cutver already rendered from the project's changelog template.
    const RENDERED_NOTES: &str = "## v0.2.0\n\n### Added\n- Support plugins";

    fn sample_payload(dry_run: bool) -> PostReleasePayload {
        PostReleasePayload {
            root_dir: ".".to_string(),
            version: "0.2.0".to_string(),
            tag_name: "v0.2.0".to_string(),
            commit_sha: "c0ffee1234567890".to_string(),
            dry_run,
            notes: Some(RENDERED_NOTES.to_string()),
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
    fn publishes_payload_notes_verbatim_and_never_reads_a_disk_changelog() {
        let payload = sample_payload(false);
        let config = PublishConfig {
            repository: Some("cutver/plugins".to_string()),
            assets: vec!["dist/app.tar.gz".to_string()],
            checksums: Some(true),
            ..Default::default()
        };
        let mock = MockGitHubClient::new();

        // The on-disk entry deliberately disagrees with the payload: if the plugin
        // reads it, the body assertion below fails.
        let file_reader = |path: &Path| -> Result<Vec<u8>, std::io::Error> {
            let path_str = path.to_string_lossy();
            if path_str.contains("dist") {
                Ok(b"tarball binary content".to_vec())
            } else {
                Err(std::io::Error::new(std::io::ErrorKind::NotFound, "missing"))
            }
        };

        handle_post_release(&payload, &config, Some("token"), &mock, file_reader).unwrap();

        let releases = mock.created_releases.lock().unwrap();
        assert_eq!(releases.len(), 1);
        let (repo_slug, req) = &releases[0];
        assert_eq!(repo_slug, "cutver/plugins");
        assert_eq!(req.name, "v0.2.0");
        assert_eq!(req.body, RENDERED_NOTES);
        assert!(!req.generate_release_notes);
        assert!(!req.prerelease);

        // Assets and checksums are unaffected by where the body came from.
        let assets = mock.uploaded_assets.lock().unwrap();
        assert_eq!(assets.len(), 2);
        assert_eq!(assets[0].1, "app.tar.gz");
        assert_eq!(assets[0].2, b"tarball binary content");
        assert_eq!(assets[1].1, "checksums.txt");
        let checksums_str = String::from_utf8(assets[1].2.clone()).unwrap();
        assert!(checksums_str.contains("app.tar.gz"));
    }

    #[test]
    fn missing_notes_fails_closed_and_creates_no_release() {
        let mut payload = sample_payload(false);
        payload.notes = None;

        let config = PublishConfig {
            repository: Some("cutver/plugins".to_string()),
            assets: vec!["dist/app.tar.gz".to_string()],
            checksums: Some(true),
            ..Default::default()
        };
        let mock = MockGitHubClient::new();

        // An on-disk changelog is available, and must still not be used as a fallback.
        let file_reader = |path: &Path| -> Result<Vec<u8>, std::io::Error> {
            let path_str = path.to_string_lossy();
            if path_str.contains("CHANGELOG.md") {
                Ok(b"## [0.2.0]\n\n### Added\n- from disk".to_vec())
            } else {
                Ok(b"tarball binary content".to_vec())
            }
        };

        let err = handle_post_release(&payload, &config, Some("token"), &mock, file_reader)
            .expect_err("absent notes must fail closed");

        assert!(matches!(err, PluginError::MissingReleaseNotes));
        let message = err.to_string();
        assert!(message.contains("[changelog] path"), "message: {message}");
        assert!(
            message.contains("generate_release_notes = true"),
            "message: {message}"
        );

        // Observable effect: nothing reached GitHub at all.
        assert_eq!(mock.created_releases.lock().unwrap().len(), 0);
        assert_eq!(mock.uploaded_assets.lock().unwrap().len(), 0);
    }

    #[test]
    fn blank_notes_are_treated_as_absent() {
        let mut payload = sample_payload(false);
        payload.notes = Some("  \n\t ".to_string());

        let config = PublishConfig {
            repository: Some("cutver/plugins".to_string()),
            ..Default::default()
        };
        let mock = MockGitHubClient::new();

        let err = handle_post_release(&payload, &config, Some("token"), &mock, |_| {
            Err(std::io::Error::new(std::io::ErrorKind::NotFound, "none"))
        })
        .expect_err("blank notes must fail closed");

        assert!(matches!(err, PluginError::MissingReleaseNotes));
        assert_eq!(mock.created_releases.lock().unwrap().len(), 0);
    }

    #[test]
    fn generate_release_notes_remains_the_explicit_opt_in_when_notes_are_absent() {
        let mut payload = sample_payload(false);
        payload.notes = None;

        let config = PublishConfig {
            repository: Some("cutver/plugins".to_string()),
            generate_release_notes: true,
            ..Default::default()
        };
        let mock = MockGitHubClient::new();

        handle_post_release(&payload, &config, Some("token"), &mock, |_| {
            Err(std::io::Error::new(std::io::ErrorKind::NotFound, "none"))
        })
        .expect("the explicit opt-in must not fail closed");

        let releases = mock.created_releases.lock().unwrap();
        assert_eq!(releases.len(), 1);
        assert_eq!(releases[0].1.body, "");
        assert!(releases[0].1.generate_release_notes);
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
