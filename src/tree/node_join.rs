//! Tree joins and splits, ported from Ropey 1.x (`Node::append_at_depth`,
//! `prepend_at_depth`, `split`, `zip_fix_*` and `fix_tree_seam`).
//!
//! Text content never changes here, only how it is spread over nodes, so a
//! node's own info stays the same when its subtree is merged or distributed.

use std::sync::Arc;

use super::{Children, Node, MAX_CHILDREN};

impl Node {
    /// Number of internal levels above the leaves.  A leaf has depth 0.
    pub(crate) fn depth(&self) -> usize {
        let mut node = self;
        let mut depth = 0;
        while let Node::Internal(ref children) = *node {
            depth += 1;
            node = &children.nodes()[0];
        }
        depth
    }

    /// Attaches `other` to the right edge of this tree, `depth` levels down.
    /// `other` must be `depth` levels shorter than `self`.
    ///
    /// Returns a node that must become this node's right sibling, if any.
    pub(crate) fn append_at_depth(&mut self, other: Node, depth: usize) -> Option<Node> {
        if depth == 0 {
            debug_assert_eq!(self.is_leaf(), other.is_leaf());
            return Some(other);
        }

        let children = self.children_mut();
        let last_i = children.len() - 1;
        let residual = children.nodes_mut()[last_i].append_at_depth(other, depth - 1);
        children.refresh_child(last_i);

        let extra = residual?;
        let info = extra.text_info();
        if children.len() < MAX_CHILDREN {
            children.push((info, extra));
            None
        } else {
            let right = children.push_split((info, extra));
            Some(Node::Internal(Arc::new(right)))
        }
    }

    /// Attaches `other` to the left edge of this tree, `depth` levels down.
    /// `other` must be `depth` levels shorter than `self`.
    ///
    /// Returns a node that must become this node's left sibling, if any.
    pub(crate) fn prepend_at_depth(&mut self, other: Node, depth: usize) -> Option<Node> {
        if depth == 0 {
            debug_assert_eq!(self.is_leaf(), other.is_leaf());
            return Some(other);
        }

        let children = self.children_mut();
        let residual = children.nodes_mut()[0].prepend_at_depth(other, depth - 1);
        children.refresh_child(0);

        let extra = residual?;
        let info = extra.text_info();
        if children.len() < MAX_CHILDREN {
            children.insert(0, (info, extra));
            None
        } else {
            let mut left = children.insert_split(0, (info, extra));
            std::mem::swap(children, &mut left);
            Some(Node::Internal(Arc::new(left)))
        }
    }

    /// Splits the tree at `byte_idx`, keeping the left part and returning
    /// the right part.  Both parts keep the same depth.
    ///
    /// `byte_idx` must be on a char boundary and strictly inside the node.
    pub(crate) fn split(&mut self, byte_idx: usize) -> Node {
        match *self {
            Node::Leaf(ref mut text) => {
                let text = Arc::make_mut(text);
                Node::Leaf(Arc::new(text.split(byte_idx)))
            }
            Node::Internal(ref mut children) => {
                let children = Arc::make_mut(children);
                let (child_i, acc) = children.search_byte_idx_only(byte_idx, false);

                if byte_idx == acc {
                    return Node::Internal(Arc::new(children.split_off(child_i)));
                }
                if byte_idx == acc + children.info()[child_i].bytes {
                    return Node::Internal(Arc::new(children.split_off(child_i + 1)));
                }

                let mut right = children.split_off(child_i + 1);
                let r_node = children.nodes_mut()[child_i].split(byte_idx - acc);
                let r_info = r_node.text_info();
                right.insert(0, (r_info, r_node));

                let l_info = children.nodes()[child_i].text_info();
                children.set_node_info(child_i, l_info);
                children.update_unbalance_flag(child_i);

                Node::Internal(Arc::new(right))
            }
        }
    }

    /// Merges or distributes undersized nodes down the left edge.
    ///
    /// Returns whether this node lost a child, which can make it undersized.
    pub(crate) fn zip_fix_left(&mut self) -> bool {
        if self.is_leaf() {
            return false;
        }
        let children = self.children_mut();
        let mut merged = false;
        loop {
            if children.len() > 1 && children.nodes()[0].is_directly_unbalanced() {
                merged |= children.merge_distribute(0, 1);
            }
            let child_merged = children.nodes_mut()[0].zip_fix_left();
            children.update_unbalance_flag(0);
            if !child_merged {
                break;
            }
        }
        merged
    }

    /// Merges or distributes undersized nodes down the right edge.
    ///
    /// Returns whether this node lost a child, which can make it undersized.
    pub(crate) fn zip_fix_right(&mut self) -> bool {
        if self.is_leaf() {
            return false;
        }
        let children = self.children_mut();
        let mut merged = false;
        loop {
            let last_i = children.len() - 1;
            if children.len() > 1 && children.nodes()[last_i].is_directly_unbalanced() {
                merged |= children.merge_distribute(last_i - 1, last_i);
            }
            let last_i = children.len() - 1;
            let child_merged = children.nodes_mut()[last_i].zip_fix_right();
            children.update_unbalance_flag(last_i);
            if !child_merged {
                break;
            }
        }
        merged
    }

    /// Merges or distributes undersized nodes along a seam at `byte_idx`,
    /// as left behind by a join.
    ///
    /// Returns whether this node lost a child, which can make it undersized.
    pub(crate) fn fix_tree_seam(&mut self, byte_idx: usize) -> bool {
        if self.is_leaf() {
            return false;
        }
        let children = self.children_mut();
        let mut merged = false;
        loop {
            if children.len() > 1 {
                let (child_i, start) = children.search_byte_idx_only(byte_idx, false);
                let undersized = children.nodes()[child_i].is_directly_unbalanced();
                if child_i == 0 {
                    if undersized {
                        merged |= children.merge_distribute(0, 1);
                    }
                } else if undersized
                    || (start == byte_idx && children.nodes()[child_i - 1].is_directly_unbalanced())
                {
                    merged |= children.merge_distribute(child_i - 1, child_i);
                }
            }

            let (child_i, start) = children.search_byte_idx_only(byte_idx, false);
            let child_merged = if start == byte_idx && child_i != 0 {
                let left_len = children.info()[child_i - 1].bytes;
                let a = children.nodes_mut()[child_i - 1].fix_tree_seam(left_len);
                children.update_unbalance_flag(child_i - 1);
                let b = children.nodes_mut()[child_i].fix_tree_seam(0);
                children.update_unbalance_flag(child_i);
                a || b
            } else {
                let c = children.nodes_mut()[child_i].fix_tree_seam(byte_idx - start);
                children.update_unbalance_flag(child_i);
                c
            };
            if !child_merged {
                break;
            }
        }
        merged
    }
}

impl Children {
    /// Refolds the info of an internal child after its subtree changed, and
    /// updates its unbalance flag.  Leaf children keep their info.
    pub(crate) fn refresh_child(&mut self, idx: usize) {
        if self.nodes()[idx].is_internal() {
            let info = self.nodes()[idx].text_info();
            self.set_node_info(idx, info);
        }
        self.update_unbalance_flag(idx);
    }
}
