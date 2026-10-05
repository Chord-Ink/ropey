//! Per-node line summary for LF rows.
//!
//! Together with the byte, char, UTF-16 and LF counts of `TextInfo`, it holds
//! everything needed to describe a text's rows: the first and last row
//! lengths and the longest row.  Two summaries combine with `concat()` as if
//! their texts were joined, which is why a parent's summary is always folded
//! from its children and never updated by subtraction.
//!
//! Only LF starts a row.  CR is an ordinary char here, as in Zed's
//! `TextSummary`.
//!
//! Row lengths and row indices are u32, as in Zed.  A value that does not
//! fit panics in every build, release included, instead of wrapping.

use str_indices::{chars, lines_lf, utf16};

use super::TextInfo;
use crate::str_utils;

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash, Default)]
pub(crate) struct LineSummary {
    /// Bytes in the last row.
    pub last_line_bytes: u32,
    /// Chars in the first row.
    pub first_line_chars: u32,
    /// Chars in the last row.
    pub last_line_chars: u32,
    /// UTF-16 code units in the last row.
    pub last_line_utf16: u32,
    /// Index of the first row with the most chars.
    pub longest_row: u32,
    /// Chars in that row.
    pub longest_row_chars: u32,
}

impl LineSummary {
    #[inline(always)]
    pub(crate) fn new() -> Self {
        LineSummary {
            last_line_bytes: 0,
            first_line_chars: 0,
            last_line_chars: 0,
            last_line_utf16: 0,
            longest_row: 0,
            longest_row_chars: 0,
        }
    }

    /// Scans `text`, whose total char and UTF-16 counts are `char_count` and
    /// `utf16_count`.  Returns the summary and the number of LFs.
    ///
    /// Reads eight bytes at a time and marks LFs and utf8 continuation bytes
    /// with plain u64 math.  A row's char count is its byte count minus the
    /// continuation bytes in it.
    pub(crate) fn from_str(text: &str, char_count: usize, utf16_count: usize) -> (Self, usize) {
        let bytes = text.as_bytes();
        let all_ascii = char_count == bytes.len();

        let mut rows = 0;
        let mut first_line_chars = 0;
        let mut longest_row = 0;
        let mut longest_row_chars = 0;
        let mut row_start = 0;
        let mut row_start_cont = 0;
        let mut cont_before_word = 0;

        let mut word_start = 0;
        let words = bytes.chunks_exact(8);
        let tail = {
            let rest = words.remainder();
            let mut buf = [0u8; 8];
            buf[..rest.len()].copy_from_slice(rest);
            u64::from_le_bytes(buf)
        };
        for word in words.map(|w| u64::from_le_bytes(w.try_into().unwrap())).chain([tail]) {
            let cont = if all_ascii { 0 } else { cont_bits(word) };

            let mut lfs = lf_bits(word);
            if lfs == 0 {
                cont_before_word += cont.count_ones() as usize;
                word_start += 8;
                continue;
            }
            while lfs != 0 {
                let bit = lfs.trailing_zeros() as usize;
                let pos = word_start + bit / 8;
                let cont_at = cont_before_word + (cont & ((1u64 << bit) - 1)).count_ones() as usize;
                let chars = (pos - row_start) - (cont_at - row_start_cont);
                if rows == 0 {
                    first_line_chars = chars;
                    longest_row_chars = chars;
                } else if chars > longest_row_chars {
                    longest_row = rows;
                    longest_row_chars = chars;
                }
                rows += 1;
                row_start = pos + 1;
                row_start_cont = cont_at;
                lfs &= lfs - 1;
            }

            cont_before_word += cont.count_ones() as usize;
            word_start += 8;
        }

        let last_line_bytes = bytes.len() - row_start;
        let last_line_chars = last_line_bytes - (cont_before_word - row_start_cont);
        if rows == 0 {
            first_line_chars = last_line_chars;
            longest_row_chars = last_line_chars;
        } else if last_line_chars > longest_row_chars {
            longest_row = rows;
            longest_row_chars = last_line_chars;
        }
        let last_line_utf16 = if all_ascii {
            last_line_bytes
        } else if rows == 0 {
            utf16_count
        } else {
            last_line_chars + utf16::count_surrogates(&text[row_start..])
        };

        // No value is larger than the byte count, so one check covers all.
        to_u32(bytes.len());
        let summary = LineSummary {
            last_line_bytes: last_line_bytes as u32,
            first_line_chars: first_line_chars as u32,
            last_line_chars: last_line_chars as u32,
            last_line_utf16: last_line_utf16 as u32,
            longest_row: longest_row as u32,
            longest_row_chars: longest_row_chars as u32,
        };
        (summary, rows)
    }

    /// The summary of `text` after inserted text with no LF is put at
    /// `byte_idx`.  `self` is the summary of `text`, which has `rows` LFs.
    /// `added` is the byte, char and UTF-16 count of the inserted text.
    ///
    /// Only the row around the insert grows, so only that row is looked at.
    /// It becomes the longest row when it is now longer, or as long and
    /// earlier, which keeps the first longest row as `concat()` does.
    pub(crate) fn insert_in_row(
        self,
        text: &str,
        byte_idx: usize,
        rows: usize,
        all_ascii: bool,
        added: [usize; 3],
    ) -> LineSummary {
        let [added_bytes, added_chars, added_utf16] = added;
        let bytes = text.as_bytes();
        let row_start = str_utils::rfind_lf(&bytes[..byte_idx]).map(|i| i + 1);
        let row_end = str_utils::find_lf(&bytes[byte_idx..]).map(|i| byte_idx + i);

        let mut s = self;
        if row_start.is_none() {
            s.first_line_chars = add(s.first_line_chars, added_chars);
        }
        if row_end.is_none() {
            s.last_line_bytes = add(s.last_line_bytes, added_bytes);
            s.last_line_chars = add(s.last_line_chars, added_chars);
            s.last_line_utf16 = add(s.last_line_utf16, added_utf16);
        }

        let (row, row_chars) = match (row_start, row_end) {
            (None, _) => (0, s.first_line_chars),
            (Some(_), None) => (rows, s.last_line_chars),
            (Some(start), Some(end)) => {
                // A row has no more chars than bytes.
                let longest = s.longest_row_chars as usize;
                if end - start + added_chars < longest {
                    return s;
                }
                let old_chars = if all_ascii {
                    end - start
                } else {
                    chars::count(&text[start..end])
                };
                if old_chars + added_chars < longest {
                    return s;
                }
                let row = if start <= text.len() / 2 {
                    lines_lf::count_breaks(&text[..start])
                } else {
                    rows - lines_lf::count_breaks(&text[end..])
                };
                (row, to_u32(old_chars + added_chars))
            }
        };

        let row = to_u32(row);
        if row_chars > s.longest_row_chars
            || (row_chars == s.longest_row_chars && row < s.longest_row)
        {
            s.longest_row = row;
            s.longest_row_chars = row_chars;
        }
        s
    }

    /// Combines the summaries of two texts as if they were joined.
    /// `rows` and `rhs_rows` are the LF counts of the two texts, and
    /// `bytes` is the byte count of the joined text.
    ///
    /// No value in a summary is larger than its text's byte count, so u32
    /// math cannot overflow while `bytes` fits in u32.  Larger texts take
    /// the checked path.
    #[inline(always)]
    pub(crate) fn concat(
        self,
        rows: usize,
        rhs: LineSummary,
        rhs_rows: usize,
        bytes: usize,
    ) -> LineSummary {
        if bytes > u32::MAX as usize {
            return self.concat_checked(rows, rhs, rhs_rows);
        }
        self.concat_u32(rows, rhs, rhs_rows)
    }

    /// The counts and summary of texts joined in order, from their counts
    /// and summaries, in one pass.  Like `concat()`, it uses u32 math while
    /// the joined text fits in u32::MAX bytes.  Past that it starts again on
    /// the checked path.
    #[inline(always)]
    pub(crate) fn join(counts: &[TextInfo], lines: &[LineSummary]) -> (TextInfo, LineSummary) {
        let mut total = TextInfo::new();
        let mut acc = LineSummary::new();
        for (&c, &l) in counts.iter().zip(lines) {
            let rows = total.line_breaks_lf;
            total += c;
            if total.bytes > u32::MAX as usize {
                return Self::join_checked(counts, lines);
            }
            acc = acc.concat_u32(rows, l, c.line_breaks_lf);
        }
        (total, acc)
    }

    /// `join()` for joined texts over u32::MAX bytes.
    #[cold]
    #[inline(never)]
    fn join_checked(counts: &[TextInfo], lines: &[LineSummary]) -> (TextInfo, LineSummary) {
        let mut total = TextInfo::new();
        let mut acc = LineSummary::new();
        for (&c, &l) in counts.iter().zip(lines) {
            acc = acc.concat_checked(total.line_breaks_lf, l, c.line_breaks_lf);
            total += c;
        }
        (total, acc)
    }

    /// `concat()` with plain u32 math, for joined texts of at most
    /// u32::MAX bytes.
    #[inline(always)]
    fn concat_u32(self, rows: usize, rhs: LineSummary, rhs_rows: usize) -> LineSummary {
        let mut longest_row = self.longest_row;
        let mut longest_row_chars = self.longest_row_chars;

        let joined_chars = self.last_line_chars + rhs.first_line_chars;
        if joined_chars > longest_row_chars {
            longest_row = rows as u32;
            longest_row_chars = joined_chars;
        }
        if rhs.longest_row_chars > longest_row_chars {
            longest_row = rows as u32 + rhs.longest_row;
            longest_row_chars = rhs.longest_row_chars;
        }

        let first_line_chars = if rows == 0 {
            self.first_line_chars + rhs.first_line_chars
        } else {
            self.first_line_chars
        };

        if rhs_rows == 0 {
            LineSummary {
                last_line_bytes: self.last_line_bytes + rhs.last_line_bytes,
                first_line_chars,
                last_line_chars: self.last_line_chars + rhs.last_line_chars,
                last_line_utf16: self.last_line_utf16 + rhs.last_line_utf16,
                longest_row,
                longest_row_chars,
            }
        } else {
            LineSummary {
                last_line_bytes: rhs.last_line_bytes,
                first_line_chars,
                last_line_chars: rhs.last_line_chars,
                last_line_utf16: rhs.last_line_utf16,
                longest_row,
                longest_row_chars,
            }
        }
    }

    /// `concat()` for joined texts over u32::MAX bytes.  The math runs in
    /// u64 and panics when a result does not fit in u32.
    #[cold]
    #[inline(never)]
    fn concat_checked(self, rows: usize, rhs: LineSummary, rhs_rows: usize) -> LineSummary {
        let rows = rows as u64;
        let mut longest_row = self.longest_row as u64;
        let mut longest_row_chars = self.longest_row_chars as u64;

        let joined_chars = self.last_line_chars as u64 + rhs.first_line_chars as u64;
        if joined_chars > longest_row_chars {
            longest_row = rows;
            longest_row_chars = joined_chars;
        }
        if rhs.longest_row_chars as u64 > longest_row_chars {
            longest_row = rows + rhs.longest_row as u64;
            longest_row_chars = rhs.longest_row_chars as u64;
        }

        let first_line_chars = if rows == 0 {
            self.first_line_chars as u64 + rhs.first_line_chars as u64
        } else {
            self.first_line_chars as u64
        };

        let [last_line_bytes, last_line_chars, last_line_utf16] = if rhs_rows == 0 {
            [
                self.last_line_bytes as u64 + rhs.last_line_bytes as u64,
                self.last_line_chars as u64 + rhs.last_line_chars as u64,
                self.last_line_utf16 as u64 + rhs.last_line_utf16 as u64,
            ]
        } else {
            [
                rhs.last_line_bytes as u64,
                rhs.last_line_chars as u64,
                rhs.last_line_utf16 as u64,
            ]
        };

        let all = longest_row
            | longest_row_chars
            | first_line_chars
            | last_line_bytes
            | last_line_chars
            | last_line_utf16;
        if all > u32::MAX as u64 {
            too_large();
        }

        LineSummary {
            last_line_bytes: last_line_bytes as u32,
            first_line_chars: first_line_chars as u32,
            last_line_chars: last_line_chars as u32,
            last_line_utf16: last_line_utf16 as u32,
            longest_row: longest_row as u32,
            longest_row_chars: longest_row_chars as u32,
        }
    }
}

/// `n` as u32.  Panics in every build when it does not fit.
#[inline(always)]
fn to_u32(n: usize) -> u32 {
    match u32::try_from(n) {
        Ok(n) => n,
        Err(_) => too_large(),
    }
}

/// `a + b` as u32.  Panics in every build when the sum does not fit.
#[inline(always)]
fn add(a: u32, b: usize) -> u32 {
    match u32::try_from(b).ok().and_then(|b| a.checked_add(b)) {
        Some(n) => n,
        None => too_large(),
    }
}

#[cold]
#[inline(never)]
fn too_large() -> ! {
    panic!("a row length or row index does not fit in u32")
}

const HIGH_BITS: u64 = 0x8080_8080_8080_8080;

/// Top bit set in each byte of `word` that is an LF.
#[inline(always)]
fn lf_bits(word: u64) -> u64 {
    let v = word ^ 0x0A0A_0A0A_0A0A_0A0A;
    !(((v & !HIGH_BITS) + !HIGH_BITS) | v) & HIGH_BITS
}

/// Top bit set in each byte of `word` that is a utf8 continuation byte.
#[inline(always)]
fn cont_bits(word: u64) -> u64 {
    word & !(word << 1) & HIGH_BITS
}

#[cfg(test)]
mod tests {
    use super::*;
    use str_indices::chars;

    fn scan(text: &str) -> (LineSummary, usize) {
        let c = chars::count(text);
        LineSummary::from_str(text, c, c + utf16::count_surrogates(text))
    }

    // A plain char-by-char scan, written like Zed's `TextSummary::from(&str)`.
    fn naive(text: &str) -> (LineSummary, usize) {
        let mut rows = 0;
        let mut s = LineSummary::new();
        for c in text.chars() {
            if c == '\n' {
                rows += 1;
                s.last_line_bytes = 0;
                s.last_line_chars = 0;
                s.last_line_utf16 = 0;
            } else {
                s.last_line_bytes += c.len_utf8() as u32;
                s.last_line_chars += 1;
                s.last_line_utf16 += c.len_utf16() as u32;
            }
            if rows == 0 {
                s.first_line_chars = s.last_line_chars;
            }
            if s.last_line_chars > s.longest_row_chars {
                s.longest_row = rows as u32;
                s.longest_row_chars = s.last_line_chars;
            }
        }
        (s, rows)
    }

    const TEXTS: &[&str] = &[
        "",
        "a",
        "\n",
        "\n\n",
        "ab\ncde\nf",
        "abc\n\nab\n",
        "こんにちは\nみんな\nさん！\r\n",
        "🐸🐸\n👨‍👩‍👧‍👦 family\nx",
        "a\nbb\nccc\nbb\nccc\n",
        "\u{feff}\u{2028}\u{85}\n\t\t\r",
    ];

    #[test]
    fn from_str_matches_naive_scan() {
        let long = format!("{}\n{}", "é🐸x".repeat(40), "\nab".repeat(50));
        for text in TEXTS.iter().copied().chain([long.as_str()]) {
            assert_eq!(scan(text), naive(text), "{text:?}");
        }
    }

    #[test]
    fn concat_matches_scan_of_joined_text() {
        for a in TEXTS {
            for b in TEXTS {
                let (sa, ra) = scan(a);
                let (sb, rb) = scan(b);
                let joined = format!("{a}{b}");
                let sum = sa.concat(ra, sb, rb, joined.len());
                assert_eq!((sum, ra + rb), naive(&joined), "{a:?} + {b:?}");
            }
        }
    }

    fn one_row(len: u32) -> LineSummary {
        LineSummary {
            last_line_bytes: len,
            first_line_chars: len,
            last_line_chars: len,
            last_line_utf16: len,
            longest_row: 0,
            longest_row_chars: len,
        }
    }

    const MAX: usize = u32::MAX as usize;

    #[test]
    fn concat_fills_u32_rows_exactly() {
        let joined = one_row(u32::MAX - 1).concat(0, one_row(1), 0, MAX);
        assert_eq!(joined, one_row(u32::MAX));
    }

    #[test]
    fn concat_of_texts_over_u32_bytes_keeps_short_rows() {
        let (row, rows) = scan("\nab");
        let joined = one_row(u32::MAX).concat(0, row, rows, MAX + 3);
        assert_eq!(
            joined,
            LineSummary {
                last_line_bytes: 2,
                last_line_chars: 2,
                last_line_utf16: 2,
                ..one_row(u32::MAX)
            }
        );
    }

    #[test]
    #[should_panic(expected = "does not fit in u32")]
    fn concat_panics_when_a_row_is_longer_than_u32() {
        one_row(u32::MAX).concat(0, one_row(1), 0, MAX + 1);
    }

    #[test]
    #[should_panic(expected = "does not fit in u32")]
    fn concat_panics_when_a_row_index_is_past_u32() {
        let (row, rows) = scan("\nab");
        one_row(1).concat(MAX, row, rows, MAX + 4);
    }

    fn counts(bytes: usize, rows: usize) -> TextInfo {
        let mut c = TextInfo::new();
        c.bytes = bytes;
        c.line_breaks_lf = rows;
        c
    }

    #[test]
    fn join_of_texts_over_u32_bytes_keeps_short_rows() {
        let (row, rows) = scan("\nab");
        let (total, joined) = LineSummary::join(
            &[counts(MAX, 0), counts(3, rows)],
            &[one_row(u32::MAX), row],
        );
        assert_eq!(total, counts(MAX + 3, 1));
        assert_eq!(
            joined,
            LineSummary {
                last_line_bytes: 2,
                last_line_chars: 2,
                last_line_utf16: 2,
                ..one_row(u32::MAX)
            }
        );
    }

    #[test]
    #[should_panic(expected = "does not fit in u32")]
    fn join_panics_when_a_row_is_longer_than_u32() {
        LineSummary::join(
            &[counts(MAX, 0), counts(1, 0)],
            &[one_row(u32::MAX), one_row(1)],
        );
    }

    #[test]
    #[should_panic(expected = "does not fit in u32")]
    fn insert_in_row_panics_when_the_row_is_longer_than_u32() {
        let (summary, rows) = scan("a");
        let summary = LineSummary {
            last_line_bytes: u32::MAX,
            ..summary
        };
        summary.insert_in_row("a", 1, rows, true, [1, 1, 1]);
    }

    #[test]
    fn insert_in_row_matches_naive_scan() {
        // Rows that tie the longest row after an insert, before and after it,
        // in the first, a middle and the last row, with and without
        // multibyte chars.
        let ties = [
            "abc\nab\nabc",
            "ab\nabc\nab",
            "ab\nab\nabc",
            "abc\nab",
            "ab\nabc",
            "abcd\néé\nabcd",
            "ab\néé\nabc",
            "x\n\n\nx\n",
            "🐸🐸\nab\n🐸🐸\nabc",
        ];
        let inserts = ["x", "é", "🐸", "ab", "\r", "\t\u{85}\u{2028}"];
        for text in TEXTS.iter().copied().chain(ties) {
            let (summary, rows) = scan(text);
            for i in (0..=text.len()).filter(|&i| text.is_char_boundary(i)) {
                for ins in inserts {
                    let joined = format!("{}{ins}{}", &text[..i], &text[i..]);
                    let c = chars::count(ins);
                    let added = [ins.len(), c, c + utf16::count_surrogates(ins)];
                    let updated = summary.insert_in_row(text, i, rows, text.is_ascii(), added);
                    assert_eq!(updated, naive(&joined).0, "{text:?} at {i} + {ins:?}");
                }
            }
        }
    }
}
