use super::*;
use crate::net::FilePart;
use crate::protocol::MAX_PART;
use crate::viewer::FileAsk;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use std::path::PathBuf;
use std::sync::Arc;

fn fetched(bytes: &[u8]) -> Option<Result<FilePart, String>> {
    Some(Ok(FilePart {
        size: bytes.len() as u64,
        mtime_ms: now_ms() - 3 * 60_000,
        bytes: bytes.to_vec(),
    }))
}

fn view(copies: Copies, shown: Option<Shown>) -> FileView {
    FileView {
        ask: FileAsk {
            peer: uuid::Uuid::from_u128(2),
            project: uuid::Uuid::from_u128(10),
            folder: uuid::Uuid::from_u128(20),
            local_root: PathBuf::from("/work/app"),
            rel: "src/main.rs".into(),
            copies,
        },
        opened: 1,
        shown: shown.map(Arc::new),
    }
}

/// The viewer over a push to Desk, and whether it asked to close.
fn harness(v: FileView) -> Harness<'static, bool> {
    Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui_state(
            move |ui, closed: &mut bool| {
                if sheet(ui, &v, Direction::Push, "Desk", None) {
                    *closed = true;
                }
            },
            false,
        )
}

#[test]
fn both_copies_show_side_by_side_with_what_the_push_does_to_each() {
    let shown = Shown::build(
        fetched(b"fn main() { grow(); }\n"),
        fetched(b"fn main() {}\n"),
    );
    let mut h = harness(view(Copies::Both, Some(shown)));
    h.get_by_label("Here");
    h.get_by_label("On Desk");
    h.get_by_label("Sent");
    h.get_by_label("Replaced");
    h.get_by_label("fn main() { grow(); }");
    h.get_by_label("fn main() {}");
    assert_eq!(h.get_all_by_label_contains("modified 3 min ago").count(), 2);
    assert!(!*h.state());
    h.get_by_label("Close").click();
    h.run();
    assert!(*h.state());
}

#[test]
fn a_file_only_here_shows_one_pane_and_says_what_the_push_does() {
    let shown = Shown::build(fetched(b"new\n"), None);
    let h = harness(view(Copies::Here, Some(shown)));
    h.get_by_label("Only this computer has this file. The push copies it to Desk.");
    h.get_by_label("Here");
    assert!(h.query_by_label("On Desk").is_none());
}

#[test]
fn a_copy_that_is_not_text_or_could_not_be_read_says_so() {
    let shown = Shown::build(
        fetched(b"PK\x03\x04\x00\x00"),
        Some(Err("Desk doesn't let this computer pull from it.".into())),
    );
    let h = harness(view(Copies::Both, Some(shown)));
    h.get_by_label("This copy isn't text, so it can't be shown. It is 6 bytes.");
    h.get_by_label("Desk doesn't let this computer pull from it.");
}

#[test]
fn different_line_endings_and_a_cut_copy_are_pointed_out() {
    let mut long = b"line\r\n".repeat(MAX_PART as usize / 6 + 1);
    long.truncate(MAX_PART as usize);
    let here = Some(Ok(FilePart {
        size: 3_000_000,
        mtime_ms: 0,
        bytes: long,
    }));
    let shown = Shown::build(here, fetched(b"line\n"));
    let h = harness(view(Copies::Both, Some(shown)));
    h.get_by_label("Line endings differ: CRLF here, LF on Desk. Lines are lined up without them.");
    h.get_by_label("Showing the first 1.0 MB of 3.0 MB.");
}

#[test]
fn copies_that_read_the_same_say_what_else_differs() {
    let lines = Shown::build(fetched(b"a\nb\n"), fetched(b"a\nb"));
    let h = harness(view(Copies::Both, Some(lines)));
    h.get_by_label(
        "The lines are the same in both copies. They differ only in line endings, a byte-order \
         mark or the line break at the end.",
    );
    let bytes = Shown::build(fetched(b"a\n"), fetched(b"a\n"));
    let h = harness(view(Copies::Both, Some(bytes)));
    h.get_by_label(
        "Both copies have the same content. The transfer rewrites this file for another reason, \
         most likely whether it can be run as a program.",
    );
}

#[test]
fn copies_that_match_only_in_the_part_read_say_so() {
    let text: String = (0..80_000).map(|i| format!("line number {i}\n")).collect();
    let bytes = text.as_bytes()[..MAX_PART as usize].to_vec();
    let part = |size: u64| {
        Some(Ok(FilePart {
            size,
            mtime_ms: 0,
            bytes: bytes.clone(),
        }))
    };
    let shown = Shown::build(part(3_000_000), part(3_100_000));
    assert_eq!(shown.alike, Some(Alike::Bytes));
    let h = harness(view(Copies::Both, Some(shown)));
    h.get_by_label(
        "The part shown reads the same in both copies. The difference is further on in the \
         file, or in something the viewer doesn't show.",
    );
    let whole = "Both copies have the same content";
    assert!(h.query_by_label_contains(whole).is_none());
}

#[test]
fn a_cut_copy_says_how_much_is_shown() {
    let part = Some(Ok(FilePart {
        size: 3_000_000,
        mtime_ms: 0,
        bytes: b"line\nline\n".to_vec(),
    }));
    let h = harness(view(Copies::Here, Some(Shown::build(part, None))));
    h.get_by_label("Showing the first 10 bytes of 3.0 MB.");
}

#[test]
fn a_copy_still_being_read_shows_a_spinner_beside_the_one_here() {
    let mut shown = Shown::build(fetched(b"fn main() {}\n"), None);
    shown.waiting = true;
    let h = harness(view(Copies::Both, Some(shown)));
    h.get_by_label("Reading the copy on Desk…");
    h.get_by_label("fn main() {}");
}

#[test]
fn the_copies_are_read_before_they_show() {
    let h = harness(view(Copies::Both, None));
    h.get_by_label("Reading the file here and on Desk…");
}

#[test]
fn each_opening_starts_at_the_top() {
    let text: String = (0..300).map(|i| format!("line {i}\n")).collect();
    let v = view(
        Copies::Here,
        Some(Shown::build(fetched(text.as_bytes()), None)),
    );
    let mut h = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui_state(
            |ui, v: &mut FileView| {
                sheet(ui, v, Direction::Push, "Desk", None);
            },
            v,
        );
    h.get_by_label("line 3").hover();
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -3000.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(4);
    assert!(
        h.query_by_label("line 0").is_none(),
        "the wheel moved the pane"
    );
    h.state_mut().opened = 2;
    h.run_steps(2);
    h.get_by_label("line 0");
}

#[test]
fn escape_closes_the_viewer() {
    let shown = Shown::build(fetched(b"a\n"), fetched(b"b\n"));
    let mut h = harness(view(Copies::Both, Some(shown)));
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(*h.state());
}
