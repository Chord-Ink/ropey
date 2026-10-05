//! O(log N) `append` and `split_off`, ported from Ropey 1.x.

use std::sync::Arc;

use crate::{
    tree::{Children, Node, NodeInfo},
    Error::*,
    Result, Rope,
};

impl Rope {
    /// Appends `other` to the end of this rope.
    ///
    /// The two trees are joined, not copied: runs in O(log N) time, and
    /// shares `other`'s nodes.
    ///
    /// # Example
    ///
    /// ```
    /// # use ropey::Rope;
    /// let mut rope = Rope::from_str("Hello ");
    /// rope.append(Rope::from_str("world!"));
    ///
    /// assert_eq!("Hello world!", rope);
    /// ```
    pub fn append(&mut self, other: Rope) {
        let mut other = other;
        self.make_whole();
        other.make_whole();

        // Keep a CRLF pair at the seam inside one leaf.
        let seam_cr = self.len() > 0 && self.byte(self.len() - 1) == b'\r';
        if seam_cr && other.len() > 0 && other.byte(0) == b'\n' {
            other.remove(0..1);
            let len = self.len();
            self.insert(len, "\n");
        }

        if other.len() == 0 {
            return;
        }
        if self.len() == 0 {
            *self = other;
            return;
        }

        let seam = self.len();
        let l_depth = self.root.depth();
        let r_depth = other.root.depth();

        if l_depth >= r_depth {
            if let Some(extra) = self.root.append_at_depth(other.root, l_depth - r_depth) {
                self.root = join_roots(self.root.clone(), extra);
            }
        } else {
            let mut root = other.root;
            if let Some(extra) = root.prepend_at_depth(self.root.clone(), r_depth - l_depth) {
                root = join_roots(extra, root);
            }
            self.root = root;
        }

        self.root.fix_tree_seam(seam);
        self.finish_structural_edit();
    }

    /// Splits the rope at `byte_idx`, keeping `..byte_idx` and returning
    /// `byte_idx..` as a new rope.
    ///
    /// Runs in O(log N) time.  No text is copied except the leaf that
    /// holds `byte_idx`.
    ///
    /// # Example
    ///
    /// ```
    /// # use ropey::Rope;
    /// let mut rope = Rope::from_str("Hello world!");
    /// let right = rope.split_off(5);
    ///
    /// assert_eq!("Hello", rope);
    /// assert_eq!(" world!", right);
    /// ```
    ///
    /// # Panics
    ///
    /// - If `byte_idx` is out of bounds (i.e. `byte_idx > len()`).
    /// - If `byte_idx` is not on a char boundary.
    #[track_caller]
    pub fn split_off(&mut self, byte_idx: usize) -> Rope {
        match self.try_split_off(byte_idx) {
            Ok(rope) => rope,
            Err(e) => panic!("{}", e),
        }
    }

    /// Non-panicking version of [`split_off()`](Rope::split_off).
    pub fn try_split_off(&mut self, byte_idx: usize) -> Result<Rope> {
        if byte_idx > self.len() {
            return Err(OutOfBounds);
        }
        if !self.is_char_boundary(byte_idx) {
            return Err(NonCharBoundary);
        }
        self.make_whole();

        if byte_idx == 0 {
            return Ok(std::mem::take(self));
        }
        if byte_idx == self.len() {
            return Ok(Rope::new());
        }

        let mut right = Rope {
            root: self.root.split(byte_idx),
            root_info: NodeInfo::new(),
            byte_range: [0, 0],
        };

        self.root.zip_fix_right();
        right.root.zip_fix_left();
        self.finish_structural_edit();
        right.finish_structural_edit();

        Ok(right)
    }

    /// Turns a "disconnected slice" into a normal rope.
    fn make_whole(&mut self) {
        if self.byte_range[0] != 0 || self.byte_range[1] != self.root_info.counts.bytes {
            self.trim_disconnected_slice();
        }
    }

    /// Recomputes the root info and range after the tree was rebuilt.
    fn finish_structural_edit(&mut self) {
        self.root.partial_rebalance();
        self.pull_up_singular_nodes();
        self.root_info = self.root.text_info();
        self.byte_range = [0, self.root_info.counts.bytes];
    }
}

fn join_roots(left: Node, right: Node) -> Node {
    let mut children = Children::new();
    children.push((left.text_info(), left));
    children.push((right.text_info(), right));
    Node::Internal(Arc::new(children))
}

//=============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RopeBuilder;

    const TEXT: &str = "Hello there!\r\nHow're you doing?\r\nこんにちは、みんなさん！\n\
                        🐸🐸 frogs 👨‍👩‍👧‍👦\r\n\n\tA longer row than the others.\r\
                        \nend of text\r";

    fn boundaries(text: &str) -> Vec<usize> {
        (0..=text.len()).filter(|&i| text.is_char_boundary(i)).collect()
    }

    fn assert_balanced(r: &Rope) {
        r.assert_invariants();
        assert!(!r.root.is_subtree_unbalanced(), "unbalanced tree: {}", shape(&r.root));
    }

    fn shape(n: &Node) -> String {
        match n {
            Node::Leaf(t) => format!("{}", t.len()),
            Node::Internal(c) => {
                let parts: Vec<String> = c.nodes().iter().map(shape).collect();
                format!("[{}]", parts.join(" "))
            }
        }
    }

    /// A rope with no undersized nodes, so that joins can be checked for
    /// leaving none behind.
    fn balanced(text: &str) -> Rope {
        let mut r = Rope::from_str(text);
        assert!(r.attempt_full_rebalance(10_000).0);
        assert_balanced(&r);
        r
    }

    /// One leaf per char, CRLF pairs kept together.
    fn rope_with_tiny_leaves(text: &str) -> Rope {
        let mut b = RopeBuilder::new();
        let mut start = 0;
        for end in boundaries(text).into_iter().skip(1) {
            if text[..end].ends_with('\r') && text[end..].starts_with('\n') {
                continue;
            }
            b._append_chunk_as_leaf(&text[start..end]);
            start = end;
        }
        b.finish()
    }

    #[test]
    fn split_off_everywhere() {
        let text = TEXT.repeat(3);
        let r = balanced(&text);
        for i in boundaries(&text) {
            let mut left = r.clone();
            let right = left.split_off(i);
            assert_eq!(left, &text[..i]);
            assert_eq!(right, &text[i..]);
            assert_balanced(&left);
            assert_balanced(&right);
        }
        assert_eq!(r, text.as_str());
    }

    #[test]
    fn split_off_unbalanced_input() {
        // Also checks that copy-on-write keeps the unbalance flags.
        let r = rope_with_tiny_leaves(TEXT);
        r.assert_invariants();
        for i in boundaries(TEXT) {
            let mut left = r.clone();
            let right = left.split_off(i);
            assert_eq!(left, &TEXT[..i]);
            assert_eq!(right, &TEXT[i..]);
            left.assert_invariants();
            right.assert_invariants();
        }
    }

    #[test]
    fn append_every_split() {
        let text = TEXT.repeat(3);
        for i in boundaries(&text) {
            let mut left = balanced(&text[..i]);
            left.append(balanced(&text[i..]));
            assert_eq!(left, text.as_str());
            assert_balanced(&left);
        }
    }

    #[test]
    fn append_different_depths() {
        let long = TEXT.repeat(40);
        for short in ["", "a", "\n", "\r", "\r\n", TEXT, &TEXT[..20]] {
            let mut a = balanced(&long);
            a.append(balanced(short));
            assert_eq!(a, format!("{long}{short}").as_str());
            assert_balanced(&a);

            let mut b = balanced(short);
            b.append(balanced(&long));
            assert_eq!(b, format!("{short}{long}").as_str());
            assert_balanced(&b);
        }
    }

    #[test]
    fn append_crlf_seam() {
        for (a, b) in [("\r", "\n"), ("ab\r", "\ncd"), ("\r", "\n\n"), ("x\r", "\nx")] {
            let long_a = format!("{}{a}", TEXT.repeat(10));
            let long_b = format!("{b}{}", TEXT.repeat(10));
            for (l, r) in [(a, b), (long_a.as_str(), b), (a, long_b.as_str())] {
                let mut rope = balanced(l);
                rope.append(balanced(r));
                assert_eq!(rope, format!("{l}{r}").as_str());
                assert_balanced(&rope);
            }
        }
    }

    #[test]
    fn append_shared_and_sliced() {
        let r = Rope::from_str(&TEXT.repeat(8));
        let text = String::from(&r);
        let at = |i: usize| (0..=i).rev().find(|&j| text.is_char_boundary(j)).unwrap();
        let (x, y, z) = (at(7), at(300), at(400));
        let mut a = Rope::from(r.slice(x..y));
        let b = crate::extra::esoterica::disconnect_slice(r.slice(z..)).unwrap();
        a.append(b);
        assert_eq!(a, format!("{}{}", &text[x..y], &text[z..]).as_str());
        a.assert_invariants();
        assert_eq!(r, text.as_str());
    }

    #[test]
    fn split_then_append_round_trip() {
        let text = TEXT.repeat(5);
        let r = balanced(&text);
        for i in boundaries(&text).into_iter().step_by(7) {
            let mut left = r.clone();
            let right = left.split_off(i);
            left.append(right);
            assert_eq!(left, text.as_str());
            assert_balanced(&left);
        }
    }

    #[test]
    fn split_off_errors() {
        let mut r = Rope::from_str("こんにちは");
        assert_eq!(r.try_split_off(1).err(), Some(NonCharBoundary));
        assert_eq!(r.try_split_off(16).err(), Some(OutOfBounds));
        assert_eq!(r.split_off(15), "");
        assert_eq!(r.split_off(0), "こんにちは");
        assert_eq!(r, "");
    }
}
