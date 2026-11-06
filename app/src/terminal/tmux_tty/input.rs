//! Input path for a single tmux pane.
//!
//! Implements [`EventLoopSender`] so a `TerminalView`'s input and resize
//! `Message`s become tmux control commands, which are forwarded to the control
//! stream's write half.

use warp_terminal::tmux::command;

use super::geometry::TmuxGeometry;
use crate::terminal::writeable_pty::Message;
use crate::terminal::writeable_pty::pty_controller::{EventLoopSendError, EventLoopSender};

/// Keys per `send-keys`. tmux rejects an over-long command outright
/// (empirically somewhere past 4 KB of hex on 3.5a), and each key costs up to
/// seven characters of hex, so 512 keys per command stays comfortably inside
/// the limit.
const MAX_SEND_KEYS_KEYS: usize = 512;

/// Bounds on a grid we will ask tmux for, so a degenerate pixel size cannot turn
/// into an absurd window.
const MIN_GRID: u32 = 1;
const MAX_GRID: u32 = 2000;

/// Turns a pane's `TerminalView` messages into control-mode command bytes.
pub struct TmuxInputSender {
    window: u32,
    pane: u32,
    /// Shared with the control-stream reader, which keeps it current from
    /// tmux's layout notifications.
    geometry: TmuxGeometry,
    control_tx: async_channel::Sender<Vec<u8>>,
}

impl TmuxInputSender {
    pub fn new(
        window: u32,
        pane: u32,
        geometry: TmuxGeometry,
        control_tx: async_channel::Sender<Vec<u8>>,
    ) -> Self {
        Self {
            window,
            pane,
            geometry,
            control_tx,
        }
    }

    /// Commands for a message; empty when it produces nothing to send (empty
    /// input, or a lifecycle message with no tmux equivalent).
    fn commands_for(&self, message: Message) -> Vec<Vec<u8>> {
        match message {
            // Every keystroke goes through `send-keys -H` (hex), the only input
            // path that can never inject a literal newline into the payload.
            // Decode to Unicode code points first: `-H` takes key values, not
            // UTF-8 bytes, so a multi-byte character has to be sent as one key
            // or it arrives as several wrong characters. Chunk by key (never
            // mid-character) so a large paste is split without corrupting one,
            // since tmux rejects an over-long command outright.
            Message::Input(bytes) => decode_keys(&bytes)
                .chunks(MAX_SEND_KEYS_KEYS)
                .filter_map(|chunk| command::send_keys_codepoints(self.pane, chunk).ok())
                .collect(),
            Message::Resize(size) => self
                .resize_command(size.columns() as u32, size.rows() as u32)
                .into_iter()
                .collect(),
            Message::Shutdown | Message::ShutdownForReplacement | Message::ChildExited => Vec::new(),
        }
    }

    /// Ask tmux to make this pane's *window* the size at which this pane lands
    /// on a `cols` x `rows` grid, keeping the other panes' share of the window.
    /// Without this tmux keeps wrapping output at its own idea of the pane
    /// width, which renders as duplicated and clipped lines.
    ///
    /// Only the per-window form is ever sent: a bare `refresh-client -C` resizes
    /// every window in the session and `resize-pane` re-lays out panes the user
    /// sized themselves, so either one turns "attach" into "overwrite my
    /// layout".
    fn resize_command(&self, cols: u32, rows: u32) -> Option<Vec<u8>> {
        if cols == 0 || rows == 0 {
            return None;
        }
        let (win_cols, win_rows) = match self.geometry.get(self.pane) {
            Some(geometry) if geometry.pane_cols > 0 && geometry.pane_rows > 0 => (
                scale(cols, geometry.win_cols, geometry.pane_cols),
                scale(rows, geometry.win_rows, geometry.pane_rows),
            ),
            // No layout seen yet: the pane is all we know of the window.
            _ => (clamp_grid(cols), clamp_grid(rows)),
        };
        self.geometry
            .should_send(self.window, win_cols, win_rows)
            .then(|| command::refresh_client_window_size(self.window, win_cols, win_rows))
    }
}

/// Decode terminal input bytes into key values (Unicode code points). Valid
/// UTF-8 yields one key per character, so multi-byte input reaches tmux intact;
/// a byte that is not valid UTF-8 is forwarded as its own value rather than
/// dropped, so a raw or partial sequence still gets through.
fn decode_keys(bytes: &[u8]) -> Vec<u32> {
    let mut keys = Vec::with_capacity(bytes.len());
    let mut rest = bytes;
    while !rest.is_empty() {
        match std::str::from_utf8(rest) {
            Ok(text) => {
                keys.extend(text.chars().map(u32::from));
                break;
            }
            Err(err) => {
                let valid = err.valid_up_to();
                if valid > 0 {
                    let text = std::str::from_utf8(&rest[..valid]).expect("valid prefix");
                    keys.extend(text.chars().map(u32::from));
                }
                match err.error_len() {
                    // A malformed sequence of known length: forward its bytes
                    // raw, then resume decoding after them.
                    Some(bad) => {
                        keys.extend(rest[valid..valid + bad].iter().map(|&b| u32::from(b)));
                        rest = &rest[valid + bad..];
                    }
                    // An incomplete trailing sequence: forward its bytes raw and
                    // stop (there is nothing after it to decode).
                    None => {
                        keys.extend(rest[valid..].iter().map(|&b| u32::from(b)));
                        break;
                    }
                }
            }
        }
    }
    keys
}

/// `value * numerator / denominator`, rounded to the nearest cell. Widened to
/// `u64` so no grid can overflow the multiply. `denominator` is non-zero.
fn scale(value: u32, numerator: u32, denominator: u32) -> u32 {
    let denominator = denominator as u64;
    let scaled = (value as u64 * numerator as u64 + denominator / 2) / denominator;
    clamp_grid(scaled.min(u32::MAX as u64) as u32)
}

fn clamp_grid(cells: u32) -> u32 {
    cells.clamp(MIN_GRID, MAX_GRID)
}

impl EventLoopSender for TmuxInputSender {
    fn send(&self, message: Message) -> Result<(), EventLoopSendError> {
        for bytes in self.commands_for(message) {
            self.control_tx.try_send(bytes).map_err(|err| match err {
                async_channel::TrySendError::Closed(_) => EventLoopSendError::Disconnected,
                async_channel::TrySendError::Full(_) => EventLoopSendError::Other(err.into()),
            })?;
        }
        Ok(())
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

/// The tmux commands a single adopted pane can issue on the user's behalf
/// (split, kill, detach, rename), addressed at that pane and its window.
///
/// Every send is best-effort: tmux is the source of truth and re-syncs the tab
/// through its notifications, so a dropped command costs a click, never
/// correctness.
#[derive(Clone)]
pub struct TmuxPaneCommands {
    window: u32,
    pane: u32,
    control_tx: async_channel::Sender<Vec<u8>>,
}

impl TmuxPaneCommands {
    pub fn new(window: u32, pane: u32, control_tx: async_channel::Sender<Vec<u8>>) -> Self {
        Self {
            window,
            pane,
            control_tx,
        }
    }

    pub fn pane(&self) -> u32 {
        self.pane
    }

    /// `split-window -h` (left-right) or `-v` (top-bottom).
    pub fn split(&self, horizontal: bool) {
        self.send(command::split_window(self.pane, horizontal));
    }

    pub fn kill_pane(&self) {
        self.send(command::kill_pane(self.pane));
    }

    pub fn kill_window(&self) {
        self.send(command::kill_window(self.window));
    }

    pub fn detach(&self) {
        self.send(command::detach_client());
    }

    pub fn new_window(&self) {
        self.send(command::new_window());
    }

    pub fn select(&self) {
        self.send(command::select_pane(self.pane));
    }

    /// No-op for a name that is empty once unsafe characters are stripped.
    pub fn rename_window(&self, name: &str) {
        if let Ok(bytes) = command::rename_window(self.window, name) {
            self.send(bytes);
        }
    }

    /// Record in tmux itself that this pane has been warpified, so the choice
    /// survives detaching and outlives the Warp process.
    pub fn mark_warpified(&self) {
        self.send(command::set_pane_warpified(self.pane));
    }

    /// Send an already-serialized control command for this pane's session.
    pub fn send_raw(&self, bytes: Vec<u8>) {
        self.send(bytes);
    }

    fn send(&self, bytes: Vec<u8>) {
        let _ = self.control_tx.try_send(bytes);
    }
}

#[cfg(test)]
#[path = "input_tests.rs"]
mod tests;
