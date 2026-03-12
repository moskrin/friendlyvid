use crate::commands::CommandHistory;
use crate::media::engine::MediaEngine;
use crate::state::AppState;
use crate::ui::layout::LayoutState;

pub struct FriendlyVidApp {
    state: AppState,
    commands: CommandHistory,
    engine: MediaEngine,
    layout: LayoutState,
}

impl FriendlyVidApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            state: AppState::new(),
            commands: CommandHistory::new(),
            engine: MediaEngine::new(),
            layout: LayoutState::new(),
        }
    }
}

impl eframe::App for FriendlyVidApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        crate::ui::layout::show_main_layout(
            ctx,
            &mut self.state,
            &mut self.commands,
            &mut self.engine,
            &mut self.layout,
        );
    }
}
