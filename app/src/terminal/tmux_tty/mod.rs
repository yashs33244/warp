//! App-level tmux control-mode (`tmux -CC`) integration.
//!
//! Builds on the protocol core in [`warp_terminal::tmux`] (escaping, layout
//! parse/emit, `%`-notification parsing, command serialization) to render a
//! tmux session's panes as native Warp panes:
//!
//! - [`process`] spawns the `tmux -CC` child and bridges its stdio to the
//!   control/input channels.
//! - [`layout_map`] converts a parsed tmux window layout into Warp's pane-tree
//!   snapshot.
//! - [`terminal_manager`] holds one [`crate::terminal::TerminalModel`] per tmux
//!   pane.
//! - [`session`] fans incoming control bytes out to the matching pane models
//!   and turns window/pane notifications into [`session::LifecycleEffect`]s.
//! - [`lifecycle`] holds the context-free pane-set / layout-diff logic the
//!   session decides with.
//! - [`flow_control`] holds the pure per-pane pause/continue bookkeeping and
//!   bounded backlog the session uses for tmux flow control.
//! - [`input`] turns terminal input/resizes into `send-keys`/`refresh-client`
//!   commands.
//! - [`event_loop`] reads the control stream and drives the session.
//!
//! The integration is gated behind [`crate::features::FeatureFlag::TmuxControlMode`]
//! (turned on by the `tmux_control_mode` Cargo feature); the code compiles
//! unconditionally so the pure logic stays type-checked and unit-tested.

pub mod event_loop;
pub mod flow_control;
pub mod input;
pub mod layout_map;
pub mod lifecycle;
pub mod process;
pub mod session;
pub mod terminal_manager;

use std::io;

use warp_terminal::tmux::command;
use warpui::{AppContext, ViewHandle};

use crate::features::FeatureFlag;
use crate::workspace::Workspace;

/// Seconds a pane may fall behind before tmux flow-control pauses it (see
/// [`command::refresh_client_pause_after`]). Small so a flood pauses promptly;
/// the session resumes paused panes as soon as it drains them.
const FLOW_CONTROL_PAUSE_AFTER_SECS: u32 = 1;

/// Whether the tmux control-mode integration is active.
pub fn is_enabled() -> bool {
    FeatureFlag::TmuxControlMode.is_enabled()
}

/// Attach a running `tmux -CC` control session, mirroring its windows as Warp
/// tabs.
///
/// The end-to-end path: [`process::spawn_control_process`] launches the child
/// and yields `control_rx`/`input_tx`; a [`session::TmuxSession`] drains
/// `control_rx` on an [`event_loop::TmuxEventLoop`], routing `%output` to per-
/// pane sinks and emitting [`session::LifecycleEffect`]s for window/pane
/// changes. The first `%layout-change` for a window calls
/// [`Workspace::adopt_tmux_window`], which builds one
/// [`terminal_manager::TmuxTerminalManager`] per pane and adopts them into a tab
/// via [`crate::pane_group::PanesLayout::AdoptedTmux`] (no shell PTY is spawned).
///
/// No-op when the integration is disabled (see [`is_enabled`]).
///
/// TODO(PR5-ui): expose an entry point (command palette / menu) that calls this.
pub fn attach(
    workspace: &ViewHandle<Workspace>,
    target: Option<&str>,
    ctx: &mut AppContext,
) -> io::Result<()> {
    if !is_enabled() {
        return Ok(());
    }

    let executor = ctx.background_executor();
    let process = process::spawn_control_process(target, executor)?;

    // Turn on tmux flow control: a pane the client falls behind on is paused
    // (via `%pause`) instead of flooding the shared control stream, and the
    // session resumes it once drained. Best-effort; a full input channel here
    // only means the initial refresh is skipped.
    let _ = process
        .input_tx
        .try_send(command::refresh_client_pause_after(
            FLOW_CONTROL_PAUSE_AFTER_SECS,
        ));

    let session = session::TmuxSession::new();
    let workspace_weak = workspace.downgrade();

    let event_loop = ctx.add_model(|model_ctx| {
        event_loop::TmuxEventLoop::start(
            session,
            process.control_rx,
            process.input_tx,
            workspace_weak,
            model_ctx,
        )
    });

    // Retain the event-loop model (and its control-stream reader) for the
    // workspace view's lifetime, the same ownership trick the terminal managers
    // use: hand it to a task that never completes.
    // TODO(PR5-teardown): drop it on `%exit` / detach instead of holding it
    // until the workspace window closes.
    workspace.update(ctx, |_workspace, ctx| {
        ctx.spawn(futures::future::pending::<()>(), move |_, _, _| {
            std::mem::drop(event_loop);
        });
    });

    Ok(())
}
