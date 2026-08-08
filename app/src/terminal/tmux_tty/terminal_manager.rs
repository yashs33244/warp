//! A [`TerminalManager`] backing a single tmux pane's [`TerminalModel`].

use std::any::Any;
use std::sync::Arc;

use parking_lot::FairMutex;
use pathfinder_geometry::vector::Vector2F;
use warpui::{AppContext, ModelHandle, ViewHandle, WindowId};

use super::input::TmuxInputSender;
use crate::context_chips::prompt_type::PromptType;
use crate::pane_group::TerminalViewResources;
use crate::terminal::event_listener::ChannelEventListener;
use crate::terminal::model::session::Sessions;
use crate::terminal::model_events::ModelEventDispatcher;
use crate::terminal::shell::ShellName;
use crate::terminal::terminal_manager::BlockSpacing;
use crate::terminal::writeable_pty::terminal_manager_util::{
    init_pty_controller_model, wire_up_pty_controller_with_surface,
};
use crate::terminal::{
    ShellLaunchState, TerminalManager, TerminalModel, TerminalView, terminal_manager,
};

/// Owns the [`TerminalModel`] that renders one tmux pane. Modeled on
/// `MockTerminalManager`: a model + view with no local PTY, since the pane's
/// bytes arrive over the tmux control stream rather than a child process.
pub struct TmuxTerminalManager {
    model: Arc<FairMutex<TerminalModel>>,
    _view: ViewHandle<TerminalView>,
}

/// The pieces produced when creating a tmux pane manager. `model` and `events`
/// let the caller build a [`super::session::TmuxPaneSink`] that writes control
/// output into this pane.
pub struct TmuxTerminalManagerInit {
    pub manager: ModelHandle<Box<dyn TerminalManager>>,
    pub view: ViewHandle<TerminalView>,
    pub model: Arc<FairMutex<TerminalModel>>,
    pub events: ChannelEventListener,
}

/// A tmux pane's already-built manager+view, ready to be adopted into a Warp tab
/// leaf without spawning a fresh shell PTY. Carried in
/// [`crate::pane_group::PanesLayout::AdoptedTmux`] and turned into a
/// `TerminalPane` by the pane group's adoption path.
#[derive(Debug)]
pub struct AdoptedTmuxPane {
    /// Fresh per-pane session UUID (tmux panes carry no persisted Warp uuid).
    pub uuid: Vec<u8>,
    pub manager: ModelHandle<Box<dyn TerminalManager>>,
    pub view: ViewHandle<TerminalView>,
}

impl TmuxTerminalManager {
    pub fn create_model(
        resources: TerminalViewResources,
        input_sender: TmuxInputSender,
        initial_size: Vector2F,
        window_id: WindowId,
        ctx: &mut AppContext,
    ) -> TmuxTerminalManagerInit {
        let (wakeups_tx, wakeups_rx) = async_channel::unbounded();
        let (events_tx, events_rx) = async_channel::unbounded();
        let (pty_reads_tx, _pty_reads_rx) = async_broadcast::broadcast(1);
        let (executor_command_tx, executor_command_rx) = async_channel::unbounded();

        let channel_event_proxy = ChannelEventListener::new(wakeups_tx, events_tx, pty_reads_tx);

        // The debug memory-stats footer is a per-block affordance; an adopted
        // pane has a single synthetic block, so it would just draw a stray stats
        // line pinned across the pane's bottom edge.
        let mut block_spacing = BlockSpacing::for_gui(ctx);
        block_spacing.show_memory_stats = false;

        let mut model = terminal_manager::create_terminal_model(
            None, /* startup_directory */
            None, /* restored_blocks */
            initial_size,
            channel_event_proxy.clone(),
            // The shell inside a tmux pane is already running and its type is
            // not known here (it is learned only if/when the pane is warpified),
            // so start in DeterminingShell rather than mislabeling it as zsh.
            ShellLaunchState::DeterminingShell {
                available_shell: None,
                display_name: ShellName::blank(),
            },
            block_spacing,
            ctx,
        );
        // The shell in a tmux pane is already running and never performs Warp's
        // bootstrap handshake, so mark the pane bootstrapped ourselves; without
        // this it rejects input and renders its output into a hidden block.
        model.block_list_mut().simulate_bootstrap_for_adopted_pane();
        // ...and route every keystroke to tmux instead of Warp's input editor.
        model.set_adopted_pane();
        let colors = model.colors();
        let model = Arc::new(FairMutex::new(model));

        let sessions: ModelHandle<Sessions> =
            ctx.add_model(|ctx| Sessions::new(executor_command_tx, ctx));
        let model_events_dispatcher =
            ctx.add_model(|ctx| ModelEventDispatcher::new(events_rx, sessions.clone(), ctx));

        // Broker for writes to this pane: user keystrokes routed through the
        // view flow here and out to tmux as `send-keys` via `input_sender`.
        let pty_controller = init_pty_controller_model(
            input_sender,
            executor_command_rx,
            model_events_dispatcher.clone(),
            sessions.clone(),
            model.clone(),
            ctx,
        );

        // Captured before `resources` is moved into the view below.
        let model_event_sender = resources.model_event_sender.clone();

        let cloned_model = model.clone();
        let prompt_type =
            ctx.add_model(|ctx| PromptType::new_dynamic_from_sessions(sessions.clone(), ctx));
        let view = ctx.add_typed_action_view(window_id, |ctx| {
            let size_info = cloned_model.lock().block_list().size().to_owned();
            TerminalView::new(
                resources,
                wakeups_rx,
                model_events_dispatcher.clone(),
                cloned_model,
                sessions.clone(),
                size_info,
                colors,
                None,
                prompt_type,
                None,
                None, // conversation_restoration
                None, // inactive_pty_reads_rx
                false,
                ctx,
            )
        });

        // Route the view's input/resize `Message`s to the PtyController (and its
        // disconnect events back), the same bi-directional wiring `local_tty`
        // uses. This is what turns a keystroke in the adopted pane into a
        // `send-keys` command to tmux.
        wire_up_pty_controller_with_surface(
            &pty_controller,
            &view,
            model.clone(),
            sessions.clone(),
            model_event_sender,
            ctx,
        );

        // Retain the model-events dispatcher and PtyController for the life of
        // the view (same trick as `MockTerminalManager`): give ownership to a
        // closure held by a task that never completes.
        view.update(ctx, |_view, ctx| {
            ctx.spawn(futures::future::pending::<()>(), move |_, _, _| {
                std::mem::drop(model_events_dispatcher);
                std::mem::drop(pty_controller);
            });
        });

        let terminal_view = view.clone();
        let manager = Self {
            model: model.clone(),
            _view: view,
        };
        let manager_model = ctx.add_model(|_ctx| {
            let manager: Box<dyn TerminalManager> = Box::new(manager);
            manager
        });

        TmuxTerminalManagerInit {
            manager: manager_model,
            view: terminal_view,
            model,
            events: channel_event_proxy,
        }
    }
}

impl TerminalManager for TmuxTerminalManager {
    fn model(&self) -> Arc<FairMutex<TerminalModel>> {
        self.model.clone()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
