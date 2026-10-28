use super::*;

#[test]
fn bad_rels_are_refused() {
    for rel in [
        "", "/a", "\\a", "C:", "c:/x", "C:x", "a/../b", "..", "./a", "a/./b", "a//b", "a/", "a\0b",
        "a\\..\\b", "..\\x", "a\\b", "dir/x\\y",
    ] {
        assert!(validate_rel(rel).is_err(), "{rel:?} should be refused");
    }
    for rel in ["a", "a/b.txt", ".git/config", "a..b", "a b/c"] {
        assert!(validate_rel(rel).is_ok(), "{rel:?} should pass");
    }
    // Names Windows can't hold: one ending in a dot, and one with a colon,
    // which it reads as a stream of file `ab`.
    for rel in ["...", "ab:c"] {
        assert_eq!(validate_rel(rel).is_ok(), !cfg!(windows), "{rel:?}");
    }
}

#[test]
fn bad_names_are_refused() {
    for n in ["", ".", "..", "a/b", "a\\b", "C:", "a\0"] {
        assert!(validate_name(n).is_err(), "{n:?}");
    }
    assert!(validate_name("My project").is_ok());
}
