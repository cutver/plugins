//! Cutver plugin that renders deterministic changelog notes for the
//! `changelog.v1` / `render` capability.
//!
//! Rendering is pure: no network, filesystem, or environment access, and the
//! same request always produces the same bytes.

use cutver_pdk::{
    Capability, ChangelogRenderRequest, ChangelogRenderResponse, PluginCommitEntry,
    PluginContributor, PluginInvocation, PluginOperation,
};
// The PDK symbols (`alloc`, `input_length`, `output_set`, ...) are imports that only
// the Extism runtime provides, so the exported entry point exists on WebAssembly and
// nowhere else. On a native target the link would fail: MSVC requires every symbol to
// be resolved, while ELF tolerates unresolved ones in a shared library, which is why
// building this crate natively only breaks on Windows.
//
// `render_invocation` stays target-independent so the behaviour can be tested natively,
// and the cross-repository end-to-end test is what proves the compiled wasm actually
// exports `invoke`.
#[cfg(target_arch = "wasm32")]
use extism_pdk::{FnResult, plugin_fn};
use thiserror::Error;

/// Number of hex characters shown for a commit hash.
const SHORT_SHA_LEN: usize = 7;

/// Errors surfaced while handling an invocation.
///
/// Every variant is a domain error with an exact cause; the `#[plugin_fn]`
/// boundary converts them into the plugin's error return.
#[derive(Debug, Error)]
pub enum PluginError {
    /// The input was not a valid [`PluginInvocation`] envelope.
    #[error("invalid invocation envelope: {0}")]
    Envelope(#[source] serde_json::Error),
    /// The envelope named a capability this plugin does not implement.
    #[error("unsupported capability `{capability}`")]
    UnsupportedCapability {
        /// Capability received on the wire.
        capability: String,
    },
    /// The envelope named an operation this plugin does not implement.
    #[error("unsupported operation `{operation}`")]
    UnsupportedOperation {
        /// Operation received on the wire.
        operation: String,
    },
    /// The payload did not decode into a [`ChangelogRenderRequest`].
    #[error("invalid changelog payload: {0}")]
    Payload(#[source] serde_json::Error),
    /// The response could not be encoded.
    #[error("failed to encode changelog response: {0}")]
    Response(#[source] serde_json::Error),
}

/// Fixed render sections, in the exact order they appear in the body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Breaking,
    Features,
    BugFixes,
    Performance,
    Refactoring,
    Documentation,
    Other,
}

impl Section {
    /// Sections in render order. `Other` is the fallback and always last.
    const ORDERED: [Self; 7] = [
        Self::Breaking,
        Self::Features,
        Self::BugFixes,
        Self::Performance,
        Self::Refactoring,
        Self::Documentation,
        Self::Other,
    ];

    /// Section heading text.
    const fn heading(self) -> &'static str {
        match self {
            Self::Breaking => "Breaking Changes",
            Self::Features => "Features",
            Self::BugFixes => "Bug Fixes",
            Self::Performance => "Performance",
            Self::Refactoring => "Refactoring",
            Self::Documentation => "Documentation",
            Self::Other => "Other Changes",
        }
    }

    /// Whether this section owns `commit`. Every commit matches at most one
    /// section because a breaking change is claimed by `Breaking` first.
    fn selects(self, commit: &PluginCommitEntry) -> bool {
        match self {
            Self::Breaking => commit.is_breaking,
            Self::Features => !commit.is_breaking && has_type(commit, "feat"),
            Self::BugFixes => !commit.is_breaking && has_type(commit, "fix"),
            Self::Performance => !commit.is_breaking && has_type(commit, "perf"),
            Self::Refactoring => !commit.is_breaking && has_type(commit, "refactor"),
            Self::Documentation => !commit.is_breaking && has_type(commit, "docs"),
            Self::Other => !commit.is_breaking && !is_known_type(commit.r#type.as_deref()),
        }
    }
}

/// Whether a commit carries the given conventional-commit type.
fn has_type(commit: &PluginCommitEntry, expected: &str) -> bool {
    commit.r#type.as_deref() == Some(expected)
}

/// Whether a type is routed to a dedicated (non-`Other`) section.
fn is_known_type(kind: Option<&str>) -> bool {
    matches!(kind, Some("feat" | "fix" | "perf" | "refactor" | "docs"))
}

/// First seven characters of `sha`, never splitting a UTF-8 boundary.
fn short_sha(sha: &str) -> &str {
    match sha.char_indices().nth(SHORT_SHA_LEN) {
        Some((index, _)) => &sha[..index],
        None => sha,
    }
}

/// One bullet: `* <message> (<short sha>)` plus ` in <pr>` when a PR exists.
fn render_entry(commit: &PluginCommitEntry) -> String {
    let mut entry = format!("* {} ({})", commit.message, short_sha(&commit.sha));
    if let Some(pr) = &commit.pr_number {
        entry.push_str(" in ");
        entry.push_str(pr);
    }
    entry
}

/// A `## <heading>` block, or `None` when the section has no entries.
fn render_block(heading: &str, lines: &[String]) -> Option<String> {
    if lines.is_empty() {
        return None;
    }
    Some(format!("## {heading}\n\n{}", lines.join("\n")))
}

/// The `Contributors` shape shared by the all/new contributor sections.
fn render_contributors(heading: &str, contributors: &[&PluginContributor]) -> Option<String> {
    let lines: Vec<String> = contributors
        .iter()
        .map(|contributor| format!("* @{}", contributor.name))
        .collect();
    render_block(heading, &lines)
}

/// The trailing full-changelog line, or `None` without a compare URL.
fn render_compare(compare_url: Option<&str>) -> Option<String> {
    compare_url.map(|url| format!("**Full Changelog**: {url}"))
}

/// Renders the release notes body for `request`.
fn render_notes(request: &ChangelogRenderRequest) -> String {
    let mut blocks: Vec<String> = Vec::new();

    for section in Section::ORDERED {
        let lines: Vec<String> = request
            .commits
            .iter()
            .filter(|commit| section.selects(commit))
            .map(render_entry)
            .collect();
        blocks.extend(render_block(section.heading(), &lines));
    }

    let all: Vec<&PluginContributor> = request.contributors.iter().collect();
    let new: Vec<&PluginContributor> = request
        .contributors
        .iter()
        .filter(|contributor| contributor.is_first_contribution)
        .collect();
    blocks.extend(render_contributors("Contributors", &all));
    blocks.extend(render_contributors("New Contributors", &new));
    blocks.extend(render_compare(request.compare_url.as_deref()));

    let mut body = blocks.join("\n\n");
    if !body.is_empty() {
        body.push('\n');
    }
    body
}

/// Decodes an invocation envelope, renders the payload, and encodes the
/// response. This is the full plugin path without the Extism host binding, so
/// tests can drive it directly.
///
/// # Errors
///
/// Returns [`PluginError`] for a malformed envelope, an unsupported capability
/// or operation, a payload that does not match [`ChangelogRenderRequest`], or a
/// response that cannot be encoded.
pub fn render_invocation(input: &str) -> Result<String, PluginError> {
    let invocation: PluginInvocation =
        serde_json::from_str(input).map_err(PluginError::Envelope)?;

    if invocation.capability != Capability::ChangelogV1 {
        return Err(PluginError::UnsupportedCapability {
            capability: invocation.capability.as_str().to_string(),
        });
    }
    if invocation.operation != PluginOperation::Render {
        return Err(PluginError::UnsupportedOperation {
            operation: invocation.operation.as_str().to_string(),
        });
    }

    let request: ChangelogRenderRequest =
        serde_json::from_value(invocation.payload).map_err(PluginError::Payload)?;
    let response = ChangelogRenderResponse {
        body: render_notes(&request),
    };
    serde_json::to_string(&response).map_err(PluginError::Response)
}

/// The plugin's single exported entry point.
///
/// Only compiled for WebAssembly: see the import comment above for why a native
/// build cannot link it.
///
/// # Errors
///
/// Returns the [`PluginError`] from [`render_invocation`] as the plugin error
/// return; it never panics.
#[cfg(target_arch = "wasm32")]
#[plugin_fn]
pub fn invoke(input: String) -> FnResult<String> {
    render_invocation(&input).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(
        message: &str,
        kind: Option<&str>,
        pr: Option<&str>,
        breaking: bool,
    ) -> PluginCommitEntry {
        PluginCommitEntry {
            sha: "a1b2c3d4e5f6a7b8c9d0".to_string(),
            message: message.to_string(),
            r#type: kind.map(str::to_string),
            scope: None,
            author_name: None,
            pr_number: pr.map(str::to_string),
            is_breaking: breaking,
        }
    }

    fn person(name: &str, first: bool) -> PluginContributor {
        PluginContributor {
            name: name.to_string(),
            is_first_contribution: first,
        }
    }

    fn request(
        commits: Vec<PluginCommitEntry>,
        contributors: Vec<PluginContributor>,
    ) -> ChangelogRenderRequest {
        ChangelogRenderRequest {
            root_dir: "/workspace".to_string(),
            version: "1.0.0".to_string(),
            tag_name: "v1.0.0".to_string(),
            previous_tag: None,
            release_date: "2026-01-01".to_string(),
            commits,
            repository: None,
            compare_url: None,
            is_prerelease: false,
            contributors,
        }
    }

    #[test]
    fn groups_known_types_in_fixed_section_order() {
        let notes = render_notes(&request(
            vec![
                commit("feat: a", Some("feat"), None, false),
                commit("fix: b", Some("fix"), None, false),
                commit("perf: c", Some("perf"), None, false),
                commit("refactor: d", Some("refactor"), None, false),
                commit("docs: e", Some("docs"), None, false),
                commit("chore: f", Some("chore"), None, false),
                commit("plain", None, None, false),
            ],
            vec![],
        ));

        let headings = [
            "## Features",
            "## Bug Fixes",
            "## Performance",
            "## Refactoring",
            "## Documentation",
            "## Other Changes",
        ];
        let mut cursor = 0;
        for heading in headings {
            let at = notes.find(heading).expect("heading present");
            assert!(at >= cursor, "{heading} out of order");
            cursor = at;
        }
        assert!(notes.contains("* feat: a (a1b2c3d)"));
        assert!(notes.contains("* chore: f (a1b2c3d)"));
        assert!(notes.contains("* plain (a1b2c3d)"));
    }

    #[test]
    fn omits_empty_sections() {
        let notes = render_notes(&request(
            vec![commit("feat: a", Some("feat"), None, false)],
            vec![],
        ));
        assert!(notes.contains("## Features"));
        assert!(!notes.contains("## Breaking Changes"));
        assert!(!notes.contains("## Bug Fixes"));
        assert!(!notes.contains("## Other Changes"));
        assert!(!notes.contains("## Contributors"));
    }

    #[test]
    fn routes_breaking_change_only_to_breaking_section() {
        let notes = render_notes(&request(
            vec![commit(
                "feat!: drop legacy config",
                Some("feat"),
                None,
                true,
            )],
            vec![],
        ));
        assert!(notes.contains("## Breaking Changes"));
        assert!(notes.contains("* feat!: drop legacy config (a1b2c3d)"));
        assert!(!notes.contains("## Features"));
    }

    #[test]
    fn renders_short_sha_and_pr_suffix() {
        let notes = render_notes(&request(
            vec![commit(
                "fix: correct tag prefix",
                Some("fix"),
                Some("#43"),
                false,
            )],
            vec![],
        ));
        assert!(notes.contains("* fix: correct tag prefix (a1b2c3d) in #43"));
    }

    #[test]
    fn short_sha_never_splits_multibyte_input() {
        assert_eq!(short_sha("a1b2c3d4e5"), "a1b2c3d");
        assert_eq!(short_sha("abc"), "abc");
        assert_eq!(short_sha("ábcdefgh"), "ábcdefg");
    }

    #[test]
    fn filters_new_contributors_and_keeps_order() {
        let notes = render_notes(&request(
            vec![],
            vec![
                person("Alice", false),
                person("Bob", true),
                person("Carol", true),
            ],
        ));
        assert!(notes.contains("## Contributors\n\n* @Alice\n* @Bob\n* @Carol"));
        assert!(notes.contains("## New Contributors\n\n* @Bob\n* @Carol"));
    }

    #[test]
    fn appends_compare_line_only_when_present() {
        let mut with = request(vec![], vec![]);
        with.compare_url = Some("https://example.test/compare".to_string());
        assert_eq!(
            render_notes(&with),
            "**Full Changelog**: https://example.test/compare\n"
        );
        assert!(!render_notes(&request(vec![], vec![])).contains("Full Changelog"));
    }

    #[test]
    fn unrecognised_wire_values_report_envelope_errors() {
        // The published envelope fields are typed enums, so a value outside the
        // capability/operation domain no longer decodes. This used to surface
        // as `UnsupportedCapability`/`UnsupportedOperation`; it now fails at the
        // envelope boundary, and a bogus value must not panic.
        let capability = r#"{"capability":"not-a-capability","operation":"render","payload":{}}"#;
        assert!(matches!(
            render_invocation(capability),
            Err(PluginError::Envelope(_))
        ));
        let operation =
            r#"{"capability":"changelog.v1","operation":"not-an-operation","payload":{}}"#;
        assert!(matches!(
            render_invocation(operation),
            Err(PluginError::Envelope(_))
        ));
    }

    #[test]
    fn rejects_invalid_invocations() {
        assert!(matches!(
            render_invocation("not json"),
            Err(PluginError::Envelope(_))
        ));
        // Valid-but-unsupported values: the typed envelope decodes these, so
        // the plugin's own capability/operation checks are what reject them.
        let capability = r#"{"capability":"manifest.v1","operation":"render","payload":{}}"#;
        assert!(matches!(
            render_invocation(capability),
            Err(PluginError::UnsupportedCapability { .. })
        ));
        let operation = r#"{"capability":"changelog.v1","operation":"compute","payload":{}}"#;
        assert!(matches!(
            render_invocation(operation),
            Err(PluginError::UnsupportedOperation { .. })
        ));
        let shape = r#"{"capability":"changelog.v1","operation":"render","payload":{"version":1}}"#;
        assert!(matches!(
            render_invocation(shape),
            Err(PluginError::Payload(_))
        ));
    }
}
