//! Changelog parser that extracts release notes for a target version.

/// Extracts the release notes markdown section for `target_version` from `changelog_content`.
///
/// Supports Keep a Changelog conventions:
/// - `## [1.2.0] - 2026-10-10`
/// - `## [v1.2.0]`
/// - `## 1.2.0 - 2026-10-10`
/// - `## v1.2.0`
///
/// Stops extraction at the next level-2 heading (`## `) or at reference-style links (`[1.2.0]: https:...`).
pub fn extract_release_notes(changelog_content: &str, target_version: &str) -> Option<String> {
    let clean_version = target_version.trim().trim_start_matches('v');
    let mut in_section = false;
    let mut collected_lines = Vec::new();

    for line in changelog_content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("## ") {
            if in_section {
                // Encountered the next release section; stop extracting.
                break;
            }

            // Check if this header matches the target version
            if header_matches_version(trimmed, clean_version) {
                in_section = true;
                continue;
            }
        } else if in_section {
            // Stop if we hit reference link definitions at bottom of changelog: [0.1.0]: https://...
            if trimmed.starts_with('[') && trimmed.contains("]: http") {
                break;
            }
            collected_lines.push(line);
        }
    }

    if !in_section {
        return None;
    }

    let joined = collected_lines.join("\n");
    let trimmed_body = joined.trim();
    if trimmed_body.is_empty() {
        None
    } else {
        Some(trimmed_body.to_string())
    }
}

/// Checks whether a level-2 header line like `## [0.1.0] - 2026-10-10` corresponds to `clean_version`.
fn header_matches_version(header_line: &str, clean_version: &str) -> bool {
    // Strip `## `
    let text = header_line.trim_start_matches('#').trim();

    // Look for bracketed version: [0.1.0] or [v0.1.0]
    if let Some(start) = text.find('[')
        && let Some(end) = text[start..].find(']')
    {
        let version_candidate = text[start + 1..start + end].trim().trim_start_matches('v');
        if version_candidate == clean_version {
            return true;
        }
    }

    // Look for unbracketed word at the beginning of text: `0.1.0 - 2026-10-10` or `v0.1.0`
    let first_word = text
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_start_matches('v');
    first_word == clean_version
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_CHANGELOG: &str = r#"# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added
- WIP feature

## [0.2.0] - 2026-11-01

### Added
- Added GitHub publish plugin (#42)
- Added SHA-256 checksum generation

### Fixed
- Fixed token lookup

## [0.1.0] - 2026-10-15

### Added
- Initial release with basic CLI.

[Unreleased]: https://github.com/cutver/plugins/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/cutver/plugins/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/cutver/plugins/releases/tag/v0.1.0
"#;

    #[test]
    fn extracts_bracketed_version() {
        let notes = extract_release_notes(SAMPLE_CHANGELOG, "0.2.0").unwrap();
        assert!(notes.starts_with("### Added"));
        assert!(notes.contains("Added GitHub publish plugin (#42)"));
        assert!(notes.contains("Fixed token lookup"));
        // Does not leak into 0.1.0
        assert!(!notes.contains("Initial release with basic CLI"));
        // Does not include link definitions
        assert!(!notes.contains("[0.2.0]: https://"));
    }

    #[test]
    fn extracts_bracketed_version_with_v_prefix() {
        let notes = extract_release_notes(SAMPLE_CHANGELOG, "v0.1.0").unwrap();
        assert_eq!(notes, "### Added\n- Initial release with basic CLI.");
    }

    #[test]
    fn extracts_unbracketed_header() {
        let changelog = r#"# Changelog

## 1.0.0 - 2026-12-01

Major release notes here.

## 0.9.0

Older notes.
"#;
        let notes = extract_release_notes(changelog, "1.0.0").unwrap();
        assert_eq!(notes, "Major release notes here.");
    }

    #[test]
    fn returns_none_when_version_not_found() {
        assert!(extract_release_notes(SAMPLE_CHANGELOG, "9.9.9").is_none());
    }

    #[test]
    fn returns_none_when_section_is_empty() {
        let changelog = "## [1.0.0]\n\n## [0.9.0]\nSome notes";
        assert!(extract_release_notes(changelog, "1.0.0").is_none());
    }
}
