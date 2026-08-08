//! Input path for a single tmux pane.
//!
//! Implements [`EventLoopSender`] so a `TerminalView`'s input and resize
//! `Message`s become tmux control commands, which are forwarded to the control
//! stream's write half.

use warp_terminal::tmux::command;

use crate::terminal::writeable_pty::Message;
use crate::terminal::writeable_pty::pty_controller::{EventLoopSendError, EventLoopSender};

/// Turns a pane's `TerminalView` messages into control-mode command bytes.
pub struct TmuxInputSender {
    pane: u32,
    control_tx: async_channel::Sender<Vec<u8>>,
}

impl TmuxInputSender {
    pub fn new(pane: u32, control_tx: async_channel::Sender<Vec<u8>>) -> Self {
        Self { pane, control_tx }
    }

    /// Command bytes for a message, or `None` when it produces nothing to send
    /// (empty input, or a lifecycle message with no tmux equivalent).
    fn command_for(&self, message: Message) -> Option<Vec<u8>> {
        match message {
            // Every keystroke goes through `send-keys -H` (hex), the only input
            // path that can never inject a literal newline into the payload.
            Message::Input(bytes) => command::send_keys_hex(self.pane, &bytes).ok(),
            // A resize maps to the control client's size; tmux re-lays the panes
            // to fit. `command::resize_pane` is available for per-pane sizing.
            Message::Resize(size) => Some(command::refresh_client_size(
                size.columns() as u32,
                size.rows() as u32,
            )),
            Message::Shutdown | Message::ChildExited => None,
        }
    }
}

impl EventLoopSender for TmuxInputSender {
    fn send(&self, message: Message) -> Result<(), EventLoopSendError> {
        let Some(bytes) = self.command_for(message) else {
            return Ok(());
        };
        self.control_tx.try_send(bytes).map_err(|err| match err {
            async_channel::TrySendError::Closed(_) => EventLoopSendError::Disconnected,
            async_channel::TrySendError::Full(_) => EventLoopSendError::Other(err.into()),
        })
    }
}

/// Sends a pane's emulator-generated replies (DSR/DA cursor-position answers and
/// other query responses) back to tmux as `send-keys` input, so full-screen apps
/// that interrogate the terminal receive their answer instead of hanging.
///
/// Separate from [`TmuxInputSender`] because a sink emits raw response bytes
/// rather than [`Message`]s, and the two live on opposite sides of the control
/// stream (input path vs. output path) while sharing the same write channel.
pub struct TmuxResponder {
    pane: u32,
    control_tx: async_channel::Sender<Vec<u8>>,
}

impl TmuxResponder {
    pub fn new(pane: u32, control_tx: async_channel::Sender<Vec<u8>>) -> Self {
        Self { pane, control_tx }
    }

    /// Forward `bytes` (the emulator's reply) to the pane via `send-keys -H`.
    /// Empty replies and a closed control stream are silently ignored — a
    /// dropped response only costs a redraw, never correctness.
    pub fn send_response(&self, bytes: &[u8]) {
        if let Ok(command) = command::send_keys_hex(self.pane, bytes) {
            let _ = self.control_tx.try_send(command);
        }
    }
}

#[cfg(test)]
#[path = "input_tests.rs"]
mod tests;
