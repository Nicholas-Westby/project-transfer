//! The log line a transfer leaves when it ends, kept apart so the executor
//! stays short.

use crate::model::Direction;
use crate::transfer::Summary;
use tracing::info;

/// One line per transfer with what moved and how fast, to tell a slow
/// network from a slow disk later. One that was cancelled or failed says how
/// far it got, since a slow transfer is the one most likely to be cancelled.
pub(super) fn log_done(direction: Direction, peer: &str, s: &Summary, stopped: bool) {
    info!("{}", summary_line(direction, peer, s, stopped));
}

fn summary_line(direction: Direction, peer: &str, s: &Summary, stopped: bool) -> String {
    let (verb, way) = match (direction, stopped) {
        (Direction::Push, false) => ("pushed", "to"),
        (Direction::Pull, false) => ("pulled", "from"),
        (Direction::Push, true) => ("stopped after pushing", "to"),
        (Direction::Pull, true) => ("stopped after pulling", "from"),
    };
    let secs = s.took_ms as f64 / 1000.0;
    let mb = s.bytes as f64 / 1e6;
    format!(
        "{verb} {} files ({mb:.1} MB) {way} {peer} in {secs:.1} s, {:.2} MB/s; \
         {} removed, {} not applied",
        s.files,
        mb / secs.max(0.001),
        s.removed,
        s.failures.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary() -> Summary {
        Summary {
            files: 3,
            bytes: 1_500_000,
            removed: 1,
            took_ms: 2_000,
            failures: vec![("a.txt".into(), "The file is in use.".into())],
        }
    }

    #[test]
    fn a_finished_transfer_says_what_moved_and_how_fast() {
        assert_eq!(
            summary_line(Direction::Push, "Desk", &summary(), false),
            "pushed 3 files (1.5 MB) to Desk in 2.0 s, 0.75 MB/s; 1 removed, 1 not applied"
        );
    }

    #[test]
    fn a_transfer_that_stops_early_says_how_far_it_got() {
        assert_eq!(
            summary_line(Direction::Pull, "Desk", &summary(), true),
            "stopped after pulling 3 files (1.5 MB) from Desk in 2.0 s, 0.75 MB/s; \
             1 removed, 1 not applied"
        );
        let push = summary_line(Direction::Push, "Desk", &summary(), true);
        assert!(
            push.starts_with("stopped after pushing 3 files (1.5 MB) to Desk"),
            "{push}"
        );
    }
}
