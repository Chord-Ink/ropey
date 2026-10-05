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

use str_indices::{chars, utf16};

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
    pub(crate) fn from_str(text: &str, char_count: usize, utf16_count: usize) -> (Self, usize) {
        let first_lf = match find_lf(text) {
            Some(i) => i,
            None => {
                let summary = LineSummary {
                    last_line_bytes: text.len() as u32,
                    first_line_chars: char_count as u32,
                    last_line_chars: char_count as u32,
                    last_line_utf16: utf16_count as u32,
                    longest_row: 0,
                    longest_row_chars: char_count as u32,
                };
                return (summary, 0);
            }
        };

        // Rows are split at LF bytes, which are always char boundaries.
        let all_ascii = char_count == text.len();
        let count_chars = |row: &str| -> usize {
            if all_ascii {
                row.len()
            } else {
                chars::count(row)
            }
        };

        let first_line_chars = count_chars(&text[..first_lf]);
        let mut longest_row = 0;
        let mut longest_row_chars = first_line_chars;

        // Walk the remaining rows.  A row's char count is at most its byte
        // count, so rows that are not longer in bytes than the current
        // longest row are skipped without counting.
        let mut row = 0;
        let mut row_start = first_lf + 1;
        let last_row_start;
        loop {
            row += 1;
            let row_end = match find_lf(&text[row_start..]) {
                Some(i) => row_start + i,
                None => {
                    last_row_start = row_start;
                    break;
                }
            };
            if row_end - row_start > longest_row_chars {
                let n = count_chars(&text[row_start..row_end]);
                if n > longest_row_chars {
                    longest_row = row;
                    longest_row_chars = n;
                }
            }
            row_start = row_end + 1;
        }

        let last = &text[last_row_start..];
        let last_line_chars = if all_ascii {
            last.len()
        } else {
            chars::count(last)
        };
        if last_line_chars > longest_row_chars {
            longest_row = row;
            longest_row_chars = last_line_chars;
        }
        let last_line_utf16 = if all_ascii {
            last.len()
        } else {
            last_line_chars + utf16::count_surrogates(last)
        };

        let summary = LineSummary {
            last_line_bytes: last.len() as u32,
            first_line_chars: first_line_chars as u32,
            last_line_chars: last_line_chars as u32,
            last_line_utf16: last_line_utf16 as u32,
            longest_row: longest_row as u32,
            longest_row_chars: longest_row_chars as u32,
        };
        (summary, row)
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

/// Byte index of the first LF in `text`.
#[inline(always)]
pub(crate) fn find_lf(text: &str) -> Option<usize> {
    #[cfg(feature = "memchr")]
    {
        memchr::memchr(b'\n', text.as_bytes())
    }
    #[cfg(not(feature = "memchr"))]
    {
        text.find('\n')
    }
}

//=============================================================

#[cfg(test)]
mod tests {
    use super::*;

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
        for text in TEXTS {
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
}
