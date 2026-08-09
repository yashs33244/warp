//! Fans tmux control-mode output out to per-pane terminal models.

use std::collections::HashMap;
use std::io;
use std::sync::Arc;

use parking_lot::FairMutex;
use warp_terminal::tmux::notification::{ControlMessage, ControlParser, TmuxEvent};

use crate::terminal::TerminalModel;
use crate::terminal::event_listener::ChannelEventListener;
use crate::terminal::model::ansi::Processor;

/// The write end of a single tmux pane: an ANSI [`Processor`] feeding a
/// [`TerminalModel`], plus the wakeup channel that repaints its view.
pub struct TmuxPaneSink {
    model: Arc<FairMutex<TerminalModel>>,
    parser: Processor,
    events: ChannelEventListener,
}

impl TmuxPaneSink {
    pub fn new(model: Arc<FairMutex<TerminalModel>>, events: ChannelEventListener) -> Self {
        Self {
            model,
            parser: Processor::new(),
            events,
        }
    }

    /// Feed already-unescaped `%output` bytes into the pane's model (the same
    /// `ansi::Processor::parse_bytes` path `remote_tty` uses), then wake the
    /// view so it repaints.
    pub fn feed_output(&mut self, data: &[u8]) {
        {
            let mut model = self.model.lock();
            // TODO(PR3-responses): the emulator's query replies (DSR/DA,
            // cursor-position) currently go to `io::sink()` and are dropped;
            // route them to the pane's input sender via `send-keys`, or
            // full-screen apps that query the terminal will hang.
            self.parser.parse_bytes(&mut *model, data, &mut io::sink());
        }
        self.events.send_wakeup_event();
    }
}

/// Owns the control-line parser and the map of tmux pane id to its sink, and
/// routes each `%output` notification to the matching pane.
#[derive(Default)]
pub struct TmuxSession {
    control: ControlParser,
    line_buf: Vec<u8>,
    panes: HashMap<u32, TmuxPaneSink>,
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
        self.panes.remove(&pane)
    }

    /// Feed a chunk of the control stream. Bytes are buffered until a full line
    /// is available, then classified and dispatched.
    pub fn feed_control_bytes(&mut self, bytes: &[u8]) {
        self.line_buf.extend_from_slice(bytes);
        for line in drain_complete_lines(&mut self.line_buf) {
            if let Some(message) = self.control.feed_line(&line) {
                self.handle_message(message);
            }
        }
    }

    fn handle_message(&mut self, message: ControlMessage) {
        // TODO(PR2-events): also handle %layout-change, %window-add/close,
        // %exit, and command replies to drive tab/pane lifecycle.
        if let ControlMessage::Notification(TmuxEvent::Output { pane, data }) = message
            && let Some(sink) = self.panes.get_mut(&pane)
        {
            sink.feed_output(&data);
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
