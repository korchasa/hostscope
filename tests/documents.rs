//! The README describes the application with the application's own words: the
//! text under the screenshots is what `--help` prints, and nothing is written
//! there by hand (section 11 of the requirements). A description kept in two
//! places is a description that disagrees with itself, and the one a reader
//! trusts least is the one in the repository.

mod support;

use support::run;

/// The help no longer arrives as one block: `scripts/readme-help.py` fences the
/// tables of options and keys, where the columns carry the meaning, and lets
/// the paragraphs be paragraphs, rewrapped for the page. So a table is looked
/// for as it stands and a paragraph with its spacing flattened - what is held
/// identical is the words, not the line breaks around them.
#[test]
fn the_readme_carries_the_help_text() {
    let help = run(&["--help"]);
    let readme = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md"))
        .expect("README.md is part of the repository");
    let flat = flatten(&readme);

    let mut blocks = 0;
    for block in help.trim().split("\n\n") {
        blocks += 1;
        let found = if block.lines().any(|l| l.starts_with("  ")) {
            readme.contains(block)
        } else {
            flat.contains(&flatten(block))
        };
        let first = block.lines().next().unwrap_or_default();
        assert!(
            found,
            "README.md is missing the help block starting \"{first}\"; run `make readme`"
        );
    }
    assert!(blocks > 1, "the help text did not split into blocks");
}

fn flatten(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The help is for someone who wants to run the application, not for someone
/// who works on it. The hooks of FR-17 still work and are documented where a
/// reader who needs them already is - section 4 of `docs/testing.md`. Keeping
/// them out of the help keeps them out of the README too, because the README
/// is written from the help.
#[test]
fn the_help_keeps_the_verification_hooks_out() {
    let help = run(&["--help"]);
    // The README as well, and not only through the help: a block dropped from
    // `USAGE` stays in `README.md` until `make readme` runs, and the test that
    // carries the help across cannot see it, because it only looks for what
    // the help says and never for what the page says back.
    let readme = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md"))
        .expect("README.md is part of the repository");
    for hook in [
        "--proc-root",
        "--cgroup-root",
        "--dump-model",
        "--dump-frame",
        "--dump-style",
        "--keys",
        "--size",
        "--log",
        "--tick",
        "--docker-socket",
        "--no-etc-passwd",
    ] {
        assert!(
            !help.contains(hook),
            "--help names {hook}, a verification hook; section 4 of docs/testing.md is where those live"
        );
        assert!(
            !readme.contains(hook),
            "README.md names {hook}, a verification hook; run `make readme`"
        );
    }
    // The other half of the same rule: what a reader who only wants to run it
    // does need must not be swept out with them.
    for option in ["--theme", "--help", "--version"] {
        assert!(
            help.contains(option),
            "--help no longer names {option}, which someone running the application needs"
        );
    }
}

/// A screenshot that has been renamed or dropped leaves a broken picture on the
/// front page of the repository, and nothing else notices: the file is not
/// compiled, not linted, and read by no other test. This walks the links the
/// README actually writes.
#[test]
fn every_picture_the_readme_shows_is_in_the_repository() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let readme = std::fs::read_to_string(root.join("README.md"))
        .expect("README.md is part of the repository");

    let mut seen = 0;
    for tail in readme.split("](").skip(1) {
        let path = match tail.split_once(')') {
            Some((p, _)) => p,
            None => continue,
        };
        // Only the pictures: an ordinary link may point at a heading, a
        // document or the internet, and none of those is a file to look for.
        if !path.ends_with(".png") && !path.ends_with(".svg") {
            continue;
        }
        seen += 1;
        assert!(
            root.join(path).is_file(),
            "README.md shows {path}, which is not in the repository"
        );
    }
    assert!(seen > 0, "README.md shows no picture at all");
}
