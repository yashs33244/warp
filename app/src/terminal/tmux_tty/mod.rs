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
pub mod lifecycle;
pub mod process;
pub mod session;
pub mod terminal_manager;

use crate::features::FeatureFlag;

/// Whether the tmux control-mode integration is active.
pub fn is_enabled() -> bool {
    FeatureFlag::TmuxControlMode.is_enabled()
}

/// Attach a running `tmux -CC` control session as a new Warp tab.
///
/// The end-to-end path is: [`process::spawn_control_process`] launches the
/// child and yields `control_rx`/`input_tx`; a [`session::TmuxSession`] drains
/// `control_rx` on an [`event_loop::TmuxEventLoop`], routing `%output` to per-
/// pane sinks and emitting [`session::LifecycleEffect`]s for window/pane
/// changes; the first `%layout-change` seeds the initial tab.
///
/// TODO(PR4-ui): wire the tab up. This needs an `AppContext`/`WorkspaceView`
/// (to call [`terminal_manager::TmuxTerminalManager::create_model`] per pane and
/// `WorkspaceView::add_tab_with_pane_layout`), so `attach` will grow those
/// parameters. It also needs a pane-adoption path that does not exist yet:
/// `PanesLayout::Snapshot` rebuilds each `TerminalPaneSnapshot` by spawning a
/// fresh shell PTY (see `PaneGroup::create_session`), so a `LeafSnapshot`
/// *cannot* reference an already-created tmux-backed manager/view. Adopting the
/// pre-built [`terminal_manager::TmuxTerminalManager`] panes into a tab requires
/// a new `PanesLayout` variant (or a whole-tree analogue of
/// `WorkspaceView::add_tab_from_existing_pane`) — that is the PR4 change.
/// Gated by [`is_enabled`].
pub fn attach() {
    if is_enabled() {
        // TODO(PR4-ui): wire up the control session (see above).
    }
}
