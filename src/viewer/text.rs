//! The start of a file read as text for the viewer: lines without their
//! endings, or a note that it is not text.

use std::borrow::Cow;
use std::ops::Range;

/// Columns a tab moves to the next multiple of.
const TAB: usize = 4;

/// How a copy's lines end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Endings {
    /// One line with no ending.
    None,
    Lf,
    Crlf,
    /// Some lines end one way and some the other.
    Mixed,
}

impl Endings {
    /// For a note such as "CRLF here, LF on Desk".
    pub fn name(self) -> &'static str {
        match self {
            Endings::None => "none",
            Endings::Lf => "LF",
            Endings::Crlf => "CRLF",
            Endings::Mixed => "mixed",
        }
    }
}

/// A copy's text, split into lines.
#[derive(Clone, Debug, PartialEq)]
pub struct Text {
    body: String,
    /// Where each line sits in `body`, without its ending.
    lines: Vec<Range<usize>>,
    pub endings: Endings,
    /// Columns in the longest line once tabs are spaces, which sets how far
    /// the panes scroll sideways.
    pub widest: usize,
}

impl Text {
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Line `i`, as in the file but without its ending.
    pub fn line(&self, i: usize) -> &str {
        &self.body[self.lines[i].clone()]
    }

    /// Every line, for lining two copies up.
    pub fn lines(&self) -> Vec<&str> {
        (0..self.len()).map(|i| self.line(i)).collect()
    }
}

/// What a copy holds, as far as the viewer can tell.
#[derive(Clone, Debug, PartialEq)]
pub enum Body {
    Text(Text),
    /// It holds a NUL byte, or bytes that are not UTF-8.
    Binary,
}

/// Reads `bytes`, the start of a file `size` bytes long. A character cut
/// in two where the read stopped is dropped rather than taken for binary.
pub fn decode(bytes: &[u8], size: u64) -> Body {
    if bytes.contains(&0) {
        return Body::Binary;
    }
    let cut = size > bytes.len() as u64;
    let text = match std::str::from_utf8(bytes) {
        Ok(t) => t,
        // `error_len` is None only for a sequence the input ends inside.
        Err(e) if cut && e.error_len().is_none() => {
            std::str::from_utf8(&bytes[..e.valid_up_to()]).unwrap_or_default()
        }
        Err(_) => return Body::Binary,
    };
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    Body::Text(split(text))
}

fn split(text: &str) -> Text {
    let mut lines = Vec::new();
    let (mut lf, mut crlf) = (false, false);
    let mut start = 0;
    for (i, _) in text.match_indices('\n') {
        let end = if i > start && text.as_bytes()[i - 1] == b'\r' {
            crlf = true;
            i - 1
        } else {
            lf = true;
            i
        };
        lines.push(start..end);
        start = i + 1;
    }
    if start < text.len() {
        lines.push(start..text.len());
    }
    let endings = match (lf, crlf) {
        (false, false) => Endings::None,
        (true, false) => Endings::Lf,
        (false, true) => Endings::Crlf,
        (true, true) => Endings::Mixed,
    };
    let widest = lines
        .iter()
        .map(|r| columns(&text[r.clone()]))
        .max()
        .unwrap_or(0);
    Text {
        body: text.to_string(),
        lines,
        endings,
        widest,
    }
}

/// How wide `line` is once its tabs are spaces.
fn columns(line: &str) -> usize {
    line.chars().fold(0, |col, c| match c {
        '\t' => col + TAB - col % TAB,
        _ => col + 1,
    })
}

/// `line` with each tab widened to the next multiple of four columns.
pub fn expand_tabs(line: &str) -> Cow<'_, str> {
    if !line.contains('\t') {
        return Cow::Borrowed(line);
    }
    let mut out = String::with_capacity(line.len() + TAB);
    let mut col = 0;
    for c in line.chars() {
        if c == '\t' {
            let n = TAB - col % TAB;
            out.extend(std::iter::repeat_n(' ', n));
            col += n;
        } else {
            out.push(c);
            col += 1;
        }
    }
    Cow::Owned(out)
}

#[cfg(test)]
#[path = "text_tests.rs"]
mod tests;
