//! IP addresses, such as `192.168.1.20` or `fe80::1`. They are listed whole
//! among the phrases, so a commit that adds one shows it in
//! `phrase-changes.txt`: a real address from someone's network is as worth
//! catching as a committed key.

use std::net::{Ipv4Addr, Ipv6Addr};

/// Finds the addresses in `chars`, blanks them so their numbers and hex
/// groups don't also turn up as words, and returns each one as written.
pub fn take(chars: &mut [char]) -> Vec<String> {
    let mut found = take_kind(chars, |c| c.is_ascii_hexdigit() || c == ':', is_v6);
    found.extend(take_kind(chars, |c| c.is_ascii_digit() || c == '.', is_v4));
    found
}

/// Takes each run of `part` characters that `is_address` accepts, perhaps
/// without a `.` or `:` that ends a sentence, and that stands apart from the
/// words around it: `Vec2::new` and `v1.2.3.4` hold no address.
fn take_kind(
    chars: &mut [char],
    part: impl Fn(char) -> bool,
    is_address: fn(&str) -> bool,
) -> Vec<String> {
    let mut found = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let start = i;
        while i < chars.len() && part(chars[i]) {
            i += 1;
        }
        if i == start {
            i += 1;
            continue;
        }
        let text = |end: usize| chars[start..end].iter().collect::<String>();
        let mut end = i;
        // Not a `::`, which ends an address such as `fc00::`.
        let trailing =
            matches!(chars[end - 1], '.' | ':') && (end - start < 2 || chars[end - 2] != ':');
        if !is_address(&text(end)) && trailing {
            end -= 1;
        }
        let apart = |c: Option<&char>| c.is_none_or(|&c| !c.is_alphanumeric() && c != '_');
        let before = start.checked_sub(1).map(|j| &chars[j]);
        if is_address(&text(end)) && apart(before) && apart(chars.get(end)) {
            found.push(text(end));
            chars[start..end].fill(' ');
        }
    }
    found
}

/// Rust paths such as `cafe::babe` parse too, so an address needs a digit.
fn is_v6(text: &str) -> bool {
    text.contains(|c: char| c.is_ascii_digit())
        && (text.contains("::") || text.matches(':').count() == 7)
        && text.parse::<Ipv6Addr>().is_ok()
}

fn is_v4(text: &str) -> bool {
    text.parse::<Ipv4Addr>().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn taken(text: &str) -> (Vec<String>, String) {
        let mut chars: Vec<char> = text.chars().collect();
        let found = take(&mut chars);
        (found, chars.into_iter().collect())
    }

    #[test]
    fn takes_addresses_as_written() {
        let (found, rest) =
            taken("Dial 192.168.1.20:47820, [fd00::5]:47820 or ::1. Not fc00::/7 or 127.0.0.1.");
        assert_eq!(
            found,
            ["fd00::5", "::1", "fc00::", "192.168.1.20", "127.0.0.1"]
        );
        assert!(!rest.contains("fd00") && !rest.contains("192"), "{rest}");
        assert!(rest.contains(":47820"), "a port is no part of the address");
    }

    #[test]
    fn takes_a_full_v6_address() {
        let (found, _) = taken("fe80::4721:9d4c:2f7a:b09c and 2001:db8:0:0:0:0:0:1");
        assert_eq!(found, ["fe80::4721:9d4c:2f7a:b09c", "2001:db8:0:0:0:0:0:1"]);
    }

    #[test]
    fn versions_paths_and_times_are_no_addresses() {
        let text = "0.1.78 1.2.3.4.5 v1.2.3.4 300.1.1.1 1.2.3.4a Vec2::new u32::from \
                    f32::MAX blake3::hash Sha256::digest cafe::babe 12:30:45 std::fs ::";
        let (found, rest) = taken(text);
        assert_eq!(found, Vec::<String>::new());
        assert_eq!(rest, text);
    }
}
