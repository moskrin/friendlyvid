use egui;
use uuid::Uuid;

use crate::model::Project;
use crate::util::time::format_duration;

pub enum MediaBrowserAction {
    None,
    OpenFile,
    RemoveSource(Uuid),
}

/// Renders the media browser as a vertical list. The caller provides
/// a constrained-width `ui`; we just fill it top-to-bottom.
pub fn show_media_browser(ui: &mut egui::Ui, project: &Project) -> MediaBrowserAction {
    let mut action = MediaBrowserAction::None;

    ui.vertical(|ui| {
        ui.heading("Media");
        ui.separator();

        if project.source_files.is_empty() {
            ui.add_space(20.0);
            ui.centered_and_justified(|ui| {
                if ui
                    .label(
                        egui::RichText::new("Click to open a media file")
                            .color(egui::Color32::from_gray(120)),
                    )
                    .clicked()
                {
                    action = MediaBrowserAction::OpenFile;
                }
            });
            return;
        }

        let mut remove_id = None;
        for source in &project.source_files {
            ui.horizontal(|ui| {
                let filename = source.filename();
                let detail = if source.has_video {
                    format!("{}x{} {}", source.width, source.height, format_duration(source.duration))
                } else {
                    format_duration(source.duration).to_string()
                };

                // Truncated filename + detail
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(format!("{} ({})", filename, detail))
                            .small()
                            .color(egui::Color32::from_gray(200)),
                    )
                    .truncate(),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button("X")
                        .on_hover_text("Remove from project")
                        .clicked()
                    {
                        remove_id = Some(source.id);
                    }
                });
            });
        }

        if let Some(id) = remove_id {
            action = MediaBrowserAction::RemoveSource(id);
        }
    });

    action
}
