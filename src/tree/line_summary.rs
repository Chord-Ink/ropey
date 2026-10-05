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

use str_indices::{chars, lines_lf, utf16};

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
            s.first_line_chars += added_chars as u32;
        }
        if row_end.is_none() {
            s.last_line_bytes += added_bytes as u32;
            s.last_line_chars += added_chars as u32;
            s.last_line_utf16 += added_utf16 as u32;
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
                (row, (old_chars + added_chars) as u32)
            }
        };

        let row = row as u32;
        if row_chars > s.longest_row_chars
            || (row_chars == s.longest_row_chars && row < s.longest_row)
        {
            s.longest_row = row;
            s.longest_row_chars = row_chars;
        }
        s
    }

    /// Combines the summaries of two texts as if they were joined.
    /// `rows` and `rhs_rows` are the LF counts of the two texts.
    #[inline(always)]
    pub(crate) fn concat(self, rows: usize, rhs: LineSummary, rhs_rows: usize) -> LineSummary {
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
                assert_eq!((sa.concat(ra, sb, rb), ra + rb), naive(&joined), "{a:?} + {b:?}");
            }
        }
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
