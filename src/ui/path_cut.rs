//! Paths shortened to fit a table cell.

/// `path` whole if it fits, else with "…" in place of the folders before
/// its last name. The last name matters most, so it always stays; then as
/// much of the start as fits. Takes `/` and `\`, as the other computer may
/// use either. When even "…" and the last name don't fit, the cell cuts
/// the end off as usual.
pub fn path_cut(path: &str, fits: impl Fn(&str) -> bool) -> String {
    if fits(path) {
        return path.to_string();
    }
    let trimmed = path.trim_end_matches(['/', '\\']);
    let Some(sep) = trimmed.rfind(['/', '\\']) else {
        return path.to_string();
    };
    let head: Vec<char> = path[..sep].chars().collect();
    if head.is_empty() {
        return path.to_string();
    }
    let tail = &path[sep..];
    let cut = |keep: usize| {
        let mut s: String = head[..keep].iter().collect();
        s.push('…');
        s.push_str(tail);
        s
    };
    // The most of the start that still fits beside the "…".
    let (mut lo, mut hi) = (0, head.len().saturating_sub(1));
    while lo < hi {
        let keep = (lo + hi).div_ceil(2);
        if fits(&cut(keep)) {
            lo = keep;
        } else {
            hi = keep - 1;
        }
    }
    cut(lo)
}

#[cfg(test)]
mod tests {
    use super::path_cut;

    fn cut(path: &str, chars: usize) -> String {
        path_cut(path, |s| s.chars().count() <= chars)
    }

    #[test]
    fn a_path_that_fits_stays_whole() {
        assert_eq!(cut("~/Dev/app", 9), "~/Dev/app");
    }

    #[test]
    fn the_last_name_stays_and_the_start_keeps_what_fits() {
        let p = "/Users/jdoe/Dev/garden-planner/plant-data";
        assert_eq!(cut(p, 24), "/Users/jdoe/…/plant-data");
        assert_eq!(cut(p, 13), "/…/plant-data");
        assert_eq!(cut(p, 4), "…/plant-data");
        assert_eq!(cut(r"D:\dev\garden-planner\docs", 12), r"D:\dev…\docs");
    }

    #[test]
    fn a_trailing_separator_belongs_to_the_last_name() {
        assert_eq!(cut("/a/bbbbbb/cc/", 8), "/a/…/cc/");
    }

    #[test]
    fn a_path_without_folders_is_left_to_the_cell() {
        assert_eq!(cut("plant-data", 4), "plant-data");
        assert_eq!(cut("/plant-data", 4), "/plant-data");
    }

    #[test]
    fn letters_of_any_width_are_cut_whole() {
        assert_eq!(cut("/fotos/über/日本語/😀", 7), "/fot…/😀");
    }
}
