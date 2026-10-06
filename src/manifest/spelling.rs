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
}
