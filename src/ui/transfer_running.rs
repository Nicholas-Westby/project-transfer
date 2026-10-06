//! The running transfer: progress, counts and the file in hand.

use super::theme::Palette;
use super::widgets::{self, plural};
use crate::units::{about, clock, size};
use egui::text::{LayoutJob, TextWrapping};
use egui::{Align, Galley, Label, Layout, ProgressBar, TextFormat, TextStyle, Ui, vec2};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Rows the current path may take. The box is always this tall, so the
/// sheet, pinned at the top, keeps its height as paths come and go.
const PATH_ROWS: usize = 3;

/// How often the clock is redrawn when no progress arrives. egui takes a
/// frame's length off such a wait, and egui_kittest's frame is a quarter of a
/// second, so a wait of that or less never lets its `run` settle.
const CLOCK_TICK: Duration = Duration::from_millis(500);

/// The sheet's content; true when Cancel was clicked.
pub(super) fn body(
    ui: &mut Ui,
    done: u64,
    total: u64,
    files: u64,
    current: &str,
    started: Instant,
    left: Option<Duration>,
) -> bool {
    let frac = if total == 0 {
        0.0
    } else {
        done as f32 / total as f32
    };
    ui.add(ProgressBar::new(frac).desired_height(8.0));
    ui.add_space(4.0);
    widgets::small_muted(
        ui,
        format!(
            "{} of {}, {}",
            size(done),
            size(total),
            plural(files, "file", "files")
        ),
    );
    widgets::small_muted(ui, time_line(started.elapsed(), left));
    // The clock moves even while no progress arrives.
    ui.ctx().request_repaint_after(CLOCK_TICK);
    path_box(ui, current);
    widgets::small_muted(
        ui,
        "Cancelling keeps the files already copied. Nothing is left half-written.",
    );
    ui.add_space(12.0);
    super::dialogs::right_row(ui, |ui| ui.button("Cancel transfer").clicked()).inner
}

/// "12 min 34 s elapsed, about 25 min left". The estimate joins in once there
/// is one; the line itself is always there, so the sheet keeps its height.
fn time_line(elapsed: Duration, left: Option<Duration>) -> String {
    let mut line = format!("{} elapsed", clock(elapsed));
    if let Some(left) = left {
        line.push_str(&format!(", {} left", about(left)));
    }
    line
}

fn path_box(ui: &mut Ui, path: &str) {
    let font = TextStyle::Monospace.resolve(ui.style());
    let color = Palette::of(ui.ctx()).muted();
    let width = ui.available_width();
    let layout = |text: String| {
        let mut job = LayoutJob::single_section(text, TextFormat::simple(font.clone(), color));
        job.wrap = TextWrapping {
            max_width: width,
            max_rows: PATH_ROWS,
            break_anywhere: true,
            overflow_character: Some('…'),
        };
        ui.ctx().fonts_mut(|f| f.layout_job(job))
    };
    // Measured rather than worked out from the font's row height: at display
    // scales that round each row up to a whole pixel, a full path would be
    // taller than three of those and push the button down.
    let height = layout("\n".repeat(PATH_ROWS - 1)).size().y;
    let galley = fit(path, layout);
    ui.allocate_ui_with_layout(vec2(width, height), Layout::top_down(Align::Min), |ui| {
        ui.set_min_size(vec2(width, height));
        ui.add(Label::new(galley));
    });
}

/// `path` laid out whole if it fits, else with characters taken from the
/// middle so its start and its file name both stay.
fn fit(path: &str, layout: impl Fn(String) -> Arc<Galley>) -> Arc<Galley> {
    let whole = layout(path.to_string());
    if !whole.elided {
        return whole;
    }
    let chars: Vec<char> = path.chars().collect();
    // The most characters that still fit beside the "…".
    let (mut lo, mut hi) = (0, chars.len());
    while lo < hi {
        let keep = (lo + hi).div_ceil(2);
        if layout(middle_cut(&chars, keep)).elided {
            hi = keep - 1;
        } else {
            lo = keep;
        }
    }
    layout(middle_cut(&chars, lo))
}

/// `chars` cut down to `keep` of them, "…" standing in for the middle.
fn middle_cut(chars: &[char], keep: usize) -> String {
    let head = keep / 2;
    let tail = keep - head;
    let mut s: String = chars[..head].iter().collect();
    s.push('…');
    s.extend(&chars[chars.len() - tail..]);
    s
}

#[cfg(test)]
mod tests {
    use super::{middle_cut, time_line};
    use std::time::Duration;

    #[test]
    fn the_time_line_has_an_estimate_only_once_there_is_one() {
        let s = Duration::from_secs;
        assert_eq!(
            time_line(s(754), Some(s(1_500))),
            "12 min 34 s elapsed, about 25 min left"
        );
        assert_eq!(time_line(s(3), None), "3 s elapsed");
    }

    #[test]
    fn the_middle_goes_and_both_ends_stay() {
        let c: Vec<char> = "src/deep/folder/name.jpg".chars().collect();
        assert_eq!(middle_cut(&c, 10), "src/d…e.jpg");
        assert_eq!(middle_cut(&c, 0), "…");
    }

    #[test]
    fn letters_of_any_width_are_cut_whole() {
        let c: Vec<char> = "fotos/über/日本語/😀.png".chars().collect();
        assert_eq!(middle_cut(&c, 9), "foto…😀.png");
    }
}
