//! Maps a parsed tmux window layout into Warp's serialized pane-tree snapshot.
//!
//! tmux `{}` splits are left-right and `[]` splits are top-bottom; Warp's
//! `SplitDirection::Horizontal` is likewise left-right and `Vertical` is
//! top-bottom, so the two enums map straight across. Each child's flex weight
//! is its extent along the parent's split axis (width for a left-right split,
//! height for a top-bottom split), which are the relative weights Warp's pane
//! tree expects.

use warp_terminal::tmux::layout::{LayoutNode, SplitDir, Window};

use crate::app_state::{BranchSnapshot, LeafSnapshot, PaneFlex, PaneNodeSnapshot, SplitDirection};

/// tmux `{}` (left-right) maps to Warp `Horizontal`; tmux `[]` (top-bottom)
/// maps to Warp `Vertical`.
pub fn map_split_direction(dir: SplitDir) -> SplitDirection {
    match dir {
        SplitDir::Horizontal => SplitDirection::Horizontal,
        SplitDir::Vertical => SplitDirection::Vertical,
    }
}

/// Convert a parsed tmux window layout into a Warp pane-node snapshot tree.
///
/// `make_leaf` builds the snapshot leaf for a given tmux pane id, so the caller
/// owns the pane contents while this function owns the pure tree shape and
/// split-direction mapping.
pub fn map_window<F>(window: &Window, make_leaf: &F) -> PaneNodeSnapshot
where
    F: Fn(u32) -> LeafSnapshot,
{
    map_node(&window.root, make_leaf)
}

/// Convert a single tmux layout node into a Warp pane-node snapshot.
pub fn map_node<F>(node: &LayoutNode, make_leaf: &F) -> PaneNodeSnapshot
where
    F: Fn(u32) -> LeafSnapshot,
{
    match node {
        LayoutNode::Leaf { pane, .. } => PaneNodeSnapshot::Leaf(make_leaf(*pane)),
        LayoutNode::Split { dir, children, .. } => PaneNodeSnapshot::Branch(BranchSnapshot {
            direction: map_split_direction(*dir),
            children: children
                .iter()
                .map(|child| (child_flex(child, *dir), map_node(child, make_leaf)))
                .collect(),
        }),
    }
}

/// A child's flex weight: its extent along the parent's split axis.
fn child_flex(child: &LayoutNode, dir: SplitDir) -> PaneFlex {
    let (w, h) = node_extent(child);
    let extent = match dir {
        SplitDir::Horizontal => w,
        SplitDir::Vertical => h,
    };
    PaneFlex(extent as f32)
}

fn node_extent(node: &LayoutNode) -> (u32, u32) {
    match node {
        LayoutNode::Leaf { w, h, .. } | LayoutNode::Split { w, h, .. } => (*w, *h),
    }
}

/// A tmux window layout whose leaves have been paired with an adopted value of
/// type `T` (e.g. a pane's pre-built manager+view). Mirrors the shape of
/// [`PaneNodeSnapshot`] but is generic over the leaf payload, so the adoption
/// path can carry live handles a serializable snapshot cannot.
#[derive(Debug)]
pub enum TmuxPaneTree<T> {
    Leaf(T),
    Branch {
        direction: SplitDirection,
        children: Vec<(PaneFlex, TmuxPaneTree<T>)>,
    },
}

/// Generic sibling of [`map_window`]: build a [`TmuxPaneTree`] whose leaves are
/// produced by `make_leaf`, carrying the same split-direction and flex mapping.
///
/// `make_leaf` is `FnMut` (unlike [`map_window`]'s `Fn`) so a caller may create
/// one pane manager per leaf — a side-effecting op that mutates the app context.
/// It is invoked in tmux's left-to-right, depth-first pane order.
pub fn map_window_to<T>(window: &Window, make_leaf: &mut impl FnMut(u32) -> T) -> TmuxPaneTree<T> {
    map_node_to(&window.root, make_leaf)
}

fn map_node_to<T>(node: &LayoutNode, make_leaf: &mut impl FnMut(u32) -> T) -> TmuxPaneTree<T> {
    match node {
        LayoutNode::Leaf { pane, .. } => TmuxPaneTree::Leaf(make_leaf(*pane)),
        LayoutNode::Split { dir, children, .. } => {
            let mut mapped = Vec::with_capacity(children.len());
            for child in children {
                let flex = child_flex(child, *dir);
                mapped.push((flex, map_node_to(child, make_leaf)));
            }
            TmuxPaneTree::Branch {
                direction: map_split_direction(*dir),
                children: mapped,
            }
        }
    }
}

#[cfg(test)]
#[path = "layout_map_tests.rs"]
mod tests;
