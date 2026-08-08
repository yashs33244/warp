//! Fans tmux control-mode output out to per-pane terminal models.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::FairMutex;
use warp_terminal::tmux::layout::{Window, parse_layout};
use warp_terminal::tmux::notification::{CommandReply, ControlMessage, ControlParser, TmuxEvent};

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
    /// Returns whether the pane's shell just revealed itself as
    /// zombie-warpified (a stale-session Precmd), i.e. it needs re-warpifying.
    pub fn feed_output(&mut self, data: &[u8]) -> bool {
        let mut responses = Vec::new();
        let stale_precmd;
        {
            let mut model = self.model.lock();
            self.parser.parse_bytes(&mut *model, data, &mut responses);
            stale_precmd = model.take_adopted_stale_precmd();
        }
        if !responses.is_empty()
            && let Some(responder) = &self.responder
        {
            responder.send_response(&responses);
        }
        self.events.send_wakeup_event();
        stale_precmd
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
    /// A not-yet-warpified pane's shell reported a stale-session Precmd: it was
    /// warpified in an earlier attach and is back at its prompt, so the caller
    /// should re-run the warpify bootstrap now.
    RewarpifyPane { pane: u32 },
    /// The control session ended; the caller tears the whole session down.
    Exit { reason: Option<String> },
}

/// Cap on `%output` buffered for a pane before its sink is registered (its
/// window is adopted). tmux streams the initial prompt before we build the tab,
/// so we hold it here and flush on registration; past the cap the oldest bytes
/// are dropped, losing at worst some initial scrollback, never live state.
const MAX_PENDING_OUTPUT: usize = 256 * 1024;

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
    /// `%output` for a pane whose sink is not registered yet, buffered (bounded)
    /// until [`register_pane`] flushes it. Lets the initial prompt survive the
    /// gap between the first `%output` and the window's adoption.
    pending_output: HashMap<u32, Vec<u8>>,
    /// Whether the one-time control-mode DCS intro (`ESC P 1 0 0 0 p`) has been
    /// stripped from the head of the stream.
    stripped_dcs: bool,
}

impl TmuxSession {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register (or replace) the sink for a tmux pane id, flushing any output
    /// that arrived before the pane was adopted (e.g. the initial prompt).
    pub fn register_pane(&mut self, pane: u32, mut sink: TmuxPaneSink) {
        if let Some(buffered) = self.pending_output.remove(&pane) {
            let _ = sink.feed_output(&buffered);
        }
        self.panes.insert(pane, sink);
    }

    /// Drop a pane that tmux has closed.
    pub fn remove_pane(&mut self, pane: u32) -> Option<TmuxPaneSink> {
        self.flow.forget(pane);
        self.pending_output.remove(&pane);
        self.panes.remove(&pane)
    }

    /// Feed a chunk of the control stream. Bytes are buffered until a full line
    /// is available, then classified and dispatched. `%output` is routed to its
    /// pane sink internally; window/pane lifecycle notifications are returned as
    /// [`LifecycleEffect`]s for the caller to apply to the UI.
    #[must_use]
    pub fn feed_control_bytes(&mut self, bytes: &[u8]) -> Vec<LifecycleEffect> {
        self.line_buf.extend_from_slice(bytes);
        self.strip_dcs_intro();
        let mut effects = Vec::new();
        for line in drain_complete_lines(&mut self.line_buf) {
            if let Some(message) = self.control.feed_line(&line) {
                self.handle_message(message, &mut effects);
            }
        }
        effects
    }

    /// Strip the one-time DCS intro (`ESC P 1 0 0 0 p`) tmux writes before the
    /// first `%begin`, so that first control line parses. Once enough bytes have
    /// arrived to confirm the prefix is absent, stop looking.
    fn strip_dcs_intro(&mut self) {
        const DCS: &[u8] = b"\x1bP1000p";
        if self.stripped_dcs {
            return;
        }
        if self.line_buf.starts_with(DCS) {
            self.line_buf.drain(0..DCS.len());
            self.stripped_dcs = true;
        } else if self.line_buf.len() >= DCS.len() || self.line_buf.contains(&b'\n') {
            self.stripped_dcs = true;
        }
    }

    fn handle_message(&mut self, message: ControlMessage, effects: &mut Vec<LifecycleEffect>) {
        match message {
            ControlMessage::Reply(reply) => self.handle_reply(&reply, effects),
            ControlMessage::Notification(event) => {
                if let Some(effect) = self.handle_event(event) {
                    effects.push(effect);
                }
            }
        }
    }

    /// A `list-windows` reply carries one `@<id> <layout>` line per window; turn
    /// each into a `WindowLayout` effect so the caller adopts (or updates) the
    /// tab. Empty-bodied replies (send-keys/refresh-client) and failures yield
    /// nothing.
    fn handle_reply(&mut self, reply: &CommandReply, effects: &mut Vec<LifecycleEffect>) {
        if !reply.success {
            return;
        }
        for line in &reply.body {
            if let Some((window, layout)) = parse_window_layout_line(line)
                && let Some(effect) = self.on_layout_change(window, &layout)
            {
                effects.push(effect);
            }
        }
    }

    fn handle_event(&mut self, event: TmuxEvent) -> Option<LifecycleEffect> {
        match event {
            TmuxEvent::Output { pane, data } => {
                // A paused pane's output is buffered (bounded) rather than
                // delivered, so a flooding pane can't stall the others.
                match self.flow.route_output(pane, &data) {
                    OutputRoute::Deliver => self
                        .deliver_output(pane, &data)
                        .then_some(LifecycleEffect::RewarpifyPane { pane }),
                    OutputRoute::Buffered => None,
                }
            }
            TmuxEvent::ExtendedOutput { pane, data, .. } => {
                // tmux replays a resumed pane's backlog as `%extended-output`
                // (carrying an age we don't render); deliver it like `%output`.
                self.deliver_output(pane, &data)
                    .then_some(LifecycleEffect::RewarpifyPane { pane })
            }
            TmuxEvent::Pause { pane } => {
                self.flow.pause(pane);
                Some(LifecycleEffect::ResumePane { pane })
            }
            TmuxEvent::Continue { pane } => {
                let mut stale = false;
                for chunk in self.flow.resume(pane) {
                    if let Some(sink) = self.panes.get_mut(&pane) {
                        stale |= sink.feed_output(&chunk);
                    }
                }
                stale.then_some(LifecycleEffect::RewarpifyPane { pane })
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
                self.pending_output.clear();
                self.flow = FlowControl::default();
                Some(LifecycleEffect::Exit { reason })
            }
            _ => None,
        }
    }

    /// Feed output to a pane's sink, or buffer it (bounded) when the pane's sink
    /// is not registered yet, so output that races ahead of adoption is not lost.
    fn deliver_output(&mut self, pane: u32, data: &[u8]) -> bool {
        if let Some(sink) = self.panes.get_mut(&pane) {
            return sink.feed_output(data);
        }
        let buf = self.pending_output.entry(pane).or_default();
        buf.extend_from_slice(data);
        if buf.len() > MAX_PENDING_OUTPUT {
            let overflow = buf.len() - MAX_PENDING_OUTPUT;
            buf.drain(0..overflow);
        }
        false
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
            self.pending_output.remove(pane);
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
                self.pending_output.remove(&pane);
            }
        }
    }
}

/// Parse a `list-windows -F '#{window_id} #{window_layout}'` reply line,
/// `@<id> <layout>`, into `(window_id, layout)`. Returns `None` for any line not
/// of that exact shape (e.g. a blank line or a non-`@` first token).
fn parse_window_layout_line(line: &[u8]) -> Option<(u32, String)> {
    let space = line.iter().position(|&b| b == b' ')?;
    let id_digits = line[..space].strip_prefix(b"@")?;
    if id_digits.is_empty() || !id_digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let window: u32 = std::str::from_utf8(id_digits).ok()?.parse().ok()?;
    let layout = std::str::from_utf8(&line[space + 1..]).ok()?.trim();
    if layout.is_empty() {
        return None;
    }
    Some((window, layout.to_string()))
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
