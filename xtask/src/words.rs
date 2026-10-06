//! Splits text into the words listed in `words.txt`. A word starts with a
//! letter and is made of letters and digits (`player1`, `blake3`), joined by
//! single hyphens (`one-time`). Everything is lowercased so `Result` and
//! `result` are one entry.

use std::collections::BTreeSet;

/// The characters a word is made of. Anything else ends it.
fn is_word_char(c: char) -> bool {
    c.is_alphabetic() || c.is_ascii_digit()
}

/// Endings kept after an apostrophe, so `doesn't` stays one word instead of
/// leaving `doesn` and `t` behind. Lifetimes and char literals don't match.
const CONTRACTIONS: [&str; 7] = ["t", "s", "re", "ve", "ll", "d", "m"];

/// Every word in `text`. `escapes` is for Rust and TOML, where `\n` in
/// `"a\nb"` is a line break, not part of the word `nb`.
pub fn words_in(text: &str, escapes: bool) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut run = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if escapes && c == '\\' {
            take(&mut run, &mut found);
            // `\\` is a backslash; `\n`, `\t`, `\u{..}` and so on are one
            // character whose letter belongs to no word.
            if let Some(&next) = chars.peek()
                && (next == '\\' || "nrtux0".contains(next))
            {
                chars.next();
            }
        } else if is_word_char(c) || ((c == '-' || is_apostrophe(c)) && !run.is_empty()) {
            run.push(c);
        } else {
            take(&mut run, &mut found);
        }
    }
    take(&mut run, &mut found);
    found
}

fn is_apostrophe(c: char) -> bool {
    c == '\'' || c == '\u{2019}'
}

/// Turns a run of word characters, hyphens and apostrophes into words.
fn take(run: &mut String, found: &mut BTreeSet<String>) {
    for piece in split_joiners(run) {
        found.extend(clean(&piece));
    }
    run.clear();
}

/// Splits a run where a hyphen or apostrophe isn't joining two words:
/// `--dest` gives `dest`, `it's` stays whole, `'a` gives `a`.
fn split_joiners(run: &str) -> Vec<String> {
    let mut pieces = vec![String::new()];
    let parts: Vec<&str> = run.split('-').collect();
    for (i, part) in parts.iter().enumerate() {
        let joined = i > 0 && !part.is_empty() && !parts[i - 1].is_empty();
        let current = pieces.last_mut().expect("pieces is never empty");
        if !joined && !current.is_empty() {
            pieces.push(String::new());
        }
        for (j, bit) in split_apostrophes(part).into_iter().enumerate() {
            let current = pieces.last_mut().expect("pieces is never empty");
            if j > 0 {
                pieces.push(bit);
            } else {
                if joined && !current.is_empty() {
                    current.push('-');
                }
                current.push_str(&bit);
            }
        }
    }
    pieces.retain(|p| !p.is_empty());
    pieces
}

/// `doesn't` stays whole; any other apostrophe splits.
fn split_apostrophes(part: &str) -> Vec<String> {
    let bits: Vec<&str> = part.split(is_apostrophe).collect();
    let mut out = vec![bits[0].to_string()];
    for bit in &bits[1..] {
        let last = out.last_mut().expect("out is never empty");
        let lower = bit.to_lowercase();
        if !last.is_empty() && CONTRACTIONS.contains(&lower.as_str()) {
            last.push('\'');
            last.push_str(bit);
        } else {
            out.push(bit.to_string());
        }
    }
    out
}

/// The words to list from one piece; none for numbers and hashes.
fn clean(piece: &str) -> Vec<String> {
    let piece = piece.replace('\u{2019}', "'");
    if looks_like_hash(&piece) {
        return Vec::new();
    }
    split_camel_case(&piece)
        .into_iter()
        .filter(|w| w.chars().next().is_some_and(char::is_alphabetic))
        .map(|w| w.to_lowercase())
        .collect()
}

/// `ClientCertVerifier` gives `Client`, `Cert` and `Verifier`, and
/// `TLSConfig` gives `TLS` and `Config`, so a misspelt part stands out
/// instead of hiding in one long word. Digits stay with the letters before
/// them (`Color32`). A piece with no capitals inside comes back whole.
fn split_camel_case(piece: &str) -> Vec<String> {
    let chars: Vec<char> = piece.chars().collect();
    let starts_word = |i: usize| {
        let (prev, c) = (chars[i - 1], chars[i]);
        let next_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
        c.is_uppercase()
            && (prev.is_lowercase() || prev.is_ascii_digit() || (prev.is_uppercase() && next_lower))
    };
    if !(1..chars.len()).any(starts_word) {
        return vec![piece.to_string()];
    }
    let mut words = vec![String::new()];
    for (i, &c) in chars.iter().enumerate() {
        if c == '-' || (i > 0 && starts_word(i)) {
            words.push(String::new());
        }
        if c != '-' {
            words.last_mut().expect("words is never empty").push(c);
        }
    }
    words.retain(|w| !w.is_empty());
    words
}

/// Commit ids, UUIDs and colors such as `fbfd86e` or `e5e7eb` would fill the
/// list with noise. Hex that switches between letters and digits more than
/// once is taken as a hash; `ed25519` and `b3` switch once and stay.
fn looks_like_hash(word: &str) -> bool {
    let hex: Vec<char> = word.chars().filter(|&c| c != '-').collect();
    if !hex.iter().all(char::is_ascii_hexdigit) {
        return false;
    }
    let switches = hex
        .windows(2)
        .filter(|w| w[0].is_ascii_digit() != w[1].is_ascii_digit())
        .count();
    switches >= 2
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(text: &str) -> Vec<String> {
        words_in(text, true).into_iter().collect()
    }

    #[test]
    fn lowercases_and_sorts() {
        assert_eq!(list("Zebra apple Apple"), ["apple", "zebra"]);
    }

    #[test]
    fn keeps_hyphenated_words_whole() {
        assert_eq!(list("a one-time thing"), ["a", "one-time", "thing"]);
        assert_eq!(
            list("--dest -x a--b trailing-"),
            ["a", "b", "dest", "trailing", "x"]
        );
    }

    #[test]
    fn keeps_digits_inside_words() {
        assert_eq!(
            list("player1 blake3 u64 x86_64"),
            ["blake3", "player1", "u64", "x86"]
        );
    }

    #[test]
    fn drops_numbers_and_hashes() {
        assert_eq!(list("2024 0.1.62 4b96 #1f2937"), Vec::<String>::new());
        assert_eq!(list("fbfd86e e5e7eb a1b2c3d4-e5f6"), Vec::<String>::new());
        assert_eq!(
            list("127ca604-f3cd-4b96-b52e-71c8d06fabe3"),
            Vec::<String>::new()
        );
        assert_eq!(list("ed25519 b3 face"), ["b3", "ed25519", "face"]);
    }

    #[test]
    fn splits_snake_case_and_paths() {
        assert_eq!(
            list("default_root src/ui/theme.rs"),
            ["default", "root", "rs", "src", "theme", "ui"]
        );
    }

    #[test]
    fn handles_apostrophes() {
        assert_eq!(
            list("doesn't it’s 'static b'x'"),
            ["b", "doesn't", "it's", "static", "x"]
        );
    }

    #[test]
    fn skips_escape_letters_only_where_asked() {
        assert_eq!(
            list(r#""a\nedition\tb \\target \u{2192}""#),
            ["a", "b", "edition", "target"]
        );
        let plain: Vec<String> = words_in(r"C:\Users\nick", false).into_iter().collect();
        assert_eq!(plain, ["c", "nick", "users"]);
    }

    #[test]
    fn splits_camel_case() {
        assert_eq!(
            list("ClientCertVerifier TLSConfig Color32Image Vec2 macOS"),
            [
                "cert", "client", "color32", "config", "image", "mac", "os", "tls", "vec2",
                "verifier"
            ]
        );
        assert_eq!(
            list("Jane's one-time GardenPlanner-web"),
            ["garden", "jane's", "one-time", "planner", "web"]
        );
    }

    #[test]
    fn keeps_letters_outside_ascii() {
        assert_eq!(list("café naïve"), ["café", "naïve"]);
    }
}
