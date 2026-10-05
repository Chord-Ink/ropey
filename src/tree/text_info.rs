use std::ops::{Add, AddAssign, Sub, SubAssign};

#[cfg(any(feature = "metric_chars", feature = "metric_utf16"))]
use str_indices::chars;

#[cfg(feature = "metric_utf16")]
use str_indices::utf16;

#[cfg(feature = "metric_lines_lf")]
use str_indices::lines_lf;

#[cfg(feature = "metric_lines_lf_cr")]
use str_indices::lines_crlf;

#[cfg(feature = "metric_lines_unicode")]
use str_indices::lines;

#[cfg(any(
    feature = "metric_lines_lf",
    feature = "metric_lines_lf_cr",
    feature = "metric_lines_unicode"
))]
use crate::LineType;

#[cfg(feature = "text_summary")]
pub(crate) use super::line_summary::LineSummary;

use crate::str_utils;

/// The counts of a text: its bytes, and its chars, UTF-16 code units and
/// line breaks when their metrics are on.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub(crate) struct TextInfo {
    pub bytes: usize,

    #[cfg(any(feature = "metric_chars", feature = "metric_utf16"))]
    pub chars: usize,

    #[cfg(feature = "metric_utf16")]
    pub utf16: usize,

    #[cfg(feature = "metric_lines_lf")]
    pub line_breaks_lf: usize,

    #[cfg(feature = "metric_lines_lf_cr")]
    pub line_breaks_cr_lf: usize,

    #[cfg(feature = "metric_lines_unicode")]
    pub line_breaks_unicode: usize,
}

impl TextInfo {
    /// Creates a new empty `TextInfo`.
    ///
    /// The returned `TextInfo` is identical to what `TextInfo::from_str("")`
    /// would return, but is constructed more efficiently since this can skip
    /// all of the text scan function calls.
    pub(crate) fn new() -> TextInfo {
        TextInfo {
            bytes: 0,

            #[cfg(any(feature = "metric_chars", feature = "metric_utf16"))]
            chars: 0,

            #[cfg(feature = "metric_utf16")]
            utf16: 0,

            #[cfg(feature = "metric_lines_lf")]
            line_breaks_lf: 0,

            #[cfg(feature = "metric_lines_lf_cr")]
            line_breaks_cr_lf: 0,

            #[cfg(feature = "metric_lines_unicode")]
            line_breaks_unicode: 0,
        }
    }

    /// With `text_summary`, `NodeInfo::from_str()` counts LFs in the same
    /// pass as the line summary, and is used instead.
    #[cfg_attr(feature = "text_summary", allow(dead_code))]
    pub(crate) fn from_str(text: &str) -> TextInfo {
        #[cfg(any(feature = "metric_chars", feature = "metric_utf16"))]
        let char_count = chars::count(text);

        TextInfo {
            bytes: text.len(),

            #[cfg(any(feature = "metric_chars", feature = "metric_utf16"))]
            chars: char_count,

            #[cfg(feature = "metric_utf16")]
            utf16: utf16::count_surrogates(text) + char_count,

            #[cfg(feature = "metric_lines_lf")]
            line_breaks_lf: lines_lf::count_breaks(text),

            #[cfg(feature = "metric_lines_lf_cr")]
            line_breaks_cr_lf: lines_crlf::count_breaks(text),

            #[cfg(feature = "metric_lines_unicode")]
            line_breaks_unicode: lines::count_breaks(text),
        }
    }

    #[cfg(any(
        feature = "metric_lines_lf",
        feature = "metric_lines_lf_cr",
        feature = "metric_lines_unicode"
    ))]
    #[inline(always)]
    pub(crate) fn line_breaks(&self, line_type: LineType) -> usize {
        match line_type {
            #[cfg(feature = "metric_lines_lf")]
            LineType::LF => self.line_breaks_lf,

            #[cfg(feature = "metric_lines_lf_cr")]
            LineType::LF_CR => self.line_breaks_cr_lf,

            #[cfg(feature = "metric_lines_unicode")]
            LineType::Unicode => self.line_breaks_unicode,
        }
    }

    /// The counts of `text` after `ins` is put at `byte_idx`, worked out
    /// from `self`, the counts of `text`, without a rescan.
    ///
    /// Returns `None` when `ins` holds an LF, or a CR while a line metric
    /// counts CR.  The caller then rescans the text.
    #[must_use]
    pub(crate) fn str_insert(self, text: &str, byte_idx: usize, ins: &str) -> Option<TextInfo> {
        #[cfg(any(feature = "metric_lines_lf_cr", feature = "metric_lines_unicode"))]
        let rescan = str_utils::has_lf_or_cr(ins.as_bytes());
        #[cfg(not(any(feature = "metric_lines_lf_cr", feature = "metric_lines_unicode")))]
        let rescan = str_utils::find_lf(ins.as_bytes()).is_some();
        if rescan {
            return None;
        }

        // Silence unused parameter warnings with some features.
        let _ = (text, byte_idx);

        #[cfg(any(feature = "metric_chars", feature = "metric_utf16"))]
        let ins_chars = chars::count(ins);

        // Putting a char between a CR and an LF splits one break into two.
        #[cfg(any(feature = "metric_lines_lf_cr", feature = "metric_lines_unicode"))]
        let split_crlf = (byte_idx > 0
            && str_utils::byte_is_cr(text, byte_idx - 1)
            && str_utils::byte_is_lf(text, byte_idx)) as usize;

        Some(TextInfo {
            bytes: self.bytes + ins.len(),

            #[cfg(any(feature = "metric_chars", feature = "metric_utf16"))]
            chars: self.chars + ins_chars,

            #[cfg(feature = "metric_utf16")]
            utf16: self.utf16 + ins_chars + utf16::count_surrogates(ins),

            #[cfg(feature = "metric_lines_lf")]
            line_breaks_lf: self.line_breaks_lf,

            #[cfg(feature = "metric_lines_lf_cr")]
            line_breaks_cr_lf: self.line_breaks_cr_lf + split_crlf,

            #[cfg(feature = "metric_lines_unicode")]
            line_breaks_unicode: self.line_breaks_unicode + split_crlf + lines::count_breaks(ins),
        })
    }
}

impl Add for TextInfo {
    type Output = Self;
    // Note: this does *not* handle anything related to CRLF line breaks.  It
    // just does a simple sum of the metrics.
    //
    // Because of that, using this correctly typically requires special
    // handling.  Beware.
    #[inline(always)]
    fn add(self, rhs: TextInfo) -> TextInfo {
        TextInfo {
            bytes: self.bytes + rhs.bytes,

            #[cfg(any(feature = "metric_chars", feature = "metric_utf16"))]
            chars: self.chars + rhs.chars,

            #[cfg(feature = "metric_utf16")]
            utf16: self.utf16 + rhs.utf16,

            #[cfg(feature = "metric_lines_lf")]
            line_breaks_lf: self.line_breaks_lf + rhs.line_breaks_lf,

            #[cfg(feature = "metric_lines_lf_cr")]
            line_breaks_cr_lf: self.line_breaks_cr_lf + rhs.line_breaks_cr_lf,

            #[cfg(feature = "metric_lines_unicode")]
            line_breaks_unicode: self.line_breaks_unicode + rhs.line_breaks_unicode,
        }
    }
}

impl AddAssign for TextInfo {
    #[inline(always)]
    fn add_assign(&mut self, other: TextInfo) {
        *self = *self + other;
    }
}

impl Sub for TextInfo {
    type Output = Self;
    // IMPORTANT: this does *not* handle anything related to CRLF line breaks.
    // It just does a simple subtraction of the metrics.
    //
    // Because of that, using this correctly typically requires special
    // handling.  Beware.
    #[inline(always)]
    fn sub(self, rhs: TextInfo) -> TextInfo {
        TextInfo {
            bytes: self.bytes - rhs.bytes,

            #[cfg(any(feature = "metric_chars", feature = "metric_utf16"))]
            chars: self.chars - rhs.chars,

            #[cfg(feature = "metric_utf16")]
            utf16: self.utf16 - rhs.utf16,

            #[cfg(feature = "metric_lines_lf")]
            line_breaks_lf: self.line_breaks_lf - rhs.line_breaks_lf,

            #[cfg(feature = "metric_lines_lf_cr")]
            line_breaks_cr_lf: self.line_breaks_cr_lf - rhs.line_breaks_cr_lf,

            #[cfg(feature = "metric_lines_unicode")]
            line_breaks_unicode: self.line_breaks_unicode - rhs.line_breaks_unicode,
        }
    }
}

impl SubAssign for TextInfo {
    #[inline(always)]
    fn sub_assign(&mut self, other: TextInfo) {
        *self = *self - other;
    }
}

//=============================================================

/// What a parent keeps for each child: the counts, and with `text_summary`
/// the line summary.
///
/// `Children` stores the two parts in separate arrays, so position searches
/// read only the counts.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub(crate) struct NodeInfo {
    pub counts: TextInfo,

    #[cfg(feature = "text_summary")]
    pub line_summary: LineSummary,
}

impl NodeInfo {
    /// Creates a new empty `NodeInfo`.
    #[inline(always)]
    pub(crate) fn new() -> NodeInfo {
        NodeInfo {
            counts: TextInfo::new(),

            #[cfg(feature = "text_summary")]
            line_summary: LineSummary::new(),
        }
    }

    pub(crate) fn from_str(text: &str) -> NodeInfo {
        #[cfg(not(feature = "text_summary"))]
        return NodeInfo {
            counts: TextInfo::from_str(text),
        };

        #[cfg(feature = "text_summary")]
        {
            let char_count = chars::count(text);
            let utf16_count = utf16::count_surrogates(text) + char_count;
            let (line_summary, lf_count) = LineSummary::from_str(text, char_count, utf16_count);

            NodeInfo {
                counts: TextInfo {
                    bytes: text.len(),
                    chars: char_count,
                    utf16: utf16_count,
                    line_breaks_lf: lf_count,

                    #[cfg(feature = "metric_lines_lf_cr")]
                    line_breaks_cr_lf: lines_crlf::count_breaks(text),

                    #[cfg(feature = "metric_lines_unicode")]
                    line_breaks_unicode: lines::count_breaks(text),
                },
                line_summary,
            }
        }
    }

    /// The info of `text` after `ins` is put at `byte_idx`, worked out from
    /// `self`, the info of `text`, without a rescan.
    ///
    /// Returns `None` when `ins` holds a line break.  See
    /// `TextInfo::str_insert()`.
    #[must_use]
    pub(crate) fn str_insert(self, text: &str, byte_idx: usize, ins: &str) -> Option<NodeInfo> {
        let counts = self.counts.str_insert(text, byte_idx, ins)?;

        Some(NodeInfo {
            #[cfg(feature = "text_summary")]
            line_summary: self.line_summary.insert_in_row(
                text,
                byte_idx,
                self.counts.line_breaks_lf,
                self.counts.chars == self.counts.bytes,
                [
                    ins.len(),
                    counts.chars - self.counts.chars,
                    counts.utf16 - self.counts.utf16,
                ],
            ),

            counts,
        })
    }

    /// Whether `self` and `other` have the same LF count and line summary.
    /// A parent's line summary depends on its children only through these.
    #[inline(always)]
    pub(crate) fn same_rows(&self, other: &NodeInfo) -> bool {
        // Silence unused parameter warning when `text_summary` is off.
        let _ = other;

        #[cfg(feature = "text_summary")]
        return self.counts.line_breaks_lf == other.counts.line_breaks_lf
            && self.line_summary == other.line_summary;

        #[cfg(not(feature = "text_summary"))]
        return true;
    }

    /// The info of a node after one child's info changed from `old` to
    /// `new`, where `self` is the node's old info.  Only the counts change:
    /// the line summary is kept, so `old` and `new` must have the same rows.
    #[inline(always)]
    pub(crate) fn replace_counts(self, old: NodeInfo, new: NodeInfo) -> NodeInfo {
        NodeInfo {
            counts: self.counts - old.counts + new.counts,
            ..self
        }
    }
}

impl Add for NodeInfo {
    type Output = Self;
    // The counts are summed as `TextInfo` does.  The line summary is
    // combined as if the texts were joined, so `a + b` is the info of `a`'s
    // text followed by `b`'s text.
    #[inline(always)]
    fn add(self, rhs: NodeInfo) -> NodeInfo {
        NodeInfo {
            #[cfg(feature = "text_summary")]
            line_summary: self.line_summary.concat(
                self.counts.line_breaks_lf,
                rhs.line_summary,
                rhs.counts.line_breaks_lf,
                self.counts.bytes + rhs.counts.bytes,
            ),

            counts: self.counts + rhs.counts,
        }
    }
}

impl AddAssign for NodeInfo {
    #[inline(always)]
    fn add_assign(&mut self, other: NodeInfo) {
        *self = *self + other;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_01() {
        assert_eq!(TextInfo::new(), TextInfo::from_str(""));
        assert_eq!(NodeInfo::new(), NodeInfo::from_str(""));
    }

    #[test]
    fn node_info_counts_match_text_info() {
        let texts = [
            "",
            "a\nb",
            "\r\n\r\n",
            "こんにちは\n🐸\r",
            "\u{85}x\u{2028}\n",
        ];
        for text in texts {
            assert_eq!(NodeInfo::from_str(text).counts, TextInfo::from_str(text));
        }
    }

    #[test]
    fn str_insert_matches_rescan() {
        let texts = [
            "",
            "a",
            "ab\ncd",
            "\r\n",
            "a\r\nb\r\n\r",
            "こんにちは\n🐸\r",
            "\u{85}x\u{2028}\u{0B}",
            "ab\nabc\nab",
        ];
        let inserts = [
            "x",
            "é",
            "🐸",
            "\t\u{85}",
            "\u{2029}\u{0C}",
            "\r",
            "\n",
            "a\r\nb",
        ];
        let cr_counts = cfg!(any(
            feature = "metric_lines_lf_cr",
            feature = "metric_lines_unicode"
        ));
        for text in texts {
            let info = NodeInfo::from_str(text);
            for i in (0..=text.len()).filter(|&i| text.is_char_boundary(i)) {
                for ins in inserts {
                    let joined = format!("{}{ins}{}", &text[..i], &text[i..]);
                    let rescan = ins.contains('\n') || (cr_counts && ins.contains('\r'));
                    match info.str_insert(text, i, ins) {
                        Some(new_info) => {
                            assert!(!rescan, "{text:?} at {i} + {ins:?}");
                            assert_eq!(new_info, NodeInfo::from_str(&joined), "{joined:?}");
                        }
                        None => assert!(rescan, "{text:?} at {i} + {ins:?}"),
                    }
                }
            }
        }
    }

    #[test]
    fn from_str_bytes_01() {
        assert_eq!(0, TextInfo::from_str("").bytes);
        assert_eq!(6, TextInfo::from_str("Hello!").bytes);
        assert_eq!(18, TextInfo::from_str("こんにちは！").bytes);
    }

    #[cfg(feature = "metric_chars")]
    #[test]
    fn from_str_chars_01() {
        assert_eq!(0, TextInfo::from_str("").chars);
        assert_eq!(6, TextInfo::from_str("Hello!").chars);
        assert_eq!(6, TextInfo::from_str("こんにちは！").chars);
    }

    #[cfg(feature = "metric_lines_lf")]
    #[test]
    fn from_str_line_breaks_lf_01() {
        assert_eq!(0, TextInfo::from_str("").line_breaks_lf);
        assert_eq!(0, TextInfo::from_str("\u{0085}").line_breaks_lf);
        assert_eq!(0, TextInfo::from_str("\r").line_breaks_lf);
        assert_eq!(1, TextInfo::from_str("\n").line_breaks_lf);
        assert_eq!(1, TextInfo::from_str("\r\n").line_breaks_lf);
        assert_eq!(1, TextInfo::from_str("Hello\n world!").line_breaks_lf);
        assert_eq!(3, TextInfo::from_str("\nこんにち\nは！\n").line_breaks_lf);
    }

    #[cfg(feature = "metric_lines_lf_cr")]
    #[test]
    fn from_str_line_breaks_cr_lf_01() {
        assert_eq!(0, TextInfo::from_str("").line_breaks_cr_lf);
        assert_eq!(0, TextInfo::from_str("\u{0085}").line_breaks_cr_lf);
        assert_eq!(1, TextInfo::from_str("\r").line_breaks_cr_lf);
        assert_eq!(1, TextInfo::from_str("\n").line_breaks_cr_lf);
        assert_eq!(1, TextInfo::from_str("\r\n").line_breaks_cr_lf);
        assert_eq!(1, TextInfo::from_str("Hello\n world!").line_breaks_cr_lf);
        assert_eq!(
            4,
            TextInfo::from_str("\nこん\rにち\nは！\r\n").line_breaks_cr_lf
        );
    }

    #[cfg(feature = "metric_lines_unicode")]
    #[test]
    fn from_str_line_breaks_unicode_01() {
        assert_eq!(0, TextInfo::from_str("").line_breaks_unicode);
        assert_eq!(1, TextInfo::from_str("\u{0085}").line_breaks_unicode);
        assert_eq!(1, TextInfo::from_str("\r").line_breaks_unicode);
        assert_eq!(1, TextInfo::from_str("\n").line_breaks_unicode);
        assert_eq!(1, TextInfo::from_str("\r\n").line_breaks_unicode);
        assert_eq!(1, TextInfo::from_str("Hello\n world!").line_breaks_unicode);
        assert_eq!(
            4,
            TextInfo::from_str("\nこん\rにち\nは！\r\n").line_breaks_unicode
        );
    }
}
