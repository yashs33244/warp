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
    /// tmux windows already adopted into a tab. The first `%layout-change` for
    /// a window builds its tab; later changes are diffed (PR5).
    adopted_windows: HashSet<u32>,
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
        workspace: WeakViewHandle<Workspace>,
        ctx: &mut ModelContext<Self>,
    ) -> Self {
        ctx.spawn_stream_local(control_rx, Self::on_control_bytes, |_, _| {});
        Self {
            session,
            workspace,
            input_tx,
            adopted_windows: HashSet::new(),
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
            LifecycleEffect::WindowLayout { window, layout, .. } => {
                if !self.adopted_windows.insert(window) {
                    // TODO(PR5-incremental): apply the layout `delta` to the
                    // existing tab (split in `added` panes, tear down
                    // `removed`). For now only the first layout per window
                    // builds a tab.
                    return;
                }
                let Some(workspace) = self.workspace.upgrade(ctx) else {
                    return;
                };
                let input_tx = self.input_tx.clone();
                let sinks = workspace.update(ctx, |workspace, ctx| {
                    workspace.adopt_tmux_window(window, &layout, &input_tx, ctx)
                });
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
            LifecycleEffect::WindowClosed { window } => {
                self.adopted_windows.remove(&window);
                // TODO(PR5-teardown): remove the adopted tab for this window.
            }
            LifecycleEffect::WindowAdded { .. } | LifecycleEffect::WindowRenamed { .. } => {
                // TODO(PR5-ui): add / rename the tab for this window. A window
                // becomes visible once its first `%layout-change` arrives.
            }
            LifecycleEffect::Exit { .. } => {
                self.adopted_windows.clear();
                // TODO(PR5-teardown): close the adopted tmux tabs and drop this
                // event loop so the control process is reaped.
            }
        }
    }
}

impl Entity for TmuxEventLoop {
    type Event = ();
}
