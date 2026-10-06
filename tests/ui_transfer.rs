//! What the transfer sheet says, over a hand-built state.

mod ui_support;

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use project_transfer::core::TransferState;
use project_transfer::transfer::Summary;
use project_transfer::ui::App;
use std::time::{Duration, Instant};
use ui_support::{FakeBackend, sample_preview, ui_harness};

/// The sheet after a transfer that ended with `summary`. A preview comes
/// first, as in the app, so the sheet knows which request it is reporting on.
fn finished(summary: Summary) -> Harness<'static, App> {
    let fake = FakeBackend::seeded();
    fake.update(|s| s.transfer = TransferState::Ready(sample_preview(s)));
    let mut h = ui_harness(fake.clone());
    fake.update(|s| s.transfer = TransferState::Finished(summary));
    h.run();
    h
}

#[test]
fn a_finished_transfer_reads_its_time_in_minutes_and_its_speed() {
    let h = finished(Summary {
        files: 25_731,
        bytes: 9_973_000_000,
        took_ms: 2_409_300,
        ..Default::default()
    });
    h.get_by_label_contains("in 40 min 9 s.");
    h.get_by_label("10.0 GB sent at 4.1 MB/s.");
}

#[test]
fn a_transfer_too_small_to_measure_gives_no_speed() {
    let h = finished(Summary {
        files: 1,
        bytes: 14,
        took_ms: 75_000,
        ..Default::default()
    });
    h.get_by_label_contains("in 1 min 15 s.");
    h.get_by_label("14 bytes sent.");
}

#[test]
fn a_running_transfer_shows_elapsed_and_remaining_time() {
    let fake = FakeBackend::seeded();
    let mut h = ui_harness(fake.clone());
    // Set after the harness is built, so little of the real clock runs before
    // the sheet is read.
    fake.update(|s| {
        s.transfer = TransferState::Running {
            done: 5_400_000_000,
            total: 10_000_000_000,
            files: 25_731,
            current: "src/a.txt".into(),
            // A Windows clock counts from boot, so it may not reach back that far.
            started: Instant::now()
                .checked_sub(Duration::from_secs(754))
                .unwrap_or_else(Instant::now),
            left: Some(Duration::from_secs(1_500)),
        }
    });
    h.run();
    // The seconds are the real clock's, so they may have moved on by now.
    let line = h.get_by_label_contains("elapsed, about 25 min left");
    // A label keeps its text in the node's value, not its label.
    let text = line.accesskit_node().value().unwrap_or_default();
    assert!(text.starts_with("12 min "), "{text}");
}

#[test]
fn the_sheet_names_folders_it_leaves_out() {
    let fake = FakeBackend::seeded();
    fake.update(|s| s.transfer = TransferState::Ready(sample_preview(s)));
    let h = ui_harness(fake);
    h.get_by_label("Left out.");
    h.get_by_label_contains("pushing it would empty the copy");
}
