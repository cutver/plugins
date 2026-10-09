//! Shared wire-contract DTOs for Cutver plugins.
//!
//! This crate is intentionally a pure data module: no I/O, no logic, and no
//! dependencies beyond `serde` and `serde_json`. It is the single source of
//! truth for the JSON envelope and the `changelog.v1` request/response shapes,
//! so a plugin never re-declares a wire field or its optionality.
//!
//! Capability and operation names travel as strings on the wire. This crate
//! deliberately does not mirror Cutver's internal enums: duplicating them would
//! create a second source of truth, and a plugin only needs to compare strings.

use serde::{Deserialize, Serialize};

/// Capability domain for rendering release notes.
pub const CAPABILITY_CHANGELOG_V1: &str = "changelog.v1";

/// Operation that renders release notes for [`CAPABILITY_CHANGELOG_V1`].
pub const OPERATION_RENDER: &str = "render";

/// The single JSON envelope a plugin receives as its input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginInvocation {
    /// Capability the host is invoking, for example `changelog.v1`.
    pub capability: String,
    /// Operation requested within the capability, for example `render`.
    pub operation: String,
    /// Operation-specific request DTO, decoded once the two names match.
    pub payload: serde_json::Value,
}

/// One commit considered when rendering release notes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginCommitEntry {
    /// Full commit hash; renderers decide how much to show.
    pub sha: String,
    /// The commit subject line.
    pub message: String,
    /// Conventional-commit type such as `feat`, when the host detected one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    /// Conventional-commit scope, when the host detected one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// Commit author name, when the host resolved one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_name: Option<String>,
    /// Pull request reference, already formatted with its `#`, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_number: Option<String>,
    /// Whether the host marked this commit as a breaking change.
    pub is_breaking: bool,
}

/// One contributor credited in rendered release notes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginContributor {
    /// Contributor handle or display name.
    pub name: String,
    /// Whether this is the contributor's first contribution to the project.
    pub is_first_contribution: bool,
}

/// Request for the `changelog.v1` / `render` operation.
///
/// Field order mirrors the wire contract exactly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChangelogRenderRequest {
    /// Project root the release was computed from.
    pub root_dir: String,
    /// Version being released, without the tag prefix.
    pub version: String,
    /// Full tag name, including any prefix.
    pub tag_name: String,
    /// Previous release tag, when one exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_tag: Option<String>,
    /// Release date in `YYYY-MM-DD` form.
    pub release_date: String,
    /// Commits between the previous tag and this release.
    pub commits: Vec<PluginCommitEntry>,
    /// Repository URL, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    /// Compare URL for the release range, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compare_url: Option<String>,
    /// Whether this release is a prerelease.
    pub is_prerelease: bool,
    /// Contributors to credit in the notes.
    pub contributors: Vec<PluginContributor>,
}

/// Response for the `changelog.v1` / `render` operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChangelogRenderResponse {
    /// Rendered release notes body.
    pub body: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_optional_fields_default_to_none() {
        let json = r#"{
            "root_dir": "/w",
            "version": "1.0.0",
            "tag_name": "v1.0.0",
            "release_date": "2026-01-01",
            "commits": [],
            "is_prerelease": false,
            "contributors": []
        }"#;
        let request: ChangelogRenderRequest = serde_json::from_str(json).expect("decodes");
        assert_eq!(request.previous_tag, None);
        assert_eq!(request.repository, None);
        assert_eq!(request.compare_url, None);
    }

    #[test]
    fn envelope_constants_and_field_order_match_the_wire_contract() {
        let invocation = PluginInvocation {
            capability: CAPABILITY_CHANGELOG_V1.to_string(),
            operation: OPERATION_RENDER.to_string(),
            payload: serde_json::json!({}),
        };
        let encoded = serde_json::to_string(&invocation).expect("encodes");
        assert_eq!(
            encoded,
            r#"{"capability":"changelog.v1","operation":"render","payload":{}}"#
        );
    }
}
