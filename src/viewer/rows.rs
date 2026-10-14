//! Two copies of a text lined up row by row, so the same lines sit side
//! by side and a line only one copy has faces a gap.

use similar::{Algorithm, DiffTag, capture_diff_slices_deadline};
use std::time::{Duration, Instant};

/// One row of the side-by-side view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    /// The line of the left copy shown on this row; None leaves a gap.
    pub left: Option<usize>,
    pub right: Option<usize>,
    /// The two sides differ here: a changed line, or one only one side has.
    pub differs: bool,
}

/// Lines `left` and `right` up, the way a line diff matches them, giving
/// up on a closer match once `budget` has passed. Every line of each side
/// appears exactly once, in order.
pub fn align(left: &[&str], right: &[&str], budget: Duration) -> Vec<Row> {
    let deadline = Some(Instant::now() + budget);
    let ops = capture_diff_slices_deadline(Algorithm::Myers, left, right, deadline);
    let mut rows = Vec::with_capacity(left.len().max(right.len()));
    for op in ops {
        let (tag, old, new) = op.as_tag_tuple();
        match tag {
            DiffTag::Equal => rows.extend(old.zip(new).map(|(l, r)| Row {
                left: Some(l),
                right: Some(r),
                differs: false,
            })),
            DiffTag::Delete => rows.extend(old.map(|l| Row {
                left: Some(l),
                right: None,
                differs: true,
            })),
            DiffTag::Insert => rows.extend(new.map(|r| Row {
                left: None,
                right: Some(r),
                differs: true,
            })),
            // Changed lines face each other; the longer side runs on
            // against gaps.
            DiffTag::Replace => {
                for k in 0..old.len().max(new.len()) {
                    rows.push(Row {
                        left: (k < old.len()).then(|| old.start + k),
                        right: (k < new.len()).then(|| new.start + k),
                        differs: true,
                    });
                }
            }
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUDGET: Duration = Duration::from_secs(2);

    fn row(left: Option<usize>, right: Option<usize>, differs: bool) -> Row {
        Row {
            left,
            right,
            differs,
        }
    }

    #[test]
    fn changed_lines_face_each_other_and_added_ones_face_a_gap() {
        let here = ["fn main() {", "    old();", "}", "// end"];
        let there = [
            "fn main() {",
            "    new();",
            "}",
            "fn extra() {}",
            "// end",
            "// more",
        ];
        assert_eq!(
            align(&here, &there, BUDGET),
            vec![
                row(Some(0), Some(0), false),
                row(Some(1), Some(1), true),
                row(Some(2), Some(2), false),
                row(None, Some(3), true),
                row(Some(3), Some(4), false),
                row(None, Some(5), true),
            ]
        );
    }

    #[test]
    fn empty_copies_give_no_rows_and_one_empty_side_gives_gaps() {
        assert!(align(&[], &[], BUDGET).is_empty());
        assert_eq!(align(&["a"], &[], BUDGET), vec![row(Some(0), None, true)]);
    }

    #[test]
    fn a_large_file_that_changed_everywhere_still_lines_up_every_line_in_time() {
        let here: Vec<String> = (0..20_000)
            .map(|i| format!("line {}", i * 7 % 1000))
            .collect();
        let there: Vec<String> = (0..20_000)
            .map(|i| format!("line {}", i * 13 % 997))
            .collect();
        let here: Vec<&str> = here.iter().map(String::as_str).collect();
        let there: Vec<&str> = there.iter().map(String::as_str).collect();
        let rows = align(&here, &there, Duration::from_millis(300));
        let left: Vec<usize> = rows.iter().filter_map(|r| r.left).collect();
        let right: Vec<usize> = rows.iter().filter_map(|r| r.right).collect();
        assert_eq!(left, (0..20_000).collect::<Vec<_>>());
        assert_eq!(right, (0..20_000).collect::<Vec<_>>());
    }
}
