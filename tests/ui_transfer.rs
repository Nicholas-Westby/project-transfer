//! What the transfer sheet says, over a hand-built state.

mod ui_support;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use project_transfer::core::TransferState;
use project_transfer::transfer::Summary;
use project_transfer::ui::App;
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
