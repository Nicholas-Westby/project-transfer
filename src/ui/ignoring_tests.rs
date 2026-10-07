use super::*;
use crate::ignore_rules::{IgnoreSpec, Matcher, trim};

fn matcher(pattern: &str) -> Matcher {
    Matcher::new(&IgnoreSpec {
        patterns: vec![pattern.to_string()],
        ..Default::default()
    })
    .unwrap()
}

fn patterns(rel: &str, is_dir: bool) -> Vec<String> {
    suggestions(rel, is_dir)
        .into_iter()
        .map(|s| s.pattern)
        .collect()
}

#[test]
fn a_file_is_offered_from_itself_up_to_its_type() {
    let rel = "plans/2026/north/bed-01.png";
    assert_eq!(
        patterns(rel, false),
        [
            "/plans/2026/north/bed-01.png",
            "/plans/2026/north/",
            "/plans/2026/",
            "/plans/",
            "north/",
            "bed-01.png",
            "*.png",
        ]
    );
    let labels: Vec<_> = suggestions(rel, false)
        .into_iter()
        .map(|s| s.label)
        .collect();
    assert_eq!(labels[0], "This file only");
    assert_eq!(labels[1], "Everything in `plans/2026/north`");
    assert_eq!(labels[4], "Any folder named `north`");
    assert_eq!(labels[5], "Anything named `bed-01.png`");
    assert_eq!(labels[6], "Every `.png` file");
}

#[test]
fn only_the_nearest_folders_are_offered() {
    let p = patterns("a/b/c/d/e/f.txt", false);
    assert_eq!(p[1..5], ["/a/b/c/d/e/", "/a/b/c/d/", "/a/b/c/", "/a/b/"]);
    assert_eq!(p[5], "e/");
}

#[test]
fn a_folder_is_offered_from_itself() {
    assert_eq!(
        patterns("old-sketches", true),
        ["/old-sketches/", "old-sketches/"]
    );
    assert_eq!(
        patterns("art/old-sketches", true),
        ["/art/old-sketches/", "/art/", "old-sketches/"]
    );
    assert_eq!(patterns(".git", true), ["/.git/", ".git/"]);
}

#[test]
fn a_file_at_the_top_without_a_type_gets_no_type_or_folder() {
    assert_eq!(patterns("Makefile", false), ["/Makefile", "Makefile"]);
    assert_eq!(patterns(".env", false), ["/.env", ".env"]);
}

#[test]
fn suggested_patterns_match_names_with_pattern_characters() {
    let rel = "notes/[old] {draft}*.md";
    let all = suggestions(rel, false);
    assert_eq!(all[0].pattern, r"/notes/\[old\] \{draft\}\*.md");
    for s in &all {
        assert!(matcher(&s.pattern).leaves_out(rel, false), "{}", s.pattern);
    }
    // Unescaped, the file's own patterns would take this neighbour too.
    let neighbour = "notes/o draft-2.md";
    assert!(matcher("/notes/[old] {draft}*.md").leaves_out(neighbour, false));
    assert!(!matcher(&all[0].pattern).leaves_out(neighbour, false));
    assert!(!matcher(&all[3].pattern).leaves_out(neighbour, false));
}

#[test]
fn names_with_spaces_at_either_end_survive_trimming() {
    for rel in ["notes/ lead.md", "notes/trail.md "] {
        for s in suggestions(rel, false) {
            let kept = trim(&s.pattern);
            assert!(matcher(kept).leaves_out(rel, false), "{rel:?}: {kept:?}");
        }
    }
}

#[test]
fn a_pattern_starting_with_a_folder_name_is_offered_without_it() {
    let folders = ["app", "plant-data"];
    let fix = |p| without_folder_name(p, &folders);
    assert_eq!(
        fix("app/src/tmp/"),
        Some(("app".into(), "/src/tmp/".into()))
    );
    assert_eq!(fix("/app/exports"), Some(("app".into(), "/exports".into())));
    assert_eq!(
        fix("!app/keep.txt"),
        Some(("app".into(), "!/keep.txt".into()))
    );
    assert_eq!(
        fix("app/**/x.log"),
        Some(("app".into(), "/**/x.log".into()))
    );
    assert_eq!(fix("app/"), None);
    assert_eq!(fix("src/app/x"), None);
    assert_eq!(fix("*.log"), None);
    // Without the name these would leave out everything.
    assert_eq!(fix("app/**"), None);
    assert_eq!(fix("app/*"), None);
}
