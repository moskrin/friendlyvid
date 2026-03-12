use egui;

pub struct ToolbarAction {
    pub new_project: bool,
    pub open_file: bool,
    pub open_project: bool,
    pub save_project: bool,
    pub save_project_as: bool,
    pub export: bool,
    pub exit: bool,
    pub split: bool,
    pub delete: bool,
    pub undo: bool,
    pub redo: bool,
    pub crop: bool,
}

impl Default for ToolbarAction {
    fn default() -> Self {
        Self {
            new_project: false,
            open_file: false,
            open_project: false,
            save_project: false,
            save_project_as: false,
            export: false,
            exit: false,
            split: false,
            delete: false,
            undo: false,
            redo: false,
            crop: false,
        }
    }
}

pub fn show_toolbar(
    ui: &mut egui::Ui,
    can_undo: bool,
    can_redo: bool,
    can_split: bool,
    has_selection: bool,
    crop_mode: bool,
    has_clips: bool,
) -> ToolbarAction {
    let mut action = ToolbarAction::default();

    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button("File", |ui| {
            if ui.button("New").clicked() {
                action.new_project = true;
                ui.close();
            }
            if ui.button("Open Media...").clicked() {
                action.open_file = true;
                ui.close();
            }
            ui.separator();
            if ui.button("Open Project...").clicked() {
                action.open_project = true;
                ui.close();
            }
            if ui
                .add_enabled(has_clips, egui::Button::new("Save Project"))
                .on_hover_text("Ctrl+S")
                .clicked()
            {
                action.save_project = true;
                ui.close();
            }
            if ui
                .add_enabled(has_clips, egui::Button::new("Save Project As..."))
                .clicked()
            {
                action.save_project_as = true;
                ui.close();
            }
            ui.separator();
            if ui
                .add_enabled(has_clips, egui::Button::new("Export..."))
                .on_hover_text("Export as H264 MP4")
                .clicked()
            {
                action.export = true;
                ui.close();
            }
            ui.separator();
            if ui.button("Exit").clicked() {
                action.exit = true;
                ui.close();
            }
        });

        ui.separator();

        if ui
            .add_enabled(can_undo, egui::Button::new("Undo"))
            .clicked()
        {
            action.undo = true;
        }

        if ui
            .add_enabled(can_redo, egui::Button::new("Redo"))
            .clicked()
        {
            action.redo = true;
        }

        ui.separator();

        if ui
            .add_enabled(can_split, egui::Button::new("Split"))
            .on_hover_text("Split clip at playhead (Ctrl+B)")
            .clicked()
        {
            action.split = true;
        }

        if ui
            .add_enabled(has_selection, egui::Button::new("Delete"))
            .on_hover_text("Delete selected clip (Del)")
            .clicked()
        {
            action.delete = true;
        }

        ui.separator();

        let crop_label = if crop_mode { "Done Crop" } else { "Crop" };
        if ui
            .add_enabled(has_selection || crop_mode, egui::Button::new(crop_label))
            .on_hover_text("Zoom/pan crop (scroll to zoom, drag to pan)")
            .clicked()
        {
            action.crop = true;
        }
    });

    action
}
