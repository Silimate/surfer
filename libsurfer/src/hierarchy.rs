//! Functions for drawing the left hand panel showing scopes and variables.
use crate::message::Message;
use crate::transaction_container::StreamScopeRef;
use crate::variable_filter::VariableFilter;
use crate::wave_container::{ScopeRef, ScopeRefExt};
use crate::wave_data::ScopeType;
use crate::SystemState;
use egui::{CentralPanel, Frame, Layout, Margin, ScrollArea, TextWrapMode, TopBottomPanel, Ui};
use emath::Align;

/// Scopes and variables in two separate lists
pub fn separate(state: &mut SystemState, ui: &mut Ui, msgs: &mut Vec<Message>) {
    ui.visuals_mut().override_text_color =
        Some(state.user.config.theme.primary_ui_color.foreground);

    let total_space = ui.available_height();
    TopBottomPanel::top("scopes")
        .resizable(true)
        .default_height(total_space / 2.0)
        .max_height(total_space - 64.0)
        .frame(Frame::new().inner_margin(Margin::same(5)))
        .show_inside(ui, |ui| {
            ui.heading("Scopes");
            ui.add_space(3.0);

            ScrollArea::both()
                .id_salt("scopes")
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    ui.style_mut().wrap_mode = Some(TextWrapMode::Extend);
                    if let Some(waves) = &state.user.waves {
                        let empty_filter = VariableFilter::new();
                        state.draw_all_scopes(msgs, waves, false, ui, &empty_filter);
                    }
                });
        });
    CentralPanel::default()
        .frame(Frame::new().inner_margin(Margin::same(5)))
        .show_inside(ui, |ui| {
            ui.with_layout(Layout::left_to_right(Align::TOP), |ui| {
                ui.heading("Variables");
                ui.add_space(3.0);
                state.draw_variable_filter_edit(ui, msgs);
            });
            ui.add_space(3.0);

            draw_variables(state, msgs, ui);
        });
}

fn draw_variables(state: &mut SystemState, msgs: &mut Vec<Message>, ui: &mut Ui) {
    let filter = &state.user.variable_filter;

    if let Some(waves) = &state.user.waves {
        let empty_scope = if waves.inner.is_waves() {
            ScopeType::WaveScope(ScopeRef::empty())
        } else {
            ScopeType::StreamScope(StreamScopeRef::Empty(String::default()))
        };
        let active_scope = waves.active_scope.as_ref().unwrap_or(&empty_scope);
        match active_scope {
            ScopeType::WaveScope(scope) => {
                let wave_container = waves.inner.as_waves().unwrap();
                let variables =
                    state.filtered_variables(&wave_container.variables_in_scope(scope), filter);
                // Parameters shown in variable list
                if !state.show_parameters_in_scopes() {
                    let parameters = wave_container.parameters_in_scope(scope);
                    if !parameters.is_empty() {
                        ScrollArea::both()
                            .auto_shrink([false; 2])
                            .id_salt("variables")
                            .show(ui, |ui| {
                                egui::collapsing_header::CollapsingState::load_with_default_open(
                                    ui.ctx(),
                                    egui::Id::new(&parameters),
                                    state.expand_parameter_section,
                                )
                                .show_header(ui, |ui| {
                                    ui.with_layout(
                                        Layout::top_down(Align::LEFT).with_cross_justify(true),
                                        |ui| {
                                            ui.label("Parameters");
                                        },
                                    );
                                })
                                .body(|ui| {
                                    state.draw_variable_list(
                                        msgs,
                                        wave_container,
                                        ui,
                                        &parameters,
                                        filter,
                                    );
                                });
                                state.draw_filtered_variable_list(
                                    msgs,
                                    wave_container,
                                    ui,
                                    &variables,
                                );
                            });
                        return; // Early exit
                    }
                }
                // Parameters not shown here or no parameters: use fast approach only drawing visible rows
                let text_style = egui::TextStyle::Body;
                let row_height = ui.text_style_height(&text_style);
                ScrollArea::both()
                    .auto_shrink([false; 2])
                    .id_salt("variables")
                    .show_rows(ui, row_height, variables.len(), |ui, row_range| {
                        state.draw_filtered_variable_list(
                            msgs,
                            wave_container,
                            ui,
                            &variables[row_range],
                        );
                    });
            }
            ScopeType::StreamScope(s) => {
                ScrollArea::both()
                    .auto_shrink([false; 2])
                    .id_salt("variables")
                    .show(ui, |ui| {
                        state.draw_transaction_variable_list(msgs, waves, ui, s);
                    });
            }
        }
    }
}

/// Scopes and variables in a joint tree.
pub fn tree(state: &mut SystemState, ui: &mut Ui, msgs: &mut Vec<Message>) {
    ui.visuals_mut().override_text_color =
        Some(state.user.config.theme.primary_ui_color.foreground);

    ui.with_layout(
        Layout::top_down(Align::LEFT).with_cross_justify(true),
        |ui| {
            Frame::new().inner_margin(Margin::same(5)).show(ui, |ui| {
                ui.with_layout(Layout::left_to_right(Align::TOP), |ui| {
                    ui.heading("Hierarchy");
                    ui.add_space(3.0);
                    state.draw_variable_filter_edit(ui, msgs);
                });
                ui.add_space(3.0);

                ScrollArea::both().id_salt("hierarchy").show(ui, |ui| {
                    ui.style_mut().wrap_mode = Some(TextWrapMode::Extend);
                    if let Some(waves) = &state.user.waves {
                        let filter = &state.user.variable_filter;
                        state.draw_all_scopes(msgs, waves, true, ui, filter);
                    }
                });
            });
        },
    );
}
