//! Per-pane flow-control bookkeeping for tmux control mode.
//!
//! With `refresh-client -f pause-after=N` (sent in [`super::attach`]) tmux pauses
//! a pane whose control client falls more than `N` seconds behind, emitting
//! `%pause %<pane>`; the client resumes it with `refresh-client -A
//! '%<pane>:continue'` and tmux replays the backlog. This module is the pure,
//! context-free half of that dance: it tracks which panes are paused and holds a
//! bounded per-pane buffer for any `%output` that races in while a pane is
//! paused, so one flooding pane can never grow an unbounded backlog or stall the
//! others. The decision to actually send commands lives in
//! [`session`](super::session) / [`event_loop`](super::event_loop).

use std::collections::{HashMap, HashSet, VecDeque};

/// Cap on bytes buffered for a single paused pane. Once exceeded, the oldest
/// buffered chunks are dropped so a runaway pane cannot exhaust memory. tmux
/// itself replays the authoritative backlog via `%extended-output` on resume,
/// so a dropped client-side chunk is a redraw hint at worst, never lost state.
pub const MAX_PANE_BUFFER_BYTES: usize = 1 << 20; // 1 MiB

/// Where a chunk of `%output` should go, given a pane's pause state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputRoute {
    /// Pane is live: feed the bytes straight to its sink.
    Deliver,
    /// Pane is paused: the bytes were buffered (subject to the byte cap) and
    /// must not be delivered until the pane continues.
    Buffered,
}

#[derive(Default)]
struct PaneBuffer {
    chunks: VecDeque<Vec<u8>>,
    bytes: usize,
}

impl PaneBuffer {
    fn push(&mut self, data: &[u8]) {
        self.chunks.push_back(data.to_vec());
        self.bytes += data.len();
        while self.bytes > MAX_PANE_BUFFER_BYTES {
            match self.chunks.pop_front() {
                Some(dropped) => self.bytes -= dropped.len(),
                None => break,
            }
        }
    }
}

/// Tracks paused panes and their bounded output backlogs.
#[derive(Default)]
pub struct FlowControl {
    paused: HashSet<u32>,
    buffers: HashMap<u32, PaneBuffer>,
}

impl FlowControl {
    pub fn is_paused(&self, pane: u32) -> bool {
        self.paused.contains(&pane)
    }

    /// Mark a pane paused. Returns `true` if it was not already paused.
    pub fn pause(&mut self, pane: u32) -> bool {
        self.paused.insert(pane)
    }

    /// Route an output chunk: deliver it immediately when the pane is live, or
    /// buffer it (bounded) while the pane is paused.
    pub fn route_output(&mut self, pane: u32, data: &[u8]) -> OutputRoute {
        if !self.paused.contains(&pane) {
            return OutputRoute::Deliver;
        }
        self.buffers.entry(pane).or_default().push(data);
        OutputRoute::Buffered
    }

    /// Resume a pane, clearing its paused flag and returning any buffered chunks
    /// in arrival order (oldest first) for the caller to flush to the sink.
    pub fn resume(&mut self, pane: u32) -> Vec<Vec<u8>> {
        self.paused.remove(&pane);
        self.buffers
            .remove(&pane)
            .map(|buffer| Vec::from(buffer.chunks))
            .unwrap_or_default()
    }

    /// Drop all flow-control state for a pane tmux has closed.
    pub fn forget(&mut self, pane: u32) {
        self.paused.remove(&pane);
        self.buffers.remove(&pane);
    }
}

#[cfg(test)]
#[path = "flow_control_tests.rs"]
mod tests;
