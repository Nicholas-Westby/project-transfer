//! Phrases are words that mix cases, with a capital inside that starts a new
//! part, such as `ActivityKind`, `TLSConfig` or `macOS` (not `TLS`).
//! `phrases.txt` lists each one as written and `words.txt` lists its parts in
//! lower case (`activity`, `kind`), so a misspelt part stands out among the
//! words while the name itself stays on record.

/// A word with lower case letters and a capital inside it that starts a new
/// part. `Hello`, `TLS` and `4B96` are not phrases, and neither are hex
/// numbers such as `0xEE6B5D`, which only look mixed-case.
pub fn is_phrase(word: &str) -> bool {
    let hex_number = word
        .strip_prefix("0x")
        .is_some_and(|digits| digits.chars().all(|c| c.is_ascii_hexdigit()));
    let chars: Vec<char> = word.chars().collect();
    !hex_number
        && chars.iter().any(|c| c.is_lowercase())
        && (1..chars.len()).any(|i| starts_part(&chars, i))
}

/// A capital after a lower case letter or a digit, or the last capital of a
/// run when a lower case letter follows, as the `C` of `TLSConfig`.
fn starts_part(chars: &[char], i: usize) -> bool {
    let (prev, c) = (chars[i - 1], chars[i]);
    let next_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
    c.is_uppercase()
        && (prev.is_lowercase() || prev.is_ascii_digit() || (prev.is_uppercase() && next_lower))
}

/// The parts of a phrase, as written: `ClientCertVerifier` gives `Client`,
/// `Cert` and `Verifier`. Digits stay with the letters before them
/// (`Color32`) and hyphens split. A key or other random-looking phrase gives
/// none, as its scraps would only be noise among the words.
pub fn parts(phrase: &str) -> Vec<String> {
    let chars: Vec<char> = phrase.chars().collect();
    let mut parts = vec![String::new()];
    for (i, &c) in chars.iter().enumerate() {
        if c == '-' || (i > 0 && starts_part(&chars, i)) {
            parts.push(String::new());
        }
        if c != '-' {
            parts.last_mut().expect("parts is never empty").push(c);
        }
    }
    parts.retain(|p| !p.is_empty());
    if looks_random(&parts) {
        return Vec::new();
    }
    parts
}

/// A key such as `aB3xQ9zK` splits into scraps (`B3x`, `K`). Real names
/// split into parts of letters with at most trailing digits, and most have
/// three letters or more.
fn looks_random(parts: &[String]) -> bool {
    let odd_digits = parts
        .iter()
        .any(|p| letters(p).chars().any(|c| c.is_ascii_digit()));
    let short = parts
        .iter()
        .filter(|p| letters(p).chars().count() <= 2)
        .count();
    odd_digits || short * 2 > parts.len()
}

/// A part without its trailing digits: `Color32` gives `Color`.
fn letters(part: &str) -> &str {
    part.trim_end_matches(|c: char| c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_phrase_mixes_cases_with_a_capital_inside() {
        for phrase in [
            "ActivityKind",
            "macOS",
            "iOS",
            "aB3xQ9zK",
            "GardenPlanner-web",
        ] {
            assert!(is_phrase(phrase), "{phrase} is a phrase");
        }
        let words = [
            "Hello",
            "TLS",
            "4B96",
            "Vec2",
            "Content-Type",
            "one-time",
            "0xEE6B5D",
            "0xFF",
        ];
        for word in words {
            assert!(!is_phrase(word), "{word} is not a phrase");
        }
    }

    #[test]
    fn splits_at_capitals_and_hyphens() {
        assert_eq!(parts("ClientCertVerifier"), ["Client", "Cert", "Verifier"]);
        assert_eq!(parts("TLSConfig"), ["TLS", "Config"]);
        assert_eq!(parts("Color32Image"), ["Color32", "Image"]);
        assert_eq!(parts("macOS"), ["mac", "OS"]);
        assert_eq!(parts("GardenPlanner-web"), ["Garden", "Planner", "web"]);
    }

    #[test]
    fn random_looking_phrases_have_no_parts() {
        for key in ["aB3xQ9zK", "sk-proj-Xy7Qa9Lm", "aBxQzK", "iOS"] {
            assert!(parts(key).is_empty(), "{key} has no parts");
        }
    }
}
