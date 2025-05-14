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
use crate::variable_direction::VariableDirectionExt;
use crate::wave_container::WaveContainer;
use crate::wave_data::ScopeType;
use crate::{message::Message, wave_container::VariableRef, SystemState};
use surfer_translation_types::VariableDirection;

use std::cmp::Ordering;

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

    pub(crate) include_inputs: bool,
    pub(crate) include_outputs: bool,
    pub(crate) include_inouts: bool,
    pub(crate) include_others: bool,

    pub(crate) group_by_direction: bool,
}

#[derive(Debug, Deserialize)]
pub enum VariableIOFilterType {
    Input,
    Output,
    InOut,
    Other,
}

impl Default for VariableFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl VariableFilter {
    pub fn new() -> VariableFilter {
        VariableFilter {
            name_filter_type: VariableNameFilterType::Contain,
            name_filter_str: String::from(""),
            name_filter_case_insensitive: true,

            include_inputs: true,
            include_outputs: true,
            include_inouts: true,
            include_others: true,

            group_by_direction: false,
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

    fn kind_filter(&self, vr: &VariableRef, wave_container_opt: Option<&WaveContainer>) -> bool {
        match get_variable_direction(vr, wave_container_opt) {
            VariableDirection::Input => self.include_inputs,
            VariableDirection::Output => self.include_outputs,
            VariableDirection::InOut => self.include_inouts,
            _ => self.include_others,
        }
    }

    pub fn matching_variables(
        &self,
        variables: &[VariableRef],
        wave_container_opt: Option<&WaveContainer>,
    ) -> Vec<VariableRef> {
        let mut name_filter = self.name_filter_fn();

        variables
            .iter()
            .filter(|&vr| name_filter(&vr.name))
            .filter(|&vr| self.kind_filter(vr, wave_container_opt))
            .cloned()
            .collect_vec()
    }
}

impl SystemState {
    pub fn draw_variable_filter_edit(&mut self, ui: &mut Ui, msgs: &mut Vec<Message>) {
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
                self.variable_filter_type_menu(ui, msgs);
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
                self.variable_filter_type_menu(ui, msgs);
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

    pub fn variable_filter_type_menu(&self, ui: &mut Ui, msgs: &mut Vec<Message>) {
        for filter_type in enum_iterator::all::<VariableNameFilterType>() {
            ui.radio(
                self.user.variable_filter.name_filter_type == filter_type,
                filter_type.to_string(),
            )
            .clicked()
            .then(|| {
                ui.close_menu();
                msgs.push(Message::SetVariableNameFilterType(filter_type));
            });
        }

        ui.separator();

        // Checkbox wants a mutable bool reference but we don't have mutable self to give it a
        // mutable 'group_by_direction' directly. Plus we want to update things via a message. So
        // make a copy of the flag here that can be mutable and just ensure we update the actual
        // flag on a click.
        let mut group_by_direction = self.user.variable_filter.group_by_direction;

        ui.checkbox(&mut group_by_direction, "Group by direction")
            .clicked()
            .then(|| {
                msgs.push(Message::SetVariableGroupByDirection(
                    !self.user.variable_filter.group_by_direction,
                ))
            });

        ui.separator();

        ui.horizontal(|ui| {
            let input = VariableDirection::Input;
            let output = VariableDirection::Output;
            let inout = VariableDirection::InOut;

            ui.add(
                Button::new(input.get_icon().unwrap())
                    .selected(self.user.variable_filter.include_inputs),
            )
            .on_hover_text("Show inputs")
            .clicked()
            .then(|| {
                msgs.push(Message::SetVariableIOFilter(
                    VariableIOFilterType::Input,
                    !self.user.variable_filter.include_inputs,
                ));
            });

            ui.add(
                Button::new(output.get_icon().unwrap())
                    .selected(self.user.variable_filter.include_outputs),
            )
            .on_hover_text("Show outputs")
            .clicked()
            .then(|| {
                msgs.push(Message::SetVariableIOFilter(
                    VariableIOFilterType::Output,
                    !self.user.variable_filter.include_outputs,
                ));
            });

            ui.add(
                Button::new(inout.get_icon().unwrap())
                    .selected(self.user.variable_filter.include_inouts),
            )
            .on_hover_text("Show inouts")
            .clicked()
            .then(|| {
                msgs.push(Message::SetVariableIOFilter(
                    VariableIOFilterType::InOut,
                    !self.user.variable_filter.include_inouts,
                ));
            });

            ui.add(
                Button::new(icons::GLOBAL_LINE).selected(self.user.variable_filter.include_others),
            )
            .on_hover_text("Show others")
            .clicked()
            .then(|| {
                msgs.push(Message::SetVariableIOFilter(
                    VariableIOFilterType::Other,
                    !self.user.variable_filter.include_others,
                ));
            });
        });
    }

    pub fn variable_cmp(
        &self,
        a: &VariableRef,
        b: &VariableRef,
        wave_container: Option<&WaveContainer>,
    ) -> Ordering {
        let a_direction = get_variable_direction(a, wave_container);
        let b_direction = get_variable_direction(b, wave_container);

        if !self.user.variable_filter.group_by_direction || a_direction == b_direction {
            numeric_sort::cmp(&a.name, &b.name)
        } else if a_direction < b_direction {
            Ordering::Less
        } else {
            Ordering::Greater
        }
    }

    pub fn filtered_variables(
        &self,
        variables: &[VariableRef],
        variable_filter: &VariableFilter,
    ) -> Vec<VariableRef> {
        let wave_container = match &self.user.waves {
            Some(wd) => wd.inner.as_waves(),
            None => None,
        };

        variable_filter
            .matching_variables(variables, wave_container)
            .iter()
            .sorted_by(|a, b| self.variable_cmp(a, b, wave_container))
            .cloned()
            .collect_vec()
    }
}

fn get_variable_direction(
    vr: &VariableRef,
    wave_container_opt: Option<&WaveContainer>,
) -> VariableDirection {
    match wave_container_opt {
        Some(wave_container) => wave_container
            .variable_meta(vr)
            .map_or(VariableDirection::Unknown, |m| {
                m.direction.unwrap_or(VariableDirection::Unknown)
            }),
        None => VariableDirection::Unknown,
    }
}
