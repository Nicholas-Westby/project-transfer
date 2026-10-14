//! The sentences for a finished or failed transfer, apart from the code
//! that runs one.

use super::pairing::files_word;
use crate::model::Direction;
use crate::transfer::Summary;

pub(super) fn verb(d: Direction, past: bool) -> &'static str {
    match (d, past) {
        (Direction::Push, false) => "push",
        (Direction::Pull, false) => "pull",
        (Direction::Push, true) => "Pushed",
        (Direction::Pull, true) => "Pulled",
    }
}

/// "Pushed 42 files to Desktop Swift Heron in 3.1 s."
pub(super) fn done_sentence(d: Direction, peer: &str, s: &Summary) -> String {
    let dir = match d {
        Direction::Push => "to",
        Direction::Pull => "from",
    };
    let mut text = format!(
        "{} {} {dir} {peer} in {}.",
        verb(d, true),
        files_word(s.files),
        crate::units::took(std::time::Duration::from_millis(s.took_ms))
    );
    if s.removed > 0 {
        text.push_str(&format!(
            " Removed {}.",
            match s.removed {
                1 => "1 item".to_string(),
                n => format!("{n} items"),
            }
        ));
    }
    if !s.failures.is_empty() {
        text.push_str(&format!(
            " {} could not be copied; open the log folder in Settings to see why.",
            s.failures.len()
        ));
    }
    text
}
