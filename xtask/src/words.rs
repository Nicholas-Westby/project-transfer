//! Splits text into the words listed in `words.txt` and the phrases listed in
//! `phrases.txt`. A word is letters and digits with at least one letter
//! (`player1`, `blake3`, `4b96`), joined by single hyphens (`one-time`).
//! Words are lowercased so `Result` and `result` are one entry. A word that
//! mixes cases, such as `ActivityKind`, is a phrase (see `phrases.rs`).
//! Hashes, keys and other random-looking strings are kept on purpose:
//! spotting a committed secret is one reason for the lists.

use crate::phrases;
use std::collections::BTreeSet;

/// The characters a word is made of. Anything else ends it.
fn is_word_char(c: char) -> bool {
    c.is_alphabetic() || c.is_ascii_digit()
}

/// Endings kept after an apostrophe, so `doesn't` stays one word instead of
/// leaving `doesn` and `t` behind. Lifetimes and char literals don't match.
const CONTRACTIONS: [&str; 7] = ["t", "s", "re", "ve", "ll", "d", "m"];

/// The words and phrases of one or more files.
#[derive(Default)]
pub struct Found {
    words: BTreeSet<String>,
    phrases: BTreeSet<String>,
}

impl Found {
    /// Adds the words and phrases in `text`. `escapes` is for Rust and TOML,
    /// where `\n` in `"a\nb"` is a line break, not part of the word `nb`.
    pub fn read(&mut self, text: &str, escapes: bool) {
        let mut run = String::new();
        let chars: Vec<char> = text.chars().collect();
        // Inside a Rust raw string, `r#"..."#`, with its number of `#`s.
        // Backslashes there are plain, as in `r"C:\Users"`.
        let mut raw: Option<usize> = None;
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            i += 1;
            if escapes && c == '"' {
                raw = match raw {
                    Some(n) if chars[i..].iter().take(n).filter(|&&h| h == '#').count() == n => {
                        None
                    }
                    Some(n) => Some(n),
                    None => raw_opening(&chars[..i - 1]),
                };
            }
            if escapes && raw.is_none() && c == '\\' {
                self.take(&mut run);
                // `\\` is a backslash; `\n`, `\t`, `\u{..}` and so on are one
                // character whose letter belongs to no word.
                if chars
                    .get(i)
                    .is_some_and(|&next| next == '\\' || "nrtux0".contains(next))
                {
                    i += 1;
                }
            } else if is_word_char(c) || ((c == '-' || is_apostrophe(c)) && !run.is_empty()) {
                run.push(c);
            } else {
                self.take(&mut run);
            }
        }
        self.take(&mut run);
    }

    /// The words and the phrases. A word that is a phrase in lower case, as
    /// `coolthings` is next to `CoolThings`, is listed only as the phrase.
    pub fn lists(mut self) -> (BTreeSet<String>, BTreeSet<String>) {
        for phrase in &self.phrases {
            self.words.remove(&phrase.to_lowercase());
        }
        (self.words, self.phrases)
    }

    /// Turns a run of word characters, hyphens and apostrophes into words
    /// and phrases.
    fn take(&mut self, run: &mut String) {
        for piece in split_joiners(run) {
            self.add(piece.replace('\u{2019}', "'"));
        }
        run.clear();
    }

    /// Lists a phrase as written along with its parts, or else the piece
    /// itself. Plain numbers are left out.
    fn add(&mut self, piece: String) {
        let words = if phrases::is_phrase(&piece) {
            let parts = phrases::parts(&piece);
            self.phrases.insert(piece);
            parts
        } else {
            vec![piece]
        };
        let words = words
            .into_iter()
            .filter(|w| w.chars().any(char::is_alphabetic));
        self.words.extend(words.map(|w| w.to_lowercase()));
    }
}

/// The number of `#`s when the text before a `"` opens a raw string: an `r`
/// (or `br`) that starts a word, then any `#`s.
fn raw_opening(before: &[char]) -> Option<usize> {
    let hashes = before.iter().rev().take_while(|&&c| c == '#').count();
    let rest = &before[..before.len() - hashes];
    let (&r, rest) = rest.split_last()?;
    let rest = rest.strip_suffix(&['b']).unwrap_or(rest);
    let starts_word = rest.last().is_none_or(|&c| !is_word_char(c) && c != '_');
    (r == 'r' && starts_word).then_some(hashes)
}

fn is_apostrophe(c: char) -> bool {
    c == '\'' || c == '\u{2019}'
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The words and the phrases in `text`, each in order.
    fn read(text: &str, escapes: bool) -> (Vec<String>, Vec<String>) {
        let mut found = Found::default();
        found.read(text, escapes);
        let (words, phrases) = found.lists();
        (words.into_iter().collect(), phrases.into_iter().collect())
    }

    fn list(text: &str) -> Vec<String> {
        read(text, true).0
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
    fn drops_plain_numbers_only() {
        assert_eq!(list("2024 0.1.62 2026-10-06"), Vec::<String>::new());
        assert_eq!(
            list("4b96 #1f2937 fbfd86e 0xEE6B5D"),
            ["0xee6b5d", "1f2937", "4b96", "fbfd86e"]
        );
        assert_eq!(
            list("127ca604-f3cd-4b96-b52e-71c8d06fabe3"),
            ["127ca604-f3cd-4b96-b52e-71c8d06fabe3"]
        );
    }

    #[test]
    fn keeps_random_looking_keys_whole_as_phrases() {
        let (words, phrases) = read("aB3xQ9zK sk-proj-Xy7Qa9Lm aBxQzK iOS", true);
        assert_eq!(words, Vec::<String>::new());
        assert_eq!(phrases, ["aB3xQ9zK", "aBxQzK", "iOS", "sk-proj-Xy7Qa9Lm"]);
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
        assert_eq!(read(r"C:\Users\nick", false).0, ["c", "nick", "users"]);
    }

    #[test]
    fn lists_phrases_as_written_and_their_parts_as_words() {
        let (words, phrases) = read("Hello, WorldPeeps, it's a fine day in the world.", true);
        assert_eq!(
            words,
            [
                "a", "day", "fine", "hello", "in", "it's", "peeps", "the", "world"
            ]
        );
        assert_eq!(phrases, ["WorldPeeps"]);
        let (words, phrases) = read("Jane's one-time GardenPlanner-web macOS", true);
        assert_eq!(
            words,
            [
                "garden", "jane's", "mac", "one-time", "os", "planner", "web"
            ]
        );
        assert_eq!(phrases, ["GardenPlanner-web", "macOS"]);
    }

    #[test]
    fn a_word_that_is_also_a_phrase_is_listed_as_the_phrase() {
        let (words, phrases) = read("CoolThings are coolthings", true);
        assert_eq!(words, ["are", "cool", "things"]);
        assert_eq!(phrases, ["CoolThings"]);
        // The rule covers the lists as a whole, not one file at a time.
        let mut found = Found::default();
        found.read("CoolThings", true);
        found.read("coolthings", true);
        let (words, phrases) = found.lists();
        assert!(!words.contains("coolthings") && phrases.contains("CoolThings"));
    }

    #[test]
    fn reads_raw_strings_without_escapes() {
        assert_eq!(
            list(r#"r"C:\Users\nick" "\nedition""#),
            ["c", "edition", "nick", "r", "users"]
        );
        assert_eq!(
            list(r###"r#"a\tb"# br"\usr" for"\nx""###),
            ["a", "br", "for", "r", "tb", "usr", "x"]
        );
    }

    #[test]
    fn keeps_letters_outside_ascii() {
        assert_eq!(list("café naïve"), ["café", "naïve"]);
    }
}
