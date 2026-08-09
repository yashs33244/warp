//! The grid geometry tmux last reported, shared between the control-stream
//! reader that learns it and the panes that need it to size themselves.
//!
//! tmux wraps a pane's output at the width *it* laid that pane out at, while
//! Warp derives a pane's grid from its own pixel area. Unless the two agree the
//! text renders wrapped in the wrong places, so a Warp-side pane resize has to
//! travel back to tmux. tmux sizes a control-mode client per *window*
//! (`refresh-client -C @<window>:<w>x<h>`), never per pane, so translating a
//! pane's requested grid into a window size needs both halves of the geometry —
//! which is what this map holds.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use warp_terminal::tmux::layout::{LayoutNode, Window};

/// The grid tmux laid one pane out at, alongside the grid of the window that
/// contains it.
#[derive(Clone, Copy, Debug)]
pub struct PaneGeometry {
    pub window: u32,
    pub pane_cols: u32,
    pub pane_rows: u32,
    pub win_cols: u32,
    pub win_rows: u32,
}

/// Latest tmux geometry per pane, keyed by tmux pane id. Cheap to clone: every
/// pane of a session shares one map.
#[derive(Clone, Default)]
pub struct TmuxGeometry(Arc<Mutex<State>>);

#[derive(Default)]
struct State {
    panes: HashMap<u32, PaneGeometry>,
    /// Last size this client asked each window to become.
    requested: HashMap<u32, (u32, u32)>,
}

impl TmuxGeometry {
    /// Replace everything known about `window` with the layout tmux just
    /// reported for it.
    pub fn record(&self, window: u32, layout: &Window) {
        let (win_cols, win_rows) = node_extent(&layout.root);
        let mut state = self.0.lock();
        state.panes.retain(|_, geometry| geometry.window != window);
        record_leaves(&layout.root, window, win_cols, win_rows, &mut state.panes);
    }

    pub fn get(&self, pane: u32) -> Option<PaneGeometry> {
        self.0.lock().panes.get(&pane).copied()
    }

    /// Whether `(cols, rows)` is a size worth asking `window` to become, marking
    /// it as the outstanding request.
    ///
    /// This is the loop breaker: tmux answers a resize with `%layout-change`,
    /// which can resize Warp's panes, which asks for another resize. Dropping
    /// the repeat of a size already requested stops that ping-pong dead.
    pub fn should_send(&self, window: u32, cols: u32, rows: u32) -> bool {
        let mut state = self.0.lock();
        if state.requested.get(&window) == Some(&(cols, rows)) {
            return false;
        }
        state.requested.insert(window, (cols, rows));
        true
    }
}

fn record_leaves(
    node: &LayoutNode,
    window: u32,
    win_cols: u32,
    win_rows: u32,
    panes: &mut HashMap<u32, PaneGeometry>,
) {
    match node {
        LayoutNode::Leaf { pane, w, h, .. } => {
            panes.insert(
                *pane,
                PaneGeometry {
                    window,
                    pane_cols: *w,
                    pane_rows: *h,
                    win_cols,
                    win_rows,
                },
            );
        }
        LayoutNode::Split { children, .. } => {
            for child in children {
                record_leaves(child, window, win_cols, win_rows, panes);
            }
        }
    }
}

fn node_extent(node: &LayoutNode) -> (u32, u32) {
    match node {
        LayoutNode::Leaf { w, h, .. } | LayoutNode::Split { w, h, .. } => (*w, *h),
    }
}
