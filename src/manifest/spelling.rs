//! The spelling a name travels under. A Mac's disk opens a file by the
//! composed or the decomposed spelling of letters like "ú", and tools and
//! older disks often leave the decomposed one; Windows disks and most
//! databases use the composed one and tell the two apart. So a Mac sends
//! the composed spelling (as Git, Syncthing and Dropbox do), and only when
//! it reaches the very same file.

use std::path::Path;

#[cfg(target_os = "macos")]
pub(super) fn composed(dir: &Path, name: String, meta: &std::fs::Metadata) -> String {
    use std::os::unix::fs::MetadataExt;
    let nfc = nfc(&name);
    if nfc == name {
        return name;
    }
    // NFC also replaces characters that were never decomposed (a compatibility
    // ideograph, the angstrom sign), and the disk opens the file by either, but
    // the Mac never held the new name. Letters made of parts, like "u" and an
    // accent, are the ones to compose: each part stays as it is on its own.
    if name.chars().any(|c| !c.is_ascii() && changes_alone(c)) {
        return name;
    }
    // A disk that keeps the spellings apart (some external ones) has two
    // names here, so the one on disk is the only one that works.
    match std::fs::symlink_metadata(dir.join(&nfc)) {
        Ok(m) if m.dev() == meta.dev() && m.ino() == meta.ino() => nfc,
        _ => name,
    }
}

#[cfg(not(target_os = "macos"))]
pub(super) fn composed(_dir: &Path, name: String, _meta: &std::fs::Metadata) -> String {
    name
}

/// Whether NFC turns `c` into something else even with nothing around it.
#[cfg(target_os = "macos")]
fn changes_alone(c: char) -> bool {
    let s = c.to_string();
    nfc(&s) != s
}

/// `s` in Unicode normalization form C, by Core Foundation.
#[cfg(target_os = "macos")]
fn nfc(s: &str) -> String {
    use core_foundation::base::TCFType;
    use core_foundation::string::{
        CFString, CFStringCreateMutableCopy, CFStringNormalize, kCFStringNormalizationFormC,
    };
    let cf = CFString::new(s);
    // SAFETY: a null allocator means the default one and a 0 length means no
    // limit. The copy is a +1 reference we own and hand to CFString, which
    // releases it; normalizing a mutable copy in place is what the API is for.
    unsafe {
        let copy = CFStringCreateMutableCopy(std::ptr::null(), 0, cf.as_concrete_TypeRef());
        if copy.is_null() {
            return s.to_string();
        }
        CFStringNormalize(copy, kCFStringNormalizationFormC);
        CFString::wrap_under_create_rule(copy as _).to_string()
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn decomposed_letters_are_composed() {
        assert_eq!(nfc("soldu\u{301}.jpg"), "sold\u{fa}.jpg");
        assert_eq!(nfc("Cafe\u{301}"), "Caf\u{e9}");
    }

    #[test]
    fn other_names_stay_as_they_are() {
        for s in [
            "plain.txt",
            "sold\u{fa}.jpg",
            "\u{65e5}\u{672c}\u{8a9e}",
            "\u{1f600}.png",
            "a\u{202f}PM.png",
        ] {
            assert_eq!(nfc(s), s);
        }
    }

    #[test]
    fn a_decomposed_file_is_listed_by_its_composed_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("soldu\u{301}.jpg");
        std::fs::write(&path, "x").unwrap();
        let meta = std::fs::symlink_metadata(&path).unwrap();
        assert_eq!(
            composed(dir.path(), "soldu\u{301}.jpg".into(), &meta),
            "sold\u{fa}.jpg"
        );
    }

    #[test]
    fn a_character_nfc_replaces_on_its_own_keeps_the_name_on_disk() {
        use std::os::unix::fs::MetadataExt;
        let dir = tempfile::tempdir().unwrap();
        // A compatibility ideograph, the angstrom and ohm signs and the Greek
        // question mark each become another character, and the disk opens the
        // file by either, but the Mac never held the other name.
        for c in ['\u{fa10}', '\u{212b}', '\u{2126}', '\u{37e}'] {
            let name = format!("{c}.txt");
            let path = dir.path().join(&name);
            std::fs::write(&path, "x").unwrap();
            let meta = std::fs::symlink_metadata(&path).unwrap();
            let replaced = nfc(&name);
            assert_ne!(replaced, name);
            let via = std::fs::symlink_metadata(dir.path().join(&replaced)).unwrap();
            assert_eq!(via.ino(), meta.ino(), "{name:?} opens as {replaced:?} too");
            assert_eq!(composed(dir.path(), name.clone(), &meta), name);
        }
    }

    #[test]
    fn decomposed_hangul_is_composed() {
        let dir = tempfile::tempdir().unwrap();
        let name = "\u{1112}\u{1161}\u{11ab}.txt";
        let path = dir.path().join(name);
        std::fs::write(&path, "x").unwrap();
        let meta = std::fs::symlink_metadata(&path).unwrap();
        assert_eq!(composed(dir.path(), name.into(), &meta), "\u{d55c}.txt");
    }

    #[test]
    fn a_composed_name_that_reaches_another_file_is_not_used() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("soldu\u{301}.jpg"), "x").unwrap();
        let other = dir.path().join("other.txt");
        std::fs::write(&other, "y").unwrap();
        // The entry being listed is `other.txt`, so the file the composed
        // spelling reaches is not it.
        let meta = std::fs::symlink_metadata(&other).unwrap();
        assert_eq!(
            composed(dir.path(), "soldu\u{301}.jpg".into(), &meta),
            "soldu\u{301}.jpg"
        );
    }

    #[test]
    fn a_composed_name_the_folder_does_not_have_is_not_used() {
        let listed = tempfile::tempdir().unwrap();
        let path = listed.path().join("soldu\u{301}.jpg");
        std::fs::write(&path, "x").unwrap();
        let meta = std::fs::symlink_metadata(&path).unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        assert_eq!(
            composed(elsewhere.path(), "soldu\u{301}.jpg".into(), &meta),
            "soldu\u{301}.jpg"
        );
    }
}
