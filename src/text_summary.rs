//! Summaries of LF rows for any byte range, in O(log N).

use std::ops::{Bound, RangeBounds};

use crate::{
    end_bound_to_num,
    slice::SliceInner,
    start_bound_to_num,
    tree::{Node, NodeInfo},
    Error::*,
    Result, Rope, RopeSlice,
};

/// Lengths and row facts of a piece of text.
///
/// Rows are split by LF only; CR is an ordinary char.  This carries the same
/// facts as Zed's `TextSummary`.
#[derive(Debug, Copy, Clone, Default, PartialEq, Eq, Hash)]
pub struct TextSummary {
    /// Length in bytes.
    pub bytes: usize,
    /// Length in `char`s.
    pub chars: usize,
    /// Length in UTF-16 code units.
    pub utf16: usize,
    /// Number of LFs, which is also the index of the last row.
    pub line_breaks: usize,
    /// Bytes in the last row.
    pub last_line_bytes: usize,
    /// Chars in the first row.
    pub first_line_chars: usize,
    /// Chars in the last row.
    pub last_line_chars: usize,
    /// UTF-16 code units in the last row.
    pub last_line_utf16: usize,
    /// Index of the first row with the most chars.
    pub longest_row: usize,
    /// Chars in that row.
    pub longest_row_chars: usize,
}

impl From<NodeInfo> for TextSummary {
    #[inline]
    fn from(info: NodeInfo) -> Self {
        let c = info.counts;
        let s = info.line_summary;
        TextSummary {
            bytes: c.bytes,
            chars: c.chars,
            utf16: c.utf16,
            line_breaks: c.line_breaks_lf,
            last_line_bytes: s.last_line_bytes as usize,
            first_line_chars: s.first_line_chars as usize,
            last_line_chars: s.last_line_chars as usize,
            last_line_utf16: s.last_line_utf16 as usize,
            longest_row: s.longest_row as usize,
            longest_row_chars: s.longest_row_chars as usize,
        }
    }
}

/// Text info of bytes `start..end` of `node`.  `start < end` must hold.
///
/// Whole children are taken from their stored info, so only the leaves at
/// the two ends of the range are scanned.  Leaves always start and end on
/// char boundaries, so the range ends are checked there, in those two
/// leaves.  Returns `None` if one of them is not a char boundary.
fn info_in_range(node: &Node, start: usize, end: usize) -> Option<NodeInfo> {
    match node {
        Node::Leaf(text) => text.text().get(start..end).map(NodeInfo::from_str),
        Node::Internal(children) => {
            let mut acc = NodeInfo::new();
            let mut child_start = 0;
            for (i, info) in children.info().iter().enumerate() {
                let child_end = child_start + info.bytes;
                if child_end > start {
                    let s = start.max(child_start) - child_start;
                    let e = end.min(child_end) - child_start;
                    acc += if s == 0 && e == info.bytes {
                        children.node_info(i)
                    } else {
                        info_in_range(&children.nodes()[i], s, e)?
                    };
                }
                if child_end >= end {
                    break;
                }
                child_start = child_end;
            }
            Some(acc)
        }
    }
}

fn to_range(byte_range: (Bound<&usize>, Bound<&usize>), len: usize) -> [usize; 2] {
    let start = start_bound_to_num(byte_range.0).unwrap_or(0);
    let end = end_bound_to_num(byte_range.1).unwrap_or(len);
    [start, end]
}

impl Rope {
    /// Returns the [`TextSummary`] of the given byte range.
    ///
    /// Runs in O(log N) time.  The whole rope is answered in O(1).
    ///
    /// # Panics
    ///
    /// - If the start of the range is greater than the end.
    /// - If the end of the range is out of bounds (i.e. `end > len()`).
    /// - If the range ends are not on char boundaries.
    #[track_caller]
    #[inline]
    pub fn text_summary<R: RangeBounds<usize>>(&self, byte_range: R) -> TextSummary {
        match self.try_text_summary(byte_range) {
            Ok(summary) => summary,
            Err(e) => panic!("{}", e),
        }
    }

    /// Non-panicking version of [`text_summary()`](Rope::text_summary).
    pub fn try_text_summary<R: RangeBounds<usize>>(&self, byte_range: R) -> Result<TextSummary> {
        let [start, end] = to_range((byte_range.start_bound(), byte_range.end_bound()), self.len());
        check_range(start, end, self.len(), |i| self.is_char_boundary(i))?;
        if start == end {
            return Ok(TextSummary::default());
        }

        let [start, end] = [self.byte_range[0] + start, self.byte_range[0] + end];
        if start == 0 && end == self.root_info.counts.bytes {
            return Ok(self.root_info.into());
        }
        info_in_range(&self.root, start, end)
            .map(Into::into)
            .ok_or(NonCharBoundary)
    }
}

impl RopeSlice<'_> {
    /// Returns the [`TextSummary`] of the given byte range of the slice.
    ///
    /// Runs in O(log N) time.
    ///
    /// # Panics
    ///
    /// - If the start of the range is greater than the end.
    /// - If the end of the range is out of bounds (i.e. `end > len()`).
    /// - If the range ends are not on char boundaries.
    #[track_caller]
    #[inline]
    pub fn text_summary<R: RangeBounds<usize>>(&self, byte_range: R) -> TextSummary {
        match self.try_text_summary(byte_range) {
            Ok(summary) => summary,
            Err(e) => panic!("{}", e),
        }
    }

    /// Non-panicking version of [`text_summary()`](RopeSlice::text_summary).
    pub fn try_text_summary<R: RangeBounds<usize>>(&self, byte_range: R) -> Result<TextSummary> {
        let [start, end] = to_range((byte_range.start_bound(), byte_range.end_bound()), self.len());
        check_range(start, end, self.len(), |i| self.is_char_boundary(i))?;
        if start == end {
            return Ok(TextSummary::default());
        }

        let info = match self.0 {
            SliceInner::Rope {
                root, byte_range, ..
            } => info_in_range(root, byte_range[0] + start, byte_range[0] + end),
            SliceInner::Str(text) => text.get(start..end).map(NodeInfo::from_str),
        };
        info.map(Into::into).ok_or(NonCharBoundary)
    }
}

/// Checks the range ends.  For a non-empty range, the char boundaries are
/// checked later, in `info_in_range`.
fn check_range(
    start: usize,
    end: usize,
    len: usize,
    is_char_boundary: impl Fn(usize) -> bool,
) -> Result<()> {
    if start > end {
        return Err(InvalidRange);
    }
    if end > len {
        return Err(OutOfBounds);
    }
    if start == end && !is_char_boundary(start) {
        return Err(NonCharBoundary);
    }
    Ok(())
}

//=============================================================

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "Hello there!\nHow're you doing?\r\nこんにちは、みんなさん！\n\
                        🐸🐸 frogs 👨‍👩‍👧‍👦\n\n\tA much longer row than the others, with tabs\t\t.\n\
                        end";

    fn naive(text: &str) -> TextSummary {
        let mut s = TextSummary {
            bytes: text.len(),
            ..Default::default()
        };
        for c in text.chars() {
            s.chars += 1;
            s.utf16 += c.len_utf16();
            if c == '\n' {
                s.line_breaks += 1;
                s.last_line_bytes = 0;
                s.last_line_chars = 0;
                s.last_line_utf16 = 0;
            } else {
                s.last_line_bytes += c.len_utf8();
                s.last_line_chars += 1;
                s.last_line_utf16 += c.len_utf16();
            }
            if s.line_breaks == 0 {
                s.first_line_chars = s.last_line_chars;
            }
            if s.last_line_chars > s.longest_row_chars {
                s.longest_row = s.line_breaks;
                s.longest_row_chars = s.last_line_chars;
            }
        }
        s
    }

    #[test]
    fn text_summary_all_ranges() {
        let r = Rope::from_str(TEXT);
        let bounds: Vec<usize> = (0..=TEXT.len()).filter(|&i| TEXT.is_char_boundary(i)).collect();
        for &a in &bounds {
            for &b in bounds.iter().filter(|&&b| b >= a) {
                assert_eq!(r.text_summary(a..b), naive(&TEXT[a..b]), "{a}..{b}");
            }
        }
        r.assert_invariants();
    }

    #[test]
    fn slice_text_summary() {
        let floor = |t: &str, mut i: usize| {
            while !t.is_char_boundary(i) {
                i -= 1;
            }
            i
        };
        let r = Rope::from_str(TEXT);
        let (a, b) = (floor(TEXT, 5), floor(TEXT, 90));
        let s = r.slice(a..b);
        let t = &TEXT[a..b];
        let (c, d) = (floor(t, 3), floor(t, 40));
        assert_eq!(s.text_summary(..), naive(t));
        assert_eq!(s.text_summary(c..d), naive(&t[c..d]));
        let s: RopeSlice = t.into();
        assert_eq!(s.text_summary(c..d), naive(&t[c..d]));
    }

    #[test]
    fn text_summary_errors() {
        let r = Rope::from_str("こんにちは");
        assert_eq!(r.try_text_summary(1..3), Err(NonCharBoundary));
        assert_eq!(r.try_text_summary(3..16), Err(OutOfBounds));
        #[allow(clippy::reversed_empty_ranges)]
        let rev = 6..3;
        assert_eq!(r.try_text_summary(rev), Err(InvalidRange));

        // Many leaves, so the bad ends land in different leaves.
        let text = "こんにちは🐸".repeat(20);
        let r = Rope::from_str(&text);
        let len = text.len();
        for i in (0..len).filter(|&i| !text.is_char_boundary(i)) {
            assert_eq!(r.try_text_summary(i..len), Err(NonCharBoundary));
            assert_eq!(r.try_text_summary(0..i), Err(NonCharBoundary));
            assert_eq!(r.try_text_summary(i..i), Err(NonCharBoundary));
            assert_eq!(r.slice(..).try_text_summary(0..i), Err(NonCharBoundary));
            assert_eq!(r.slice(..).try_text_summary(i..i), Err(NonCharBoundary));
        }
    }
}
