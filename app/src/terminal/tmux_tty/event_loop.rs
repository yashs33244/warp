//! Reads the tmux control stream and drives a [`TmuxSession`].
//!
//! Modeled on `remote_tty::event_loop`: one connection (here the read half of
//! the `tmux -CC` process's stdio, delivered as byte chunks over a channel)
//! feeds a set of terminal models through a session. Lifecycle decisions the
//! session emits are applied to the [`Workspace`] this loop was started for.

use std::collections::HashSet;

use async_channel::{Receiver, Sender};
use warp_terminal::tmux::command;
use warpui::{Entity, ModelContext, WeakViewHandle};

use super::geometry::TmuxGeometry;
use super::session::{LifecycleEffect, TmuxSession};
use crate::workspace::Workspace;

/// Owns the [`TmuxSession`] and feeds it bytes read from the control stream,
/// applying the lifecycle effects it emits to the owning [`Workspace`].
pub struct TmuxEventLoop {
    session: TmuxSession,
    /// The workspace whose tabs mirror this tmux session's windows.
    workspace: WeakViewHandle<Workspace>,
    /// Write half of the control stream, handed to each adopted pane so its
    /// keystrokes can be sent back to tmux.
    input_tx: Sender<Vec<u8>>,
    /// tmux windows already adopted into a tab. The first known layout for a
    /// window builds its tab; later changes are diffed (PR5).
    adopted_windows: HashSet<u32>,
    /// The grid tmux last laid each pane and window out at, shared with every
    /// adopted pane so its resizes can be phrased as a window size.
    geometry: TmuxGeometry,
    /// Kills the `tmux -CC` child when this loop is dropped (its workspace
    /// window closed), tearing the control session down.
    _process_handle: super::process::TmuxProcessHandle,
}

impl TmuxEventLoop {
    /// Start draining `control_rx` (chunks read from the control fd) into
    /// `session`, applying the resulting lifecycle effects to `workspace`.
    /// Unknown-pane `%output` is dropped by the session until its pane is
    /// registered during adoption.
    pub fn start(
        session: TmuxSession,
        control_rx: Receiver<Vec<u8>>,
        input_tx: Sender<Vec<u8>>,
        process_handle: super::process::TmuxProcessHandle,
        workspace: WeakViewHandle<Workspace>,
        ctx: &mut ModelContext<Self>,
    ) -> Self {
        ctx.spawn_stream_local(control_rx, Self::on_control_bytes, |_, _| {});
        Self {
            session,
            workspace,
            input_tx,
            adopted_windows: HashSet::new(),
            geometry: TmuxGeometry::default(),
            _process_handle: process_handle,
        }
    }

    fn on_control_bytes(&mut self, bytes: Vec<u8>, ctx: &mut ModelContext<Self>) {
        for effect in self.session.feed_control_bytes(&bytes) {
            self.apply_effect(effect, ctx);
        }
    }

    /// Apply one lifecycle decision to the workspace.
    fn apply_effect(&mut self, effect: LifecycleEffect, ctx: &mut ModelContext<Self>) {
        match effect {
            LifecycleEffect::WindowLayout {
                window,
                layout,
                delta,
            } => {
                // Recorded for every layout, including the size-only ones that
                // produce no delta, so a pane resize always scales against what
                // tmux currently believes.
                self.geometry.record(window, &layout);
                let Some(workspace) = self.workspace.upgrade(ctx) else {
                    return;
                };
                let input_tx = self.input_tx.clone();
                let geometry = self.geometry.clone();
                // The first layout for a window builds its tab; later ones only
                // splice in what changed.
                let sinks = if self.adopted_windows.insert(window) {
                    workspace.update(ctx, |workspace, ctx| {
                        workspace.adopt_tmux_window(window, &layout, &input_tx, &geometry, ctx)
                    })
                } else if delta.is_empty() {
                    Vec::new()
                } else {
                    workspace.update(ctx, |workspace, ctx| {
                        workspace.apply_tmux_layout_delta(
                            window, &layout, &delta, &input_tx, &geometry, ctx,
                        )
                    })
                };
                for (pane, sink) in sinks {
                    self.session.register_pane(pane, sink);
                }
            }
            LifecycleEffect::ResumePane { pane } => {
                // Ask tmux to resume a flow-control-paused pane. We're a fast
                // native consumer, so we always request resumption rather than
                // leaving the pane stalled; the session's bounded buffer guards
                // against a flooding pane in the meantime.
                let _ = self
                    .input_tx
                    .try_send(command::refresh_client_continue(pane));
            }
            LifecycleEffect::RewarpifyPane { pane } => {
                // The pane's shell is back at a prompt but speaks with a stale
                // session id; re-run the warpify handshake for it.
                if let Some(workspace) = self.workspace.upgrade(ctx) {
                    workspace.update(ctx, |workspace, ctx| {
                        workspace.rewarpify_tmux_pane(pane, ctx)
                    });
                }
            }
            LifecycleEffect::WindowClosed { window } => {
                self.adopted_windows.remove(&window);
                // TODO(PR5-teardown): remove the adopted tab for this window.
            }
            LifecycleEffect::WindowAdded { .. } => {
                // tmux announces a new window with `%window-add` but no layout;
                // query the current windows so the session parses this one's
                // layout from the reply and emits a `WindowLayout` to adopt it.
                let _ = self.input_tx.try_send(command::list_windows());
            }
            LifecycleEffect::WindowRenamed { .. } => {
                // TODO(PR5-ui): retitle the tab for this window.
            }
            LifecycleEffect::Exit { .. } => {
                // Detaching produces `%exit` too. Either way the panes now
                // mirror a session Warp cannot reach, so they are closed rather
                // than left on screen as dead output.
                self.adopted_windows.clear();
                if let Some(workspace) = self.workspace.upgrade(ctx) {
                    workspace.update(ctx, |workspace, ctx| workspace.close_all_tmux_panes(ctx));
                }
            }
        }
    }
}

impl Entity for TmuxEventLoop {
    type Event = ();
}
