use std::ops::{Add, AddAssign};

#[cfg(any(feature = "metric_chars", feature = "metric_utf16"))]
use str_indices::chars;

#[cfg(feature = "metric_utf16")]
use str_indices::utf16;

#[cfg(all(feature = "metric_lines_lf", not(feature = "text_summary")))]
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

    #[cfg(feature = "text_summary")]
    pub line_summary: LineSummary,
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

            #[cfg(feature = "text_summary")]
            line_summary: LineSummary::new(),
        }
    }

    pub(crate) fn from_str(text: &str) -> TextInfo {
        #[cfg(any(feature = "metric_chars", feature = "metric_utf16"))]
        let char_count = chars::count(text);

        #[cfg(feature = "metric_utf16")]
        let utf16_count = utf16::count_surrogates(text) + char_count;

        #[cfg(feature = "text_summary")]
        let (line_summary, lf_count) = LineSummary::from_str(text, char_count, utf16_count);

        TextInfo {
            bytes: text.len(),

            #[cfg(any(feature = "metric_chars", feature = "metric_utf16"))]
            chars: char_count,

            #[cfg(feature = "metric_utf16")]
            utf16: utf16_count,

            #[cfg(all(feature = "metric_lines_lf", not(feature = "text_summary")))]
            line_breaks_lf: lines_lf::count_breaks(text),

            #[cfg(feature = "text_summary")]
            line_breaks_lf: lf_count,

            #[cfg(feature = "metric_lines_lf_cr")]
            line_breaks_cr_lf: lines_crlf::count_breaks(text),

            #[cfg(feature = "metric_lines_unicode")]
            line_breaks_unicode: lines::count_breaks(text),

            #[cfg(feature = "text_summary")]
            line_summary,
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
}

impl TextInfo {
    /// Adds only the counts (bytes, chars, utf16 and line breaks).  The line
    /// summary of `self` is kept as is.
    ///
    /// For position searches, which only need the counts of the text to the
    /// left and run once per child on every lookup.
    #[inline(always)]
    pub(crate) fn add_counts(self, rhs: TextInfo) -> TextInfo {
        TextInfo {
            #[cfg(feature = "text_summary")]
            line_summary: self.line_summary,

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

impl Add for TextInfo {
    type Output = Self;
    // Note: this does *not* handle anything related to CRLF line breaks.  It
    // just does a simple sum of the metrics.
    //
    // Because of that, using this correctly typically requires special
    // handling.  Beware.
    //
    // The line summary is combined as if the texts were concatenated, so
    // `a + b` is the info of `a`'s text followed by `b`'s text.
    #[inline(always)]
    fn add(self, rhs: TextInfo) -> TextInfo {
        TextInfo {
            #[cfg(feature = "text_summary")]
            line_summary: self.line_summary.concat(
                self.line_breaks_lf,
                rhs.line_summary,
                rhs.line_breaks_lf,
            ),

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_01() {
        assert_eq!(TextInfo::new(), TextInfo::from_str(""));
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
