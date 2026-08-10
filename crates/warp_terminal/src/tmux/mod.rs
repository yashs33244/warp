//! tmux control-mode TTY primitives: output escaping, layout parse/emit,
//! `%`-notification parsing + reply assembly, and command serialization.
//!
//! Pure protocol logic with no app dependencies. The app-level integration
//! (TmuxTerminalManager, fan-out event loop, UI) lives in
//! `app/src/terminal/tmux_tty` and builds on this.

pub mod command;
pub mod escape;
pub mod layout;
pub mod notification;
