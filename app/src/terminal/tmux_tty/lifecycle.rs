//! Pure window/pane lifecycle logic for tmux control mode.
//!
//! These helpers are context-free so the lifecycle decisions — which panes a
//! window layout contains, and how a window's pane set changed between two
//! layouts — stay unit-testable without an `AppContext` or a running tmux
//! process. [`session`](super::session) tracks the per-window pane sets and
//! feeds them through these functions.

use warp_terminal::tmux::layout::LayoutNode;

/// Collect every tmux pane id in a parsed window layout, left-to-right and
/// depth-first (the order tmux emits panes within a `#{window_layout}`).
pub fn pane_ids(node: &LayoutNode) -> Vec<u32> {
    let mut ids = Vec::new();
    collect_pane_ids(node, &mut ids);
    ids
}

fn collect_pane_ids(node: &LayoutNode, out: &mut Vec<u32>) {
    match node {
        LayoutNode::Leaf { pane, .. } => out.push(*pane),
        LayoutNode::Split { children, .. } => {
            for child in children {
                collect_pane_ids(child, out);
            }
        }
    }
}

/// The panes added and removed when a window's layout changes from one set of
/// pane ids to another.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LayoutDelta {
    /// Panes in the new layout that were not in the old one (tmux split in a
    /// new pane); a caller must create a model for each.
    pub added: Vec<u32>,
    /// Panes in the old layout that are gone from the new one (tmux closed a
    /// pane); a caller must tear each down.
    pub removed: Vec<u32>,
}

impl LayoutDelta {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty()
    }
}

/// Compute the panes added and removed going from `old` to `new`. `added`
/// follows `new` order and `removed` follows `old` order.
pub fn diff_pane_ids(old: &[u32], new: &[u32]) -> LayoutDelta {
    let added = new.iter().copied().filter(|id| !old.contains(id)).collect();
    let removed = old.iter().copied().filter(|id| !new.contains(id)).collect();
    LayoutDelta { added, removed }
}

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod tests;
