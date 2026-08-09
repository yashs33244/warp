//! Reads the tmux control stream and drives a [`TmuxSession`].
//!
//! Modeled on `remote_tty::event_loop`: one connection (here the read half of
//! the `tmux -CC` process's stdio, delivered as byte chunks over a channel)
//! feeds a set of terminal models through a session.

use async_channel::Receiver;
use warpui::{Entity, ModelContext};

use super::session::TmuxSession;

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
        self.session.feed_control_bytes(&bytes);
    }
}

impl Entity for TmuxEventLoop {
    type Event = ();
}
