#![cfg(all(not(miri), feature = "text_summary"))]

//! `text_summary()`, `append()` and `split_off()` against a `String` model.

use proptest::prelude::*;
use proptest::test_runner::Config;

use ropey::{Rope, TextSummary};

/// A plain char-by-char scan, written like Zed's `TextSummary::from(&str)`.
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

fn floor(text: &str, frac: f64) -> usize {
    let mut i = ((text.len() as f64) * frac) as usize;
    while !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn text_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        "\\PC{0,100}",
        "[a-z \\t\\n]{0,400}",
        "[一-龥ぁ-んé🐸👨👩👧👦\\u{200D}\\u{FE0F}\\n\\r]{0,200}",
        "(\\r\\n|\\n|\\r|x){0,100}",
        "x{300,3000}",
    ]
}

#[derive(Debug, Clone)]
enum Op {
    Insert(f64, String),
    Type(f64, String),
    Remove(f64, f64),
    Append(String),
    Prepend(String),
    KeepLeft(f64),
    KeepRight(f64),
    SplitJoin(f64),
    SliceJoin(f64, f64),
    Move(f64, f64, f64),
}

fn op_strategy() -> impl Strategy<Value = Op> {
    let f = 0.0..=1.0f64;
    prop_oneof![
        (f.clone(), text_strategy()).prop_map(|(a, t)| Op::Insert(a, t)),
        (f.clone(), "[a-zé🐸 \\t\\r]{1,40}").prop_map(|(a, t)| Op::Type(a, t)),
        (f.clone(), f.clone()).prop_map(|(a, b)| Op::Remove(a, b)),
        text_strategy().prop_map(Op::Append),
        text_strategy().prop_map(Op::Prepend),
        f.clone().prop_map(Op::KeepLeft),
        f.clone().prop_map(Op::KeepRight),
        f.clone().prop_map(Op::SplitJoin),
        (f.clone(), f.clone()).prop_map(|(a, b)| Op::SliceJoin(a, b)),
        (f.clone(), f.clone(), f).prop_map(|(a, b, c)| Op::Move(a, b, c)),
    ]
}

/// Applies `op` to both the rope and the model.
fn apply(rope: &mut Rope, text: &mut String, op: &Op) {
    match op {
        Op::Insert(a, t) => {
            let i = floor(text, *a);
            rope.insert(i, t);
            text.insert_str(i, t);
        }
        Op::Type(a, t) => {
            // One char at a time, the way typing reaches the rope.
            let mut i = floor(text, *a);
            for c in t.chars() {
                rope.insert_char(i, c);
                text.insert(i, c);
                i += c.len_utf8();
            }
        }
        Op::Remove(a, b) => {
            let (i, j) = (floor(text, a.min(*b)), floor(text, a.max(*b)));
            rope.remove(i..j);
            text.replace_range(i..j, "");
        }
        Op::Append(t) => {
            rope.append(Rope::from_str(t));
            text.push_str(t);
        }
        Op::Prepend(t) => {
            let mut r = Rope::from_str(t);
            r.append(std::mem::take(rope));
            *rope = r;
            text.insert_str(0, t);
        }
        Op::KeepLeft(a) => {
            let i = floor(text, *a);
            let right = rope.split_off(i);
            assert_eq!(right, &text[i..]);
            right.assert_invariants();
            text.truncate(i);
        }
        Op::KeepRight(a) => {
            let i = floor(text, *a);
            let right = rope.split_off(i);
            assert_eq!(*rope, &text[..i]);
            rope.assert_invariants();
            *rope = right;
            text.replace_range(..i, "");
        }
        Op::SplitJoin(a) => {
            let i = floor(text, *a);
            let right = rope.split_off(i);
            rope.append(right);
        }
        Op::SliceJoin(a, b) => {
            // Rebuilds the rope from owned slices, the way `text`'s
            // `RopeBuilder` does: prefix, then the middle, then the suffix.
            let (i, j) = (floor(text, a.min(*b)), floor(text, a.max(*b)));
            let mut r = Rope::from(rope.slice(..i));
            r.append(Rope::from(rope.slice(i..j)));
            r.append(Rope::from(rope.slice(j..)));
            *rope = r;
        }
        Op::Move(a, b, c) => {
            // Cuts a range out with two splits and joins it back elsewhere,
            // the way deleted text moves between `text`'s two ropes.
            let (i, j) = (floor(text, a.min(*b)), floor(text, a.max(*b)));
            let mut middle = rope.split_off(i);
            let tail = middle.split_off(j - i);
            rope.append(tail);
            let moved = text[i..j].to_string();
            text.replace_range(i..j, "");
            let k = floor(text, *c);
            let tail = rope.split_off(k);
            rope.append(middle);
            rope.append(tail);
            text.insert_str(k, &moved);
        }
    }
}

fn check(rope: &Rope, text: &str, ranges: &[(f64, f64)]) {
    rope.assert_invariants();
    assert_eq!(rope, text);
    assert_eq!(rope.text_summary(..), naive(text));
    for &(a, b) in ranges {
        let (i, j) = (floor(text, a.min(b)), floor(text, a.max(b)));
        assert_eq!(rope.text_summary(i..j), naive(&text[i..j]), "{i}..{j}");
        let slice = rope.slice(i..);
        assert_eq!(slice.text_summary(..j - i), naive(&text[i..j]), "slice {i}..{j}");
    }
}

proptest! {
    #![proptest_config(Config::with_cases(1024))]

    #[test]
    fn pt_text_summary_after_edits(
        ref start in text_strategy(),
        ref ops in prop::collection::vec(op_strategy(), 1..16),
        ref ranges in prop::collection::vec((0.0..=1.0f64, 0.0..=1.0f64), 8),
    ) {
        let mut rope = Rope::from_str(start);
        let mut text = start.clone();
        check(&rope, &text, ranges);
        for op in ops {
            apply(&mut rope, &mut text, op);
            check(&rope, &text, ranges);
        }
    }

    #[test]
    fn pt_text_summary_random_ranges(
        ref text in "(\\PC{0,30}\\n){0,60}",
        ref ranges in prop::collection::vec((0.0..=1.0f64, 0.0..=1.0f64), 32),
    ) {
        let rope = Rope::from_str(text);
        check(&rope, text, ranges);
    }
}
