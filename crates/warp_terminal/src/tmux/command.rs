//! Outgoing-command serializer for tmux control mode.
//!
//! Every command is a byte buffer ending in EXACTLY one `b"\n"`. Empty or
//! whitespace-only command strings are rejected because a bare newline
//! detaches the tmux session.

use std::fmt::Write as _;

/// tmux command names, kept in one place so the wire spelling is not repeated
/// across builders.
const SEND_KEYS: &str = "send-keys";
const RESIZE_PANE: &str = "resize-pane";
const SPLIT_WINDOW: &str = "split-window";
const KILL_PANE: &str = "kill-pane";
const NEW_WINDOW: &str = "new-window";
const KILL_WINDOW: &str = "kill-window";
const SELECT_LAYOUT: &str = "select-layout";
const REFRESH_CLIENT: &str = "refresh-client";
const CAPTURE_PANE: &str = "capture-pane";
const LIST_WINDOWS: &str = "list-windows";
const DETACH_CLIENT: &str = "detach-client";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandError;

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("empty or whitespace-only tmux command")
    }
}

impl std::error::Error for CommandError {}

/// Serializer for tmux control-mode commands: a namespace grouping every
/// command builder under one type.
pub struct Command;

impl Command {
    /// Serialize a single command line. Trims the input; a command that is empty
    /// after trimming, or that contains any embedded ASCII control byte, is
    /// rejected. Control bytes are refused because a command must occupy exactly
    /// one line: an embedded `\n`/`\r` would smuggle a second command onto the
    /// control stream (and a bare newline detaches the session), so the
    /// one-command-per-line framing must never carry one. The returned buffer is
    /// the trimmed command bytes followed by exactly one `b"\n"`.
    pub fn write_command(cmd: &str) -> Result<Vec<u8>, CommandError> {
        let trimmed = cmd.trim();
        if trimmed.is_empty() {
            return Err(CommandError);
        }
        if trimmed.bytes().any(|b| b.is_ascii_control()) {
            return Err(CommandError);
        }
        let mut out = Vec::with_capacity(trimmed.len() + 1);
        out.extend_from_slice(trimmed.as_bytes());
        out.push(b'\n');
        Ok(out)
    }

    /// Build a command from a string that is statically guaranteed to be
    /// non-empty and non-whitespace. Never panics for the callers below because
    /// each carries a literal non-whitespace prefix.
    fn fixed(cmd: String) -> Vec<u8> {
        match Self::write_command(&cmd) {
            Ok(bytes) => bytes,
            // Unreachable for the callers here, but stay panic-free regardless.
            Err(_) => {
                let mut out = cmd.into_bytes();
                out.push(b'\n');
                out
            }
        }
    }

    /// The ONLY input path. Each byte is emitted as two lowercase hex digits, so
    /// no byte (including `0x0a`) can ever appear literally in the payload.
    pub fn send_keys_hex(pane: u32, bytes: &[u8]) -> Result<Vec<u8>, CommandError> {
        if bytes.is_empty() {
            return Err(CommandError);
        }
        let mut cmd = format!("{SEND_KEYS} -t %{pane} -H");
        for b in bytes {
            // Infallible write into a String.
            let _ = write!(cmd, " {b:02x}");
        }
        Self::write_command(&cmd)
    }

    pub fn resize_pane(pane: u32, w: u32, h: u32) -> Vec<u8> {
        Self::fixed(format!("{RESIZE_PANE} -t %{pane} -x {w} -y {h}"))
    }

    pub fn split_window(pane: u32, horizontal: bool) -> Vec<u8> {
        let dir = if horizontal { " -h" } else { " -v" };
        Self::fixed(format!("{SPLIT_WINDOW} -t %{pane}{dir}"))
    }

    pub fn kill_pane(pane: u32) -> Vec<u8> {
        Self::fixed(format!("{KILL_PANE} -t %{pane}"))
    }

    pub fn new_window() -> Vec<u8> {
        Self::fixed(NEW_WINDOW.to_string())
    }

    pub fn kill_window(window: u32) -> Vec<u8> {
        Self::fixed(format!("{KILL_WINDOW} -t @{window}"))
    }

    pub fn select_layout(window: u32, layout: &str) -> Vec<u8> {
        Self::fixed(format!("{SELECT_LAYOUT} -t @{window} {layout}"))
    }

    pub fn refresh_client_size(w: u32, h: u32) -> Vec<u8> {
        Self::fixed(format!("{REFRESH_CLIENT} -C {w},{h}"))
    }

    pub fn refresh_client_pause_after(secs: u32) -> Vec<u8> {
        Self::fixed(format!("{REFRESH_CLIENT} -f pause-after={secs}"))
    }

    pub fn refresh_client_continue(pane: u32) -> Vec<u8> {
        Self::fixed(format!("{REFRESH_CLIENT} -A '%{pane}:continue'"))
    }

    pub fn capture_pane(pane: u32, start: Option<i64>) -> Vec<u8> {
        let mut cmd = format!("{CAPTURE_PANE} -p -e -J -t %{pane}");
        if let Some(s) = start {
            let _ = write!(cmd, " -S {s}");
        }
        Self::fixed(cmd)
    }

    pub fn list_windows() -> Vec<u8> {
        Self::fixed(format!("{LIST_WINDOWS} -F '#{{window_layout}}'"))
    }

    pub fn detach_client() -> Vec<u8> {
        Self::fixed(DETACH_CLIENT.to_string())
    }
}

#[cfg(test)]
#[path = "command_tests.rs"]
mod tests;
