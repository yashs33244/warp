//! Fans tmux control-mode output out to per-pane terminal models.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::FairMutex;
use warp_terminal::tmux::layout::{Window, parse_layout};
use warp_terminal::tmux::notification::{ControlMessage, ControlParser, TmuxEvent};

use super::flow_control::{FlowControl, OutputRoute};
use super::input::TmuxResponder;
use super::lifecycle::{LayoutDelta, diff_pane_ids, pane_ids};
use crate::terminal::TerminalModel;
use crate::terminal::event_listener::ChannelEventListener;
use crate::terminal::model::ansi::Processor;

/// The write end of a single tmux pane: an ANSI [`Processor`] feeding a
/// [`TerminalModel`], plus the wakeup channel that repaints its view.
pub struct TmuxPaneSink {
    model: Arc<FairMutex<TerminalModel>>,
    parser: Processor,
    events: ChannelEventListener,
    /// Routes the emulator's query replies back to tmux via `send-keys`. `None`
    /// until the adoption path wires the pane's control write channel in.
    responder: Option<TmuxResponder>,
}

impl TmuxPaneSink {
    pub fn new(model: Arc<FairMutex<TerminalModel>>, events: ChannelEventListener) -> Self {
        Self {
            model,
            parser: Processor::new(),
            events,
            responder: None,
        }
    }

    /// Attach the responder that forwards this pane's emulator replies to tmux.
    pub fn with_responder(mut self, responder: TmuxResponder) -> Self {
        self.responder = Some(responder);
        self
    }

    /// Feed already-unescaped `%output` bytes into the pane's model (the same
    /// `ansi::Processor::parse_bytes` path `remote_tty` uses), then wake the
    /// view so it repaints. Any reply the emulator generates (DSR/DA,
    /// cursor-position) is routed back to tmux via the responder so full-screen
    /// apps that query the terminal don't hang.
    pub fn feed_output(&mut self, data: &[u8]) {
        let mut responses = Vec::new();
        {
            let mut model = self.model.lock();
            self.parser.parse_bytes(&mut *model, data, &mut responses);
        }
        if !responses.is_empty()
            && let Some(responder) = &self.responder
        {
            responder.send_response(&responses);
        }
        self.events.send_wakeup_event();
    }
}

/// A window/pane lifecycle decision the control stream produced that must be
/// applied to the workspace UI.
///
/// [`TmuxSession`] performs the context-free bookkeeping itself — tracking each
/// window's pane set and dropping the sinks of panes tmux has closed — but
/// applying these to tabs and panes needs an `AppContext` the session does not
/// hold, so it hands them back to the caller (the event loop) to apply.
#[derive(Debug, Clone, PartialEq)]
pub enum LifecycleEffect {
    /// A window's layout changed. `layout` is the freshly parsed tree and
    /// `delta` is its pane set relative to the previously known one; the caller
    /// recomputes the tab's split tree, creates models for `delta.added`, and
    /// tears down `delta.removed`.
    WindowLayout {
        window: u32,
        layout: Window,
        delta: LayoutDelta,
    },
    /// tmux created a new window; the caller adds a tab for it.
    WindowAdded { window: u32 },
    /// tmux closed a window; the caller removes its tab.
    WindowClosed { window: u32 },
    /// tmux renamed a window; the caller retitles its tab.
    WindowRenamed { window: u32, name: String },
    /// A pane was paused by tmux flow control (`%pause`); the caller resumes it
    /// with `refresh-client -A '%<pane>:continue'` once it is ready for more
    /// output. Buffered backlog is flushed when the matching `%continue` lands.
    ResumePane { pane: u32 },
    /// The control session ended; the caller tears the whole session down.
    Exit { reason: Option<String> },
}

/// Owns the control-line parser and the map of tmux pane id to its sink, routes
/// each `%output` notification to the matching pane, and turns window/pane
/// notifications into [`LifecycleEffect`]s.
#[derive(Default)]
pub struct TmuxSession {
    control: ControlParser,
    line_buf: Vec<u8>,
    panes: HashMap<u32, TmuxPaneSink>,
    /// Known pane ids per window, tracked from `%layout-change` so the next
    /// change to the same window can be diffed into added/removed panes.
    window_panes: HashMap<u32, Vec<u32>>,
    /// Per-pane pause state and bounded backlog for tmux flow control.
    flow: FlowControl,
}

impl TmuxSession {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register (or replace) the sink for a tmux pane id.
    pub fn register_pane(&mut self, pane: u32, sink: TmuxPaneSink) {
        self.panes.insert(pane, sink);
    }

    /// Drop a pane that tmux has closed.
    pub fn remove_pane(&mut self, pane: u32) -> Option<TmuxPaneSink> {
        self.flow.forget(pane);
        self.panes.remove(&pane)
    }

    /// Feed a chunk of the control stream. Bytes are buffered until a full line
    /// is available, then classified and dispatched. `%output` is routed to its
    /// pane sink internally; window/pane lifecycle notifications are returned as
    /// [`LifecycleEffect`]s for the caller to apply to the UI.
    #[must_use]
    pub fn feed_control_bytes(&mut self, bytes: &[u8]) -> Vec<LifecycleEffect> {
        self.line_buf.extend_from_slice(bytes);
        let mut effects = Vec::new();
        for line in drain_complete_lines(&mut self.line_buf) {
            if let Some(message) = self.control.feed_line(&line)
                && let Some(effect) = self.handle_message(message)
            {
                effects.push(effect);
            }
        }
        effects
    }

    fn handle_message(&mut self, message: ControlMessage) -> Option<LifecycleEffect> {
        // Command replies (%begin/%end blocks) are not consumed yet; PR3's
        // bootstrap will match the `list-windows` reply here to seed the first
        // tab. TODO(PR4-bootstrap): route CommandReply to the pending command.
        let ControlMessage::Notification(event) = message else {
            return None;
        };
        match event {
            TmuxEvent::Output { pane, data } => {
                // A paused pane's output is buffered (bounded) rather than
                // delivered, so a flooding pane can't stall the others.
                match self.flow.route_output(pane, &data) {
                    OutputRoute::Deliver => {
                        if let Some(sink) = self.panes.get_mut(&pane) {
                            sink.feed_output(&data);
                        }
                    }
                    OutputRoute::Buffered => {}
                }
                None
            }
            TmuxEvent::ExtendedOutput { pane, data, .. } => {
                // tmux replays a resumed pane's backlog as `%extended-output`
                // (carrying an age we don't render); deliver it like `%output`.
                if let Some(sink) = self.panes.get_mut(&pane) {
                    sink.feed_output(&data);
                }
                None
            }
            TmuxEvent::Pause { pane } => {
                self.flow.pause(pane);
                Some(LifecycleEffect::ResumePane { pane })
            }
            TmuxEvent::Continue { pane } => {
                for chunk in self.flow.resume(pane) {
                    if let Some(sink) = self.panes.get_mut(&pane) {
                        sink.feed_output(&chunk);
                    }
                }
                None
            }
            TmuxEvent::LayoutChange { window, layout, .. } => {
                self.on_layout_change(window, &layout)
            }
            TmuxEvent::WindowAdd { window } => Some(LifecycleEffect::WindowAdded { window }),
            TmuxEvent::WindowClose { window } => {
                self.forget_window(window);
                Some(LifecycleEffect::WindowClosed { window })
            }
            TmuxEvent::WindowRenamed { window, name } => {
                Some(LifecycleEffect::WindowRenamed { window, name })
            }
            TmuxEvent::Exit { reason } => {
                self.panes.clear();
                self.window_panes.clear();
                self.flow = FlowControl::default();
                Some(LifecycleEffect::Exit { reason })
            }
            _ => None,
        }
    }

    /// Parse a `%layout-change` layout, diff it against the window's last known
    /// pane set, drop the sinks of removed panes, and record the new set.
    ///
    /// Sinks for `delta.added` panes are *not* created here: a sink wraps a
    /// [`TerminalModel`], whose creation needs an `AppContext`. That is deferred
    /// to the caller via [`LifecycleEffect::WindowLayout`].
    fn on_layout_change(&mut self, window: u32, layout: &str) -> Option<LifecycleEffect> {
        // A malformed / checksum-mismatched layout is dropped rather than
        // mutating tracked state on garbage input.
        let parsed = parse_layout(layout).ok()?;
        let new_ids = pane_ids(&parsed.root);
        let old_ids = self
            .window_panes
            .get(&window)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let delta = diff_pane_ids(old_ids, &new_ids);
        for pane in &delta.removed {
            self.panes.remove(pane);
            self.flow.forget(*pane);
        }
        self.window_panes.insert(window, new_ids);
        Some(LifecycleEffect::WindowLayout {
            window,
            layout: parsed,
            delta,
        })
    }

    /// Forget a closed window: drop the sinks of every pane it owned and remove
    /// its tracked pane set.
    fn forget_window(&mut self, window: u32) {
        if let Some(ids) = self.window_panes.remove(&window) {
            for pane in ids {
                self.panes.remove(&pane);
                self.flow.forget(pane);
            }
        }
    }
}

/// Split off every complete (newline-terminated) line from `buf`, leaving any
/// trailing partial line buffered. A trailing `\r` is stripped from each line.
fn drain_complete_lines(buf: &mut Vec<u8>) -> Vec<Vec<u8>> {
    let mut lines = Vec::new();
    let mut start = 0;
    for i in 0..buf.len() {
        if buf[i] == b'\n' {
            let mut end = i;
            if end > start && buf[end - 1] == b'\r' {
                end -= 1;
            }
            lines.push(buf[start..end].to_vec());
            start = i + 1;
        }
    }
    if start > 0 {
        buf.drain(0..start);
    }
    lines
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
