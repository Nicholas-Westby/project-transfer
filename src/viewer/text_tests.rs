use super::*;

fn text(body: &Body) -> &Text {
    match body {
        Body::Text(t) => t,
        Body::Binary => panic!("read as binary"),
    }
}

#[test]
fn lines_lose_their_endings_and_a_final_newline_adds_no_line() {
    let b = decode(b"one\r\ntwo\r\n", 10);
    let t = text(&b);
    assert_eq!(t.lines(), ["one", "two"]);
    assert_eq!(t.endings, Endings::Crlf);
    let b = decode(b"one\ntwo", 7);
    assert_eq!(text(&b).lines(), ["one", "two"]);
    assert_eq!(text(&b).endings, Endings::Lf);
}

#[test]
fn endings_of_both_kinds_are_mixed_and_one_line_has_none() {
    assert_eq!(text(&decode(b"a\r\nb\nc", 7)).endings, Endings::Mixed);
    assert_eq!(text(&decode(b"only", 4)).endings, Endings::None);
    assert!(text(&decode(b"", 0)).is_empty());
}

#[test]
fn a_nul_byte_or_bytes_that_are_not_utf8_are_binary() {
    assert_eq!(decode(b"PK\x03\x04\x00\x00", 6), Body::Binary);
    assert_eq!(decode(b"caf\xe9 au lait", 12), Body::Binary);
}

#[test]
fn a_character_cut_where_the_read_stopped_is_still_text() {
    // "é" is two bytes; the read stopped after the first.
    let b = decode(b"caf\xc3", 5_000);
    assert_eq!(text(&b).lines(), ["caf"]);
    // The whole file was read, so the cut is in the file itself.
    assert_eq!(decode(b"caf\xc3", 4), Body::Binary);
}

#[test]
fn a_byte_order_mark_is_not_shown() {
    let b = decode("\u{feff}title".as_bytes(), 8);
    assert_eq!(text(&b).lines(), ["title"]);
}

#[test]
fn tabs_widen_to_the_next_stop_of_four() {
    assert_eq!(expand_tabs("\tx"), "    x");
    assert_eq!(expand_tabs("ab\tc"), "ab  c");
    assert_eq!(expand_tabs("abcd\te"), "abcd    e");
    assert!(matches!(expand_tabs("plain"), Cow::Borrowed(_)));
    assert_eq!(text(&decode(b"ab\tc\nx", 6)).widest, 5);
}
