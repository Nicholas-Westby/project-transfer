//! File names beyond plain ASCII arrive exactly as they were sent, byte for
//! byte, in both directions. A name only ever travels as UTF-8 text, so a
//! letter can't turn into another one on the way.

mod support;

use std::path::Path;
use support::*;

/// Each in its own folder: macOS treats the two spellings of "ú" as one
/// name, so side by side they would collide.
const NAMES: [&str; 6] = [
    // "ú" as one code point, as most databases store it.
    "composed/sold\u{fa}-5313338.jpg",
    // "u" followed by a combining accent, as older Mac tools write it.
    "decomposed/soldu\u{301}-5313338.jpg",
    "cjk/\u{65e5}\u{672c}\u{8a9e}.txt",
    "emoji/\u{1f600}.png",
    // The half-width bracket a code-page mix-up leaves in place of "ú".
    "halfwidth/sold\u{ff63}-5313338.jpg",
    "caf\u{e9}/men\u{fc}.md",
];

fn fill(root: &Path) {
    for name in NAMES {
        write(root, name, name);
    }
}

/// Every file under `root` by its exact name, with its content.
fn names(root: &Path) -> Vec<(Vec<u8>, String)> {
    let mut out: Vec<(Vec<u8>, String)> = tree(root)
        .into_iter()
        .filter(|(_, (body, _))| body != "<dir>")
        .map(|(rel, (body, _))| (rel.into_bytes(), body))
        .collect();
    out.sort();
    out
}

fn expected() -> Vec<(Vec<u8>, String)> {
    let mut want: Vec<(Vec<u8>, String)> = NAMES
        .iter()
        .map(|n| (n.as_bytes().to_vec(), n.to_string()))
        .collect();
    want.sort();
    want
}

#[tokio::test]
async fn a_push_keeps_every_name_byte_for_byte() {
    let (a, b) = pair_full().await;
    let src = a.project_dir("media");
    fill(&src);
    let p = a.add_project("Garden", &[("media", &src)]).await;
    let (_, summary) = push(&a, &b, p.id, false).await;
    assert!(summary.failures.is_empty(), "{:?}", summary.failures);
    assert_eq!(names(&b.dev().join("media")), expected());
}

#[tokio::test]
async fn a_pull_keeps_every_name_byte_for_byte() {
    let (a, b) = pair_full().await;
    let theirs = b.project_dir("media");
    fill(&theirs);
    let p = b.add_project("Garden", &[("media", &theirs)]).await;
    let (_, summary) = pull(&a, &b, p.id).await;
    assert!(summary.failures.is_empty(), "{:?}", summary.failures);
    assert_eq!(names(&a.dev().join("media")), expected());
}
