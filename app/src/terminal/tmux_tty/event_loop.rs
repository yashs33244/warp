//! Reads the tmux control stream and drives a [`TmuxSession`].
//!
//! Modeled on `remote_tty::event_loop`: one connection (here the read half of
//! the `tmux -CC` process's stdio, delivered as byte chunks over a channel)
//! feeds a set of terminal models through a session.

use async_channel::Receiver;
use warpui::{Entity, ModelContext};

use super::session::{LifecycleEffect, TmuxSession};

/// Owns the [`TmuxSession`] and feeds it bytes read from the control stream.
pub struct TmuxEventLoop {
    session: TmuxSession,
}

impl TmuxEventLoop {
    /// Start draining `control_rx` (chunks read from the control fd) into
    /// `session`. Panes should be registered on `session` before/as they are
    /// created; unknown-pane `%output` is dropped by the session.
    pub fn start(
        session: TmuxSession,
        control_rx: Receiver<Vec<u8>>,
        ctx: &mut ModelContext<Self>,
    ) -> Self {
        ctx.spawn_stream_local(control_rx, Self::on_control_bytes, |_, _| {});
        Self { session }
    }

    fn on_control_bytes(&mut self, bytes: Vec<u8>, _ctx: &mut ModelContext<Self>) {
        for effect in self.session.feed_control_bytes(&bytes) {
            apply_effect(effect);
        }
    }
}

/// Apply one lifecycle decision to the workspace.
///
/// TODO(PR4-ui): actually mutate the UI here — add/remove/rename tabs, recompute
/// a window's split tree, create a `TmuxTerminalManager` for each added pane,
/// and tear down on exit. That needs an `AppContext`/`WorkspaceView` the event
/// loop does not yet hold; until then the decisions are computed and logged so
/// the pure lifecycle path stays exercised end to end.
fn apply_effect(effect: LifecycleEffect) {
    log::debug!("tmux control-mode lifecycle effect (PR4 will apply): {effect:?}");
}

impl Entity for TmuxEventLoop {
    type Event = ();
}
