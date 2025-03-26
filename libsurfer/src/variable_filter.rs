//! Filtering of the variable list.
use derive_more::Display;
use egui::{Button, Layout, TextEdit, Ui};
use egui_remixicon::icons;
use emath::{Align, Vec2};
use enum_iterator::Sequence;
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use itertools::Itertools;
use regex::{escape, Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

use crate::data_container::DataContainer::Transactions;
use crate::transaction_container::{StreamScopeRef, TransactionStreamRef};
use crate::wave_data::ScopeType;
use crate::{message::Message, wave_container::VariableRef, SystemState};

#[derive(Debug, Display, PartialEq, Serialize, Deserialize, Sequence)]
pub enum VariableNameFilterType {
    #[display("Fuzzy")]
    Fuzzy,

    #[display("Regular expression")]
    Regex,

    #[display("Variable starts with")]
    Start,

    #[display("Variable contains")]
    Contain,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VariableFilter {
    pub(crate) name_filter_type: VariableNameFilterType,
    pub(crate) name_filter_str: String,
    pub(crate) name_filter_case_insensitive: bool,
}

impl VariableFilter {
    pub fn new() -> VariableFilter {
        VariableFilter {
            name_filter_type: VariableNameFilterType::Contain,
            name_filter_str: String::from(""),
            name_filter_case_insensitive: true,
        }
    }

    fn name_filter_fn(&self) -> Box<dyn FnMut(&str) -> bool> {
        if self.name_filter_str.is_empty() {
            return Box::new(|_var_name| true);
        }

        match self.name_filter_type {
            VariableNameFilterType::Fuzzy => {
                let matcher = if self.name_filter_case_insensitive {
                    SkimMatcherV2::default().ignore_case()
                } else {
                    SkimMatcherV2::default().respect_case()
                };

                // Make a copy of the filter string to move into the closure below
                let filter_str_clone = self.name_filter_str.clone();

                Box::new(move |var_name| matcher.fuzzy_match(var_name, &filter_str_clone).is_some())
            }
            VariableNameFilterType::Regex => {
                if let Ok(regex) = RegexBuilder::new(&self.name_filter_str)
                    .case_insensitive(self.name_filter_case_insensitive)
                    .build()
                {
                    Box::new(move |var_name| regex.is_match(var_name))
                } else {
                    Box::new(|_var_name| false)
                }
            }
            VariableNameFilterType::Start => {
                if let Ok(regex) = RegexBuilder::new(&format!("^{}", escape(&self.name_filter_str)))
                    .case_insensitive(self.name_filter_case_insensitive)
                    .build()
                {
                    Box::new(move |var_name| regex.is_match(var_name))
                } else {
                    Box::new(|_var_name| false)
                }
            }
            VariableNameFilterType::Contain => {
                if let Ok(regex) = RegexBuilder::new(&escape(&self.name_filter_str))
                    .case_insensitive(self.name_filter_case_insensitive)
                    .build()
                {
                    Box::new(move |var_name| regex.is_match(var_name))
                } else {
                    Box::new(|_var_name| false)
                }
            }
        }
    }

    pub fn matching_variables(&self, variables: &[VariableRef]) -> Vec<VariableRef> {
        let mut name_filter = self.name_filter_fn();

        variables
            .iter()
            .filter(|&vr| name_filter(&vr.name))
            .cloned()
            .collect_vec()
    }
}

impl SystemState {
    pub fn draw_variable_name_filter_edit(&mut self, ui: &mut Ui, msgs: &mut Vec<Message>) {
        ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
            let default_padding = ui.spacing().button_padding;
            ui.spacing_mut().button_padding = Vec2 {
                x: 0.,
                y: default_padding.y,
            };
            ui.button(icons::ADD_FILL)
                .on_hover_text("Add all variables from active Scope")
                .clicked()
                .then(|| {
                    if let Some(waves) = self.user.waves.as_ref() {
                        // Iterate over the reversed list to get
                        // waves in the same order as the variable
                        // list
                        if let Some(active_scope) = waves.active_scope.as_ref() {
                            match active_scope {
                                ScopeType::WaveScope(active_scope) => {
                                    let variables = waves
                                        .inner
                                        .as_waves()
                                        .unwrap()
                                        .variables_in_scope(active_scope);
                                    msgs.push(Message::AddVariables(self.filtered_variables(
                                        &variables,
                                        &self.user.variable_filter,
                                    )));
                                }
                                ScopeType::StreamScope(active_scope) => {
                                    let Transactions(inner) = &waves.inner else {
                                        return;
                                    };
                                    match active_scope {
                                        StreamScopeRef::Root => {
                                            for stream in inner.get_streams() {
                                                msgs.push(Message::AddStreamOrGenerator(
                                                    TransactionStreamRef::new_stream(
                                                        stream.id,
                                                        stream.name.clone(),
                                                    ),
                                                ));
                                            }
                                        }
                                        StreamScopeRef::Stream(s) => {
                                            for gen_id in
                                                &inner.get_stream(s.stream_id).unwrap().generators
                                            {
                                                let gen = inner.get_generator(*gen_id).unwrap();

                                                msgs.push(Message::AddStreamOrGenerator(
                                                    TransactionStreamRef::new_gen(
                                                        gen.stream_id,
                                                        gen.id,
                                                        gen.name.clone(),
                                                    ),
                                                ));
                                            }
                                        }
                                        StreamScopeRef::Empty(_) => {}
                                    }
                                }
                            }
                        }
                    }
                });
            ui.add(
                Button::new(icons::FONT_SIZE)
                    .selected(!self.user.variable_filter.name_filter_case_insensitive),
            )
            .on_hover_text("Case sensitive filter")
            .clicked()
            .then(|| {
                msgs.push(Message::SetVariableNameFilterCaseInsensitive(
                    !self.user.variable_filter.name_filter_case_insensitive,
                ));
            });
            ui.menu_button(icons::FILTER_FILL, |ui| {
                variable_name_filter_type_menu(
                    ui,
                    msgs,
                    &self.user.variable_filter.name_filter_type,
                );
            });
            ui.add_enabled(
                !self.user.variable_filter.name_filter_str.is_empty(),
                Button::new(icons::CLOSE_FILL),
            )
            .on_hover_text("Clear filter")
            .clicked()
            .then(|| self.user.variable_filter.name_filter_str.clear());

            // Check if regex and if an incorrect regex, change background color
            if self.user.variable_filter.name_filter_type == VariableNameFilterType::Regex
                && Regex::new(&self.user.variable_filter.name_filter_str).is_err()
            {
                ui.style_mut().visuals.extreme_bg_color =
                    self.user.config.theme.accent_error.background;
            }
            // Create text edit
            let response = ui.add(
                TextEdit::singleline(&mut self.user.variable_filter.name_filter_str)
                    .hint_text("Filter (context menu for type)"),
            );
            response.context_menu(|ui| {
                variable_name_filter_type_menu(
                    ui,
                    msgs,
                    &self.user.variable_filter.name_filter_type,
                );
            });
            // Handle focus
            if response.gained_focus() {
                msgs.push(Message::SetFilterFocused(true));
            }
            if response.lost_focus() {
                msgs.push(Message::SetFilterFocused(false));
            }
            ui.spacing_mut().button_padding = default_padding;
        });
    }

    pub fn filtered_variables(
        &self,
        variables: &[VariableRef],
        variable_filter: &VariableFilter,
    ) -> Vec<VariableRef> {
        variable_filter
            .matching_variables(variables)
            .iter()
            .sorted_by(|a, b| numeric_sort::cmp(&a.name, &b.name))
            .cloned()
            .collect_vec()
    }
}

pub fn variable_name_filter_type_menu(
    ui: &mut Ui,
    msgs: &mut Vec<Message>,
    variable_name_filter_type: &VariableNameFilterType,
) {
    for filter_type in enum_iterator::all::<VariableNameFilterType>() {
        ui.radio(
            *variable_name_filter_type == filter_type,
            filter_type.to_string(),
        )
        .clicked()
        .then(|| {
            ui.close_menu();
            msgs.push(Message::SetVariableNameFilterType(filter_type));
        });
    }
}
