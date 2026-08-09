//! Outgoing-command serializer for tmux control mode.
//!
//! Every command is a byte buffer ending in EXACTLY one `b"\n"`. Empty or
//! whitespace-only command strings are rejected because a bare newline
//! detaches the tmux session.

use std::fmt::Write as _;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandError;

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("empty or whitespace-only tmux command")
    }
}

impl std::error::Error for CommandError {}

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
    match write_command(&cmd) {
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
    let mut cmd = format!("send-keys -t %{pane} -H");
    for b in bytes {
        // Infallible write into a String.
        let _ = write!(cmd, " {b:02x}");
    }
    write_command(&cmd)
}

pub fn resize_pane(pane: u32, w: u32, h: u32) -> Vec<u8> {
    fixed(format!("resize-pane -t %{pane} -x {w} -y {h}"))
}

pub fn split_window(pane: u32, horizontal: bool) -> Vec<u8> {
    let dir = if horizontal { " -h" } else { " -v" };
    fixed(format!("split-window -t %{pane}{dir}"))
}

pub fn kill_pane(pane: u32) -> Vec<u8> {
    fixed(format!("kill-pane -t %{pane}"))
}

pub fn new_window() -> Vec<u8> {
    fixed("new-window".to_string())
}

pub fn kill_window(window: u32) -> Vec<u8> {
    fixed(format!("kill-window -t @{window}"))
}

pub fn select_layout(window: u32, layout: &str) -> Vec<u8> {
    fixed(format!("select-layout -t @{window} {layout}"))
}

pub fn refresh_client_size(w: u32, h: u32) -> Vec<u8> {
    fixed(format!("refresh-client -C {w},{h}"))
}

pub fn refresh_client_pause_after(secs: u32) -> Vec<u8> {
    fixed(format!("refresh-client -f pause-after={secs}"))
}

pub fn refresh_client_continue(pane: u32) -> Vec<u8> {
    fixed(format!("refresh-client -A '%{pane}:continue'"))
}

pub fn capture_pane(pane: u32, start: Option<i64>) -> Vec<u8> {
    let mut cmd = format!("capture-pane -p -e -J -t %{pane}");
    if let Some(s) = start {
        let _ = write!(cmd, " -S {s}");
    }
    fixed(cmd)
}

pub fn list_windows() -> Vec<u8> {
    fixed("list-windows -F '#{window_layout}'".to_string())
}

pub fn detach_client() -> Vec<u8> {
    fixed("detach-client".to_string())
}

#[cfg(test)]
#[path = "command_tests.rs"]
mod tests;
