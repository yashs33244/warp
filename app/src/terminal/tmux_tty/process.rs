//! Spawns the `tmux -CC` control-mode child process and bridges its stdio to
//! the async channels the session and event loop use.
//!
//! Mirrors [`crate::remote_server::ssh_transport`]: launch through the
//! `command` wrapper with `kill_on_drop`, take the child's stdin/stdout, then
//! pump bytes between them and a pair of `async_channel`s on the background
//! executor. The process's stdout becomes `control_rx` (the read stream the
//! event loop drains); `input_tx` writes command bytes to its stdin.

use std::io;
use std::process::Stdio;

use futures::io::{AsyncReadExt, AsyncWriteExt};
use warpui::r#async::executor;

const READ_CHUNK: usize = 8192;

/// The channels bridging a running `tmux -CC` process to the integration.
///
/// The child process itself is owned by the stdout-reader task and killed on
/// drop once that task ends (tmux exit / stdout EOF), so callers hold only the
/// channels.
pub struct TmuxControlProcess {
    /// Raw stdout chunks from the control stream, for the event loop to feed
    /// into a [`super::session::TmuxSession`].
    pub control_rx: async_channel::Receiver<Vec<u8>>,
    /// Command bytes to write to the process's stdin. Serialize commands with
    /// [`warp_terminal::tmux::command`]; never send a bare newline.
    pub input_tx: async_channel::Sender<Vec<u8>>,
}

/// Arguments for launching the control client: `tmux -CC` starts a new session,
/// `tmux -CC attach -t <target>` attaches to an existing one.
pub fn control_args(target: Option<&str>) -> Vec<String> {
    match target {
        Some(name) => vec![
            "-CC".to_string(),
            "attach".to_string(),
            "-t".to_string(),
            name.to_string(),
        ],
        None => vec!["-CC".to_string()],
    }
}

/// Spawn `tmux -CC` (optionally attaching to `target`) and wire its stdio to a
/// pair of channels on `executor`.
pub fn spawn_control_process(
    target: Option<&str>,
    executor: &executor::Background,
) -> io::Result<TmuxControlProcess> {
    let mut child = command::r#async::Command::new("tmux")
        .args(control_args(target))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("failed to capture tmux stdin"))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("failed to capture tmux stdout"))?;

    let (control_tx, control_rx) = async_channel::unbounded::<Vec<u8>>();
    let (input_tx, input_rx) = async_channel::unbounded::<Vec<u8>>();

    // Reader: stdout chunks -> control_tx. Owns `child`, so `kill_on_drop`
    // terminates tmux once this task ends (stdout EOF / read error).
    executor
        .spawn(async move {
            let _child = child;
            let mut buf = vec![0u8; READ_CHUNK];
            loop {
                match stdout.read(&mut buf).await {
                    // EOF or read error: the control stream is gone.
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if control_tx.send(buf[..n].to_vec()).await.is_err() {
                            break;
                        }
                    }
                }
            }
        })
        .detach();

    // Writer: input_rx -> stdin. Ends when every `input_tx` clone is dropped.
    executor
        .spawn(async move {
            while let Ok(chunk) = input_rx.recv().await {
                if stdin.write_all(&chunk).await.is_err() || stdin.flush().await.is_err() {
                    break;
                }
            }
        })
        .detach();

    Ok(TmuxControlProcess {
        control_rx,
        input_tx,
    })
}

#[cfg(test)]
#[path = "process_tests.rs"]
mod tests;
