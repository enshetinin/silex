//! Release checks (ADR 0014): what the tag workflow also checks, caught
//! before tagging.

/// `CHANGELOG.md` has a section for the version in `Cargo.toml`, and
/// `scripts/release-notes.sh` would find it.
#[test]
fn the_changelog_has_a_section_for_this_version() {
    let changelog = include_str!("../CHANGELOG.md");
    let heading = format!("## [{}]", env!("CARGO_PKG_VERSION"));
    let at = changelog
        .lines()
        .position(|line| line.starts_with(&heading))
        .unwrap_or_else(|| panic!("CHANGELOG.md has no «{heading}» section"));
    let body: Vec<&str> = changelog
        .lines()
        .skip(at + 1)
        .take_while(|line| !line.starts_with("## ["))
        .filter(|line| !line.trim().is_empty())
        .collect();
    assert!(!body.is_empty(), "the «{heading}» section is empty");
}
