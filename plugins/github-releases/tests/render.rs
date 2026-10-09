//! End-to-end test for the public rendering path.
//!
//! The fixture files are embedded with `include_str!`, so the test reads no
//! filesystem and makes no network call at runtime. The committed expected
//! notes are the specification: the assertion is byte equality.

use cutver_pdk::ChangelogRenderResponse;

const REQUEST: &str = include_str!("fixtures/changelog_request.json");
const EXPECTED_NOTES: &str = include_str!("fixtures/changelog_notes_expected.md");

#[test]
fn renders_the_canonical_fixture_exactly() {
    let response_json =
        github_releases::render_invocation(REQUEST).expect("fixture envelope renders");

    let response: ChangelogRenderResponse =
        serde_json::from_str(&response_json).expect("response decodes");

    // The renderer emits LF. A checkout on a platform with `core.autocrlf` enabled
    // can hand us CRLF for the fixture, and `.gitattributes` marks the fixtures
    // `-text` to prevent that — but a tree checked out before the attribute landed
    // would still fail on git configuration rather than on the render.
    let expected = EXPECTED_NOTES.replace("\r\n", "\n");

    assert_eq!(response.body, expected);
}
