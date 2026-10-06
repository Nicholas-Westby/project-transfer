//! The log line a finished transfer leaves, kept apart so the executor stays short.

use crate::model::Direction;
use crate::transfer::Summary;
use tracing::info;

/// One line per transfer with what moved and how fast, to tell a slow
/// network from a slow disk later.
pub(super) fn log_done(direction: Direction, peer: &str, s: &Summary) {
    let (verb, way) = match direction {
        Direction::Push => ("pushed", "to"),
        Direction::Pull => ("pulled", "from"),
    };
    let secs = s.took_ms as f64 / 1000.0;
    let mb = s.bytes as f64 / 1e6;
    info!(
        "{verb} {} files ({mb:.1} MB) {way} {peer} in {secs:.1} s, {:.2} MB/s; \
         {} removed, {} not applied",
        s.files,
        mb / secs.max(0.001),
        s.removed,
        s.failures.len()
    );
}
