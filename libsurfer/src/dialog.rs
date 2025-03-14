use crate::message::Message;
use crate::SystemState;
use ecolor::Color32;
use egui::{Layout, RichText};
use emath::Align;

#[derive(Debug, Default, Copy, Clone)]
pub struct ReloadWaveformDialog {
    /// `true` to persist the setting returned by the dialog.
    do_not_show_again: bool,
}

#[derive(Debug, Default, Copy, Clone)]
pub struct OpenSiblingStateFileDialog {
    do_not_show_again: bool,
}

impl SystemState {
    /// Draw a dialog that asks the user if it wants to load a state file situated in the same directory as the waveform file.
    pub(crate) fn draw_open_sibling_state_file_dialog(
        &self,
        ctx: &egui::Context,
        dialog: &OpenSiblingStateFileDialog,
        msgs: &mut Vec<Message>,
    ) {
        let mut do_not_show_again = dialog.do_not_show_again;
        egui::Window::new("State file detected")
            .auto_sized()
            .collapsible(false)
            .fixed_pos(ctx.available_rect().center())
            .show(ctx, |ui| {
                let label = ui.label(RichText::new("A state file was detected in the same directory as the loaded file.\nLoad state?").heading());
                ui.set_width(label.rect.width());
                ui.add_space(5.0);
                ui.checkbox(
                    &mut do_not_show_again,
                    "Remember my decision for this session",
                );
                ui.add_space(14.0);
                ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                    // Sets the style when focused
                    ui.style_mut().visuals.widgets.active.weak_bg_fill = Color32::BLUE;
                    let load_button = ui.button("Load");
                    let dont_load_button = ui.button("Don't load");
                    ctx.memory_mut(|mem| {
                        if mem.focused() != Some(load_button.id)
                            && mem.focused() != Some(dont_load_button.id)
                        {
                            mem.request_focus(load_button.id)
                        }
                    });

                    if load_button.clicked() {
                        msgs.push(Message::CloseOpenSiblingStateFileDialog {
                            load_state: true,
                            do_not_show_again,
                        });
                    } else if dont_load_button.clicked() {
                        msgs.push(Message::CloseOpenSiblingStateFileDialog {
                            load_state: false,
                            do_not_show_again,
                        });
                    } else if do_not_show_again != dialog.do_not_show_again {
                        msgs.push(Message::UpdateOpenSiblingStateFileDialog(OpenSiblingStateFileDialog {
                            do_not_show_again,
                        }));
                    }
                });
            });
    }

    /// Draw a dialog that asks for user confirmation before re-loading a file.
    /// This is triggered by a file loading event from disk.
    pub(crate) fn draw_reload_waveform_dialog(
        &self,
        ctx: &egui::Context,
        dialog: &ReloadWaveformDialog,
        msgs: &mut Vec<Message>,
    ) {
        let mut do_not_show_again = dialog.do_not_show_again;
        egui::Window::new("File Change")
            .auto_sized()
            .collapsible(false)
            .fixed_pos(ctx.available_rect().center())
            .show(ctx, |ui| {
                let label = ui.label(RichText::new("File on disk has changed. Reload?").heading());
                ui.set_width(label.rect.width());
                ui.add_space(5.0);
                ui.checkbox(
                    &mut do_not_show_again,
                    "Remember my decision for this session",
                );
                ui.add_space(14.0);
                ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                    // Sets the style when focused
                    ui.style_mut().visuals.widgets.active.weak_bg_fill = Color32::BLUE;
                    let reload_button = ui.button("Reload");
                    let leave_button = ui.button("Leave");
                    ctx.memory_mut(|mem| {
                        if mem.focused() != Some(reload_button.id)
                            && mem.focused() != Some(leave_button.id)
                        {
                            mem.request_focus(reload_button.id)
                        }
                    });

                    if reload_button.clicked() {
                        msgs.push(Message::CloseReloadWaveformDialog {
                            reload_file: true,
                            do_not_show_again,
                        });
                    } else if leave_button.clicked() {
                        msgs.push(Message::CloseReloadWaveformDialog {
                            reload_file: false,
                            do_not_show_again,
                        });
                    } else if do_not_show_again != dialog.do_not_show_again {
                        msgs.push(Message::UpdateReloadWaveformDialog(ReloadWaveformDialog {
                            do_not_show_again,
                        }));
                    }
                });
            });
    }
}
