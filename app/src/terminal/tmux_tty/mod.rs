//! App-level tmux control-mode (`tmux -CC`) integration.
//!
//! Builds on the protocol core in [`warp_terminal::tmux`] (escaping, layout
//! parse/emit, `%`-notification parsing, command serialization) to render a
//! tmux session's panes as native Warp panes:
//!
//! - [`layout_map`] converts a parsed tmux window layout into Warp's pane-tree
//!   snapshot.
//! - [`terminal_manager`] holds one [`crate::terminal::TerminalModel`] per tmux
//!   pane.
//! - [`session`] fans incoming control bytes out to the matching pane models.
//! - [`input`] turns terminal input/resizes into `send-keys`/`refresh-client`
//!   commands.
//! - [`event_loop`] reads the control stream and drives the session.
//!
//! The integration is gated behind [`crate::features::FeatureFlag::TmuxControlMode`]
//! (turned on by the `tmux_control_mode` Cargo feature); the code compiles
//! unconditionally so the pure logic stays type-checked and unit-tested.

pub mod event_loop;
pub mod input;
pub mod layout_map;
pub mod session;
pub mod terminal_manager;

use crate::features::FeatureFlag;

/// Whether the tmux control-mode integration is active.
pub fn is_enabled() -> bool {
    FeatureFlag::TmuxControlMode.is_enabled()
}

/// Attach a running `tmux -CC` control session as a new Warp tab.
///
/// TODO(PR2-ui): spawn the `tmux -CC` process, split its stdio into a control
/// read stream and a write sink, create one
/// [`terminal_manager::TmuxTerminalManager`] per pane from the initial
/// `%layout-change`, register each as a [`session::TmuxPaneSink`], map that
/// layout via [`layout_map::map_window`] into a tab through
/// `WorkspaceView::add_tab_with_pane_layout`, and drive it with
/// [`event_loop::TmuxEventLoop`]. Gated by [`is_enabled`].
pub fn attach() {
    if is_enabled() {
        // TODO(PR2-ui): wire up the control session (see above).
    }
}
