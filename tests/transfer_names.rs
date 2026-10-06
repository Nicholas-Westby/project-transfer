//! File names beyond plain ASCII arrive exactly as they were sent, byte for
//! byte, in both directions. A name only ever travels as UTF-8 text, so a
//! letter can't turn into another one on the way. The one change is made
//! before a name leaves a Mac: it sends the composed spelling of a letter
//! like "ú", because Windows tells the two spellings apart.

mod support;

#[cfg(target_os = "macos")]
use project_transfer::protocol::{Request, Response};
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

/// The name a file arrives under. A Mac sends the composed spelling, so the
/// name stored with a combining accent is the only one that changes there.
fn arrives_as(sent: &str) -> &str {
    match sent {
        "decomposed/soldu\u{301}-5313338.jpg" if cfg!(target_os = "macos") => {
            "decomposed/sold\u{fa}-5313338.jpg"
        }
        other => other,
    }
}

/// What the receiving folder holds. The content is what was written, so the
/// composed name still holds the decomposed text.
fn expected() -> Vec<(Vec<u8>, String)> {
    let mut want: Vec<(Vec<u8>, String)> = NAMES
        .iter()
        .map(|n| (arrives_as(n).as_bytes().to_vec(), n.to_string()))
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

/// A client may send both a write and a removal for two spellings of one
/// name; on a Mac they are one file, which must survive.
#[cfg(target_os = "macos")]
#[tokio::test]
async fn a_push_that_writes_one_spelling_and_removes_another_keeps_the_file() {
    let (a, b) = pair_full().await;
    let src = a.project_dir("media");
    write(&src, "keep.txt", "k");
    let p = a.add_project("Garden", &[("media", &src)]).await;
    push(&a, &b, p.id, false).await;
    let dest = b.dev().join("media");
    write(&dest, "soldu\u{301}.jpg", "old");
    let mut conn = a.open(&b).await;
    let begin = Request::BeginPush {
        project: a.project(p.id).await.unwrap(),
        folder: p.primary,
        expected_path: dest.display().to_string(),
    };
    assert_eq!(conn.request(&begin).await.unwrap(), Response::Ok);
    let put = Request::PutFile {
        rel: "sold\u{fa}.jpg".into(),
        size: 3,
        mtime_ms: 5,
        exec: false,
    };
    conn.send(&put).await.unwrap();
    conn.send_raw(b"new").await.unwrap();
    assert_eq!(conn.recv().await.unwrap(), Response::Ok);
    let remove = Request::Remove {
        rel: "soldu\u{301}.jpg".into(),
        is_dir: false,
    };
    assert_eq!(conn.request(&remove).await.unwrap(), Response::Ok);
    assert_eq!(read(&dest, "sold\u{fa}.jpg"), "new");
}

/// A pull from a computer that lists the file under the other spelling, such
/// as a Windows PC that kept the decomposed one, plans the same pair, and the
/// pull keeps its own applier for each folder it writes. A Mac lists both
/// spellings as one name, so the plan gets its removal by hand.
#[cfg(target_os = "macos")]
#[tokio::test]
async fn a_pull_that_writes_one_spelling_and_removes_another_keeps_the_file() {
    use project_transfer::{manifest::Change, model::Direction, transfer};
    let (a, b) = pair_full().await;
    let theirs = b.project_dir("media");
    write(&theirs, "keep.txt", "k");
    let p = b.add_project("Garden", &[("media", &theirs)]).await;
    pull(&a, &b, p.id).await;
    write(&theirs, "sold\u{fa}.jpg", "new");
    let mut conn = a.open(&b).await;
    let req = a.request(&b, p.id, Direction::Pull).await;
    let mut preview = transfer::prepare(&mut conn, &a.shared, req).await.unwrap();
    let mine = a.dev().join("media");
    write(&mine, "soldu\u{301}.jpg", "old");
    let remove = Change::RemoveFile("soldu\u{301}.jpg".into());
    preview.folders[0].plan.changes.push(remove);
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let cancel = Default::default();
    let summary = transfer::execute(&mut conn, &a.shared, preview, tx, cancel)
        .await
        .unwrap();
    assert!(summary.failures.is_empty(), "{:?}", summary.failures);
    assert_eq!(read(&mine, "sold\u{fa}.jpg"), "new");
}
