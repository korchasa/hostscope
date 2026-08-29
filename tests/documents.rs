//! The README describes the application with the application's own words: the
//! text under the screenshot is what `--help` prints, and nothing is written
//! there by hand (section 11 of the requirements). A description kept in two
//! places is a description that disagrees with itself, and the one a reader
//! trusts least is the one in the repository.

mod support;

use support::run;

#[test]
fn the_readme_carries_the_help_text() {
    let help = run(&["--help"]);
    let readme = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md"))
        .expect("README.md is part of the repository");
    assert!(
        readme.contains(help.trim_end()),
        "README.md does not carry what --help prints; run `make readme`"
    );
}
