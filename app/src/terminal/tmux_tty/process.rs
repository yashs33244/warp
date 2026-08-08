//! Spawns the `tmux -CC` control-mode child process and bridges its stdio to
//! the async channels the session and event loop use.
//!
//! Unlike a plain pipe transport (`ssh_transport`), tmux's control client calls
//! `tcgetattr` on its stdio at startup and exits immediately if it is not a tty,
//! so `tmux -CC` must run on a **PTY**, not pipes. We allocate one the same way
//! Warp spawns its shells (`local_tty::unix`): `openpty`, then a `pre_exec` hook
//! that dups the follower onto the child's stdio, `setsid`s, and makes the
//! follower the controlling terminal. The leader fd is bridged to a pair of
//! `async_channel`s by two blocking threads (one control process per attach, so
//! the thread cost is negligible). The leader's read half becomes `control_rx`
//! (the stream the event loop drains); `input_tx` writes command bytes to it.

use std::io;

const READ_CHUNK: usize = 8192;

/// The channels bridging a running `tmux -CC` process to the integration.
///
/// `handle` owns the child and kills it on drop, so the control session is torn
/// down when the event loop that holds this is dropped (its workspace window
/// closes). Callers keep `handle` alive for as long as the session should live.
pub struct TmuxControlProcess {
    /// Raw leader chunks from the control stream, for the event loop to feed
    /// into a [`super::session::TmuxSession`].
    pub control_rx: async_channel::Receiver<Vec<u8>>,
    /// Command bytes to write to the control stream. Serialize commands with
    /// [`warp_terminal::tmux::command`]; never send a bare newline.
    pub input_tx: async_channel::Sender<Vec<u8>>,
    /// Kills the tmux child when dropped.
    pub handle: TmuxProcessHandle,
}

/// Kills the `tmux -CC` child on drop, tearing down the control session (which
/// closes the leader fd and ends the bridge threads).
pub struct TmuxProcessHandle {
    child: std::sync::Arc<std::sync::Mutex<std::process::Child>>,
}

impl Drop for TmuxProcessHandle {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
        }
    }
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

/// Directories searched for `tmux` when it is not on `PATH`. A GUI app started
/// by launchd inherits only `/usr/bin:/bin:/usr/sbin:/sbin`, so a Homebrew or
/// MacPorts tmux is invisible to a bare `Command::new("tmux")` even though it
/// works in the user's shell.
const TMUX_FALLBACK_DIRS: &[&str] = &[
    "/opt/homebrew/bin",
    "/usr/local/bin",
    "/opt/local/bin",
    "/usr/bin",
    "/bin",
];

/// Absolute path of the `tmux` binary, or plain `"tmux"` when no candidate
/// exists (the spawn then fails with a clear "not found").
///
/// `WARP_TMUX_PROGRAM` overrides the search.
pub fn tmux_program() -> std::ffi::OsString {
    if let Some(program) = std::env::var_os("WARP_TMUX_PROGRAM") {
        return program;
    }
    let from_path = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .unwrap_or_default();
    let candidates = from_path
        .into_iter()
        .chain(TMUX_FALLBACK_DIRS.iter().map(std::path::PathBuf::from));
    for dir in candidates {
        let candidate = dir.join("tmux");
        if candidate.is_file() {
            return candidate.into_os_string();
        }
    }
    std::ffi::OsString::from("tmux")
}

/// Names of the tmux sessions on the default server, for the attach picker.
///
/// Runs `tmux list-sessions` out-of-band (the in-band `list-windows` needs a
/// control client, which is what we are about to start). No server running, or
/// any failure, yields an empty list rather than an error: "nothing to attach
/// to" is the same outcome to the caller.
pub fn list_sessions() -> Vec<String> {
    let Ok(output) = command::blocking::Command::new(tmux_program())
        .args(["list-sessions", "-F", "#{session_name}"])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect()
}

/// tmux pane ids the user warpified during an earlier attach.
///
/// The flag lives on the pane as a tmux user option, so it survives detaching
/// and outlives the Warp process; reading it back is what makes the choice stick
/// per pane. Any failure yields an empty set, i.e. "nothing was warpified",
/// which only costs the user a click.
pub fn warpified_panes() -> std::collections::HashSet<u32> {
    let Ok(output) = command::blocking::Command::new(tmux_program())
        .args([
            "list-panes",
            "-a",
            "-F",
            &format!(
                "#{{pane_id}} #{{{}}}",
                warp_terminal::tmux::command::WARPIFIED_PANE_OPTION
            ),
        ])
        .output()
    else {
        return Default::default();
    };
    if !output.status.success() {
        return Default::default();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let (id, value) = line.split_once(' ')?;
            // Only a pane explicitly marked in a previous attach counts; an
            // unset option reports as empty.
            (!value.trim().is_empty()).then(|| id.strip_prefix('%')?.parse::<u32>().ok())?
        })
        .collect()
}

/// Shells that own the prompt, i.e. panes where typing a bootstrap line is safe.
const KNOWN_SHELLS: &[&str] = &[
    "zsh", "bash", "fish", "sh", "dash", "ksh", "tcsh", "csh", "nu", "xonsh", "elvish",
];

/// The foreground command in each pane, keyed by tmux pane id (`zsh` at a
/// prompt, `nvim` / `claude` / `node` while a program runs).
pub fn pane_foreground_commands() -> std::collections::HashMap<u32, String> {
    let Ok(output) = command::blocking::Command::new(tmux_program())
        .args([
            "list-panes",
            "-a",
            "-F",
            "#{pane_id} #{pane_current_command}",
        ])
        .output()
    else {
        return Default::default();
    };
    if !output.status.success() {
        return Default::default();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let (id, cmd) = line.split_once(' ')?;
            let id = id.strip_prefix('%')?.parse::<u32>().ok()?;
            Some((id, cmd.trim().to_string()))
        })
        .collect()
}

/// Whether `cmd` is a shell that owns its prompt.
pub fn is_known_shell(cmd: &str) -> bool {
    KNOWN_SHELLS.contains(&cmd)
}

/// Whether `pane` is sitting at its shell prompt.
///
/// Warpifying types a bootstrap line into the pane, so it is only safe when the
/// shell is the one reading input. Injecting while a program runs (an editor, a
/// coding agent) feeds the script to that program instead: it never bootstraps
/// and the program receives a screenful of garbage.
pub fn pane_is_at_shell_prompt(pane: u32) -> bool {
    pane_foreground_commands()
        .get(&pane)
        .is_some_and(|cmd| is_known_shell(cmd.as_str()))
}

/// Spawn `tmux -CC` (optionally attaching to `target`) on a PTY and bridge its
/// leader fd to a pair of channels.
#[cfg(unix)]
pub fn spawn_control_process(target: Option<&str>) -> io::Result<TmuxControlProcess> {
    use std::io::{Read, Write};
    use std::os::fd::FromRawFd;
    use std::sync::{Arc, Mutex};

    use command::blocking::Command;

    // Initial PTY size; tmux renders panes at this geometry until a resize
    // (`refresh-client -C`) adjusts it. A generous default keeps first output
    // from wrapping awkwardly before the real pane size is known.
    let (leader, follower) = make_pty(40, 120)?;

    let mut cmd = Command::new(tmux_program());
    cmd.args(control_args(target));

    // SAFETY: the hook runs after fork and before exec. It only calls
    // async-signal-safe libc functions (dup2/setsid/ioctl) and returns any
    // failure as an errno the parent surfaces from `spawn`.
    unsafe {
        cmd.pre_exec(move || {
            // The follower PTY becomes the child's stdio. dup2 clears
            // FD_CLOEXEC on the new fds, so 0/1/2 survive exec even though the
            // follower itself is close-on-exec.
            if libc::dup2(follower, libc::STDIN_FILENO) < 0
                || libc::dup2(follower, libc::STDOUT_FILENO) < 0
                || libc::dup2(follower, libc::STDERR_FILENO) < 0
            {
                return Err(io::Error::last_os_error());
            }
            // New session, then make the follower the controlling terminal so
            // tmux's `tcgetattr` succeeds and job control works.
            if libc::setsid() < 0 {
                return Err(io::Error::last_os_error());
            }
            #[allow(clippy::cast_lossless)]
            if libc::ioctl(follower, libc::TIOCSCTTY as _, 0) < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }

    let child = cmd.spawn()?;
    // The parent no longer needs the follower; the child holds its own copies.
    unsafe { libc::close(follower) };

    // Own the leader fd; a duplicate goes to the writer thread so the reader and
    // writer close their ends independently.
    let leader_read = unsafe { std::fs::File::from_raw_fd(leader) };
    let leader_write = leader_read.try_clone()?;

    let child = Arc::new(Mutex::new(child));
    let (control_tx, control_rx) = async_channel::unbounded::<Vec<u8>>();
    let (input_tx, input_rx) = async_channel::unbounded::<Vec<u8>>();

    // Reader: leader -> control_tx. On EOF/read error the control stream is
    // gone; reap the child so tmux doesn't linger as a zombie.
    {
        let child = Arc::clone(&child);
        std::thread::Builder::new()
            .name("tmux-control-reader".to_string())
            .spawn(move || {
                let mut leader = leader_read;
                let mut buf = vec![0u8; READ_CHUNK];
                loop {
                    match leader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            // The channel is unbounded, so `send` is always
                            // immediately ready; `block_on` just resolves it (the
                            // repo bans `send_blocking`, which is wasm-incompatible).
                            if warpui::r#async::block_on(control_tx.send(buf[..n].to_vec()))
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                }
                if let Ok(mut child) = child.lock() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            })?;
    }

    // Writer: input_rx -> leader. Ends when every `input_tx` clone is dropped.
    std::thread::Builder::new()
        .name("tmux-control-writer".to_string())
        .spawn(move || {
            let mut leader = leader_write;
            while let Ok(chunk) = input_rx.recv_blocking() {
                if leader.write_all(&chunk).is_err() || leader.flush().is_err() {
                    break;
                }
            }
        })?;

    Ok(TmuxControlProcess {
        control_rx,
        input_tx,
        handle: TmuxProcessHandle { child },
    })
}

#[cfg(not(unix))]
pub fn spawn_control_process(_target: Option<&str>) -> io::Result<TmuxControlProcess> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "tmux control mode is only supported on unix",
    ))
}

/// Allocate a PTY and return `(leader, follower)` raw fds, both close-on-exec.
/// Mirrors `local_tty::unix::make_pty`: the `dup2` in the child's `pre_exec`
/// clears FD_CLOEXEC on the stdio copies, so the follower still reaches tmux.
#[cfg(unix)]
fn make_pty(rows: u16, cols: u16) -> io::Result<(std::os::fd::RawFd, std::os::fd::RawFd)> {
    let win_size = libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let ends = nix::pty::openpty(Some(&win_size), None)
        .map_err(|e| io::Error::other(format!("openpty failed: {e}")))?;
    unsafe {
        libc::fcntl(ends.master, libc::F_SETFD, libc::FD_CLOEXEC);
        libc::fcntl(ends.slave, libc::F_SETFD, libc::FD_CLOEXEC);
    }
    Ok((ends.master, ends.slave))
}

#[cfg(test)]
#[path = "process_tests.rs"]
mod tests;
