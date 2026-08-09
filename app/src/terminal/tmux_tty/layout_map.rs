//! Maps a parsed tmux window layout into Warp's serialized pane-tree snapshot.
//!
//! tmux `{}` splits are left-right and `[]` splits are top-bottom; Warp's
//! `SplitDirection::Horizontal` is likewise left-right and `Vertical` is
//! top-bottom, so the two enums map straight across. Each child's flex weight
//! is its extent along the parent's split axis (width for a left-right split,
//! height for a top-bottom split), which are the relative weights Warp's pane
//! tree expects.

use warp_terminal::tmux::layout::{LayoutNode, SplitDir, Window};

use super::lifecycle::pane_ids;
use crate::app_state::{BranchSnapshot, LeafSnapshot, PaneFlex, PaneNodeSnapshot, SplitDirection};
use crate::pane_group::Direction;

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
/// `make_leaf` receives the pane id plus the columns and rows tmux laid that
/// pane out at, so the adopted pane's grid can start at tmux's geometry instead
/// of a size Warp invented.
pub fn map_window_to<T>(
    window: &Window,
    make_leaf: &mut impl FnMut(u32, u32, u32) -> T,
) -> TmuxPaneTree<T> {
    map_node_to(&window.root, make_leaf)
}

fn map_node_to<T>(
    node: &LayoutNode,
    make_leaf: &mut impl FnMut(u32, u32, u32) -> T,
) -> TmuxPaneTree<T> {
    match node {
        LayoutNode::Leaf { pane, w, h, .. } => TmuxPaneTree::Leaf(make_leaf(*pane, *w, *h)),
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

/// Where a pane belongs relative to a pane Warp has already adopted: the pane
/// to split off, and the side the new pane lands on.
#[derive(Debug, Clone, Copy)]
pub struct PanePlacement {
    pub base: u32,
    pub direction: Direction,
}

/// Work out how to splice `pane` into the tab Warp already renders for this
/// window, given `is_adopted` naming the panes that tab holds.
///
/// Walks outwards from the split `pane` sits in, so a pane added inside a
/// freshly created nested split still finds an adopted neighbour to attach to.
/// Returns `None` when no adopted pane remains to split off.
pub fn placement_for_pane(
    window: &Window,
    pane: u32,
    is_adopted: &impl Fn(u32) -> bool,
) -> Option<PanePlacement> {
    let mut path = Vec::new();
    if !find_split_path(&window.root, pane, &mut path) {
        return None;
    }
    for (split, idx) in path {
        let LayoutNode::Split { dir, children, .. } = split else {
            continue;
        };
        // Prefer a neighbour ahead of the pane so the new pane keeps the side
        // tmux put it on; fall back to one after it.
        for sibling in children[..idx].iter().rev() {
            if let Some(base) = pane_ids(sibling).into_iter().rev().find(|p| is_adopted(*p)) {
                return Some(PanePlacement {
                    base,
                    direction: side(*dir, true),
                });
            }
        }
        for sibling in &children[idx + 1..] {
            if let Some(base) = pane_ids(sibling).into_iter().find(|p| is_adopted(*p)) {
                return Some(PanePlacement {
                    base,
                    direction: side(*dir, false),
                });
            }
        }
    }
    None
}

/// The side of a split axis a pane lands on: `after` is right of a left-right
/// split and below a top-bottom one.
fn side(dir: SplitDir, after: bool) -> Direction {
    match (dir, after) {
        (SplitDir::Horizontal, true) => Direction::Right,
        (SplitDir::Horizontal, false) => Direction::Left,
        (SplitDir::Vertical, true) => Direction::Down,
        (SplitDir::Vertical, false) => Direction::Up,
    }
}

/// Push the splits enclosing `pane` onto `path`, innermost first, each paired
/// with the index of the child the pane lives under.
fn find_split_path<'a>(
    node: &'a LayoutNode,
    pane: u32,
    path: &mut Vec<(&'a LayoutNode, usize)>,
) -> bool {
    match node {
        LayoutNode::Leaf { pane: id, .. } => *id == pane,
        LayoutNode::Split { children, .. } => {
            for (idx, child) in children.iter().enumerate() {
                if find_split_path(child, pane, path) {
                    path.push((node, idx));
                    return true;
                }
            }
            false
        }
    }
}

/// The columns and rows tmux laid `pane` out at, or `None` when the window's
/// layout does not contain it.
pub fn pane_grid_size(window: &Window, pane: u32) -> Option<(u32, u32)> {
    fn find(node: &LayoutNode, pane: u32) -> Option<(u32, u32)> {
        match node {
            LayoutNode::Leaf { pane: id, w, h, .. } => (*id == pane).then_some((*w, *h)),
            LayoutNode::Split { children, .. } => children.iter().find_map(|c| find(c, pane)),
        }
    }
    find(&window.root, pane)
}

#[cfg(test)]
#[path = "layout_map_tests.rs"]
mod tests;
