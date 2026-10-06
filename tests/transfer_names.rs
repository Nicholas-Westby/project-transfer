//! File names beyond plain ASCII arrive exactly as they were sent, byte for
//! byte, in both directions. A name only ever travels as UTF-8 text, so a
//! letter can't turn into another one on the way. Both computers here run
//! the same system, and between two Macs names travel as stored. Toward a
//! computer that is not a Mac, a Mac sends a decomposed accented letter, such
//! as "u" plus the combining accent U+0301, as the one composed letter "ú"
//! (U+00FA), because Windows tells the two spellings apart. Nothing else is
//! replaced.

mod support;

#[cfg(target_os = "macos")]
use project_transfer::protocol::{Request, Response};
use std::path::Path;
use support::*;

/// Each in its own folder: macOS treats the two spellings of "ú" as one
/// name, so side by side they would collide.
const NAMES: [&str; 7] = [
    // "ú" as one code point, as most databases store it.
    "composed/sold\u{fa}-5313338.jpg",
    // "u" followed by a combining accent, as older Mac tools write it.
    "decomposed/soldu\u{301}-5313338.jpg",
    // Unicode composition would swap this ideograph for U+585A, though a
    // Mac's disk opens the file by either; it was never decomposed, so it
    // must keep its name.
    "compat/\u{fa10}.txt",
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

/// What the receiving folder holds: every name as it was written, each file
/// holding its own name as text.
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

/// A Mac lists a name it stores decomposed by its composed spelling only for
/// a computer that is not a Mac. Another Mac, and an older version that does
/// not say what it runs on, get the name as stored.
#[cfg(target_os = "macos")]
#[tokio::test]
async fn a_mac_lists_the_composed_name_only_for_a_computer_that_is_not_a_mac() {
    use project_transfer::{ignore_rules::IgnoreSpec, model::Os};
    let (a, b) = pair_full().await;
    let theirs = b.project_dir("media");
    write(&theirs, "soldu\u{301}.jpg", "x");
    let p = b.add_project("Garden", &[("media", &theirs)]).await;
    let mut conn = a.open(&b).await;
    for (from_os, listed) in [
        (Some(Os::Windows), "sold\u{fa}.jpg"),
        (Some(Os::MacOs), "soldu\u{301}.jpg"),
        (None, "soldu\u{301}.jpg"),
    ] {
        let ask = Request::Manifest {
            project: p.id,
            folder: p.primary,
            ignore: IgnoreSpec::default(),
            folder_name: "media".into(),
            project_name: "Garden".into(),
            multi_folder: false,
            from_os,
            home_hint: None,
        };
        let Response::Manifest(scan) = conn.request(&ask).await.unwrap() else {
            panic!("no folder scan when asked as {from_os:?}");
        };
        let rels: Vec<String> = scan.manifest.entries.into_iter().map(|e| e.rel).collect();
        assert_eq!(rels, [listed], "asked as {from_os:?}");
    }
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
        home_hints: Default::default(),
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

/// A Windows PC that kept the decomposed "ú" (a "u" and a combining accent)
/// lists the file that way, while this Mac lists its own as the composed "ú".
/// The plan adds the decomposed name and removes the composed one, which on a
/// Mac is the same file, and the pull keeps its own applier for each folder it
/// writes. The other computer here is a Mac, so the plan is edited by hand to
/// match.
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
    write(&mine, "sold\u{fa}.jpg", "old");
    let plan = &mut preview.folders[0].plan;
    let added = plan.changes.iter_mut().find_map(|c| match c {
        Change::Add(e) if e.rel == "sold\u{fa}.jpg" => Some(e),
        _ => None,
    });
    added.expect("the new file is planned as an add").rel = "soldu\u{301}.jpg".into();
    plan.changes
        .push(Change::RemoveFile("sold\u{fa}.jpg".into()));
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let cancel = Default::default();
    let summary = transfer::execute(&mut conn, &a.shared, preview, tx, cancel)
        .await
        .unwrap();
    assert!(summary.failures.is_empty(), "{:?}", summary.failures);
    assert_eq!(read(&mine, "soldu\u{301}.jpg"), "new");
}
