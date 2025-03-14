//! Command prompt handling.
use crate::command_parser::get_parser;
use crate::fzcmd::{expand_command, parse_command, FuzzyOutput, ParseError};
use crate::{message::Message, SystemState};
use egui::scroll_area::ScrollBarVisibility;
use egui::text::{CCursor, CCursorRange, LayoutJob, TextFormat};
use egui::{Key, RichText, TextEdit, TextStyle};
use emath::{Align, Align2, NumExt, Vec2};
use epaint::{FontFamily, FontId};
use itertools::Itertools;
use std::iter::zip;

pub fn run_fuzzy_parser(input: &str, state: &SystemState, msgs: &mut Vec<Message>) {
    let FuzzyOutput {
        expanded: _,
        suggestions,
    } = expand_command(input, get_parser(state));

    msgs.push(Message::CommandPromptUpdate {
        suggestions: suggestions.unwrap_or_else(|_| vec![]),
    });
}

#[derive(Default)]
pub struct CommandPrompt {
    pub visible: bool,
    pub suggestions: Vec<(String, Vec<bool>)>,
    pub selected: usize,
    pub new_selection: Option<usize>,
    pub new_cursor_pos: Option<usize>,
    pub previous_commands: Vec<(String, Vec<bool>)>,
}

pub fn show_command_prompt(
    state: &mut SystemState,
    ctx: &egui::Context,
    // Window size if known. If unknown defaults to a width of 200pts
    window_size: Option<Vec2>,
    msgs: &mut Vec<Message>,
) {
    egui::Window::new("Commands")
        .anchor(Align2::CENTER_TOP, Vec2::ZERO)
        .title_bar(false)
        .min_width(window_size.map_or(200., |s| s.x * 0.3))
        .resizable(true)
        .show(ctx, |ui| {
            egui::Frame::NONE.show(ui, |ui| {
                let input = &mut *state.command_prompt_text.borrow_mut();
                let new_c = *state.char_to_add_to_prompt.borrow();
                if let Some(c) = new_c {
                    input.push(c);
                    *state.char_to_add_to_prompt.borrow_mut() = None;
                }
                let response = ui.add(
                    TextEdit::singleline(input)
                        .desired_width(f32::INFINITY)
                        .lock_focus(true),
                );

                if response.changed() || state.command_prompt.suggestions.is_empty() {
                    run_fuzzy_parser(input, state, msgs);
                }

                let set_cursor_to_pos = |pos, ui: &mut egui::Ui| {
                    if let Some(mut state) = TextEdit::load_state(ui.ctx(), response.id) {
                        let ccursor = CCursor::new(pos);
                        state
                            .cursor
                            .set_char_range(Some(CCursorRange::one(ccursor)));
                        state.store(ui.ctx(), response.id);
                        ui.ctx().memory_mut(|m| m.request_focus(response.id));
                    }
                };

                if response.ctx.input(|i| i.key_pressed(Key::ArrowUp)) {
                    set_cursor_to_pos(input.chars().count(), ui);
                }
                if let Some(new_pos) = state.command_prompt.new_cursor_pos {
                    set_cursor_to_pos(new_pos, ui);
                    state.command_prompt.new_cursor_pos = None;
                }

                let suggestions = state
                    .command_prompt
                    .previous_commands
                    .iter()
                    // take up to 3 previous commands
                    .take(if input.is_empty() { 3 } else { 0 })
                    // reverse them so that the most recent one is at the bottom
                    .rev()
                    .chain(state.command_prompt.suggestions.iter())
                    .enumerate()
                    // allow scrolling down the suggestions
                    .collect_vec();

                // Expand the current input to full command and append the suggestion that is selected in the ui.
                let append_suggestion = |input: &String| -> String {
                    let new_input = if !state.command_prompt.suggestions.is_empty() {
                        // if no suggestions exist we use the last argument in the input (e.g., for divider_add)
                        let default = input
                            .split_ascii_whitespace()
                            .last()
                            .unwrap_or("")
                            .to_string();

                        let selection = suggestions
                            .get(state.command_prompt.selected)
                            .map_or(&default, |s| &s.1 .0);

                        if input.chars().last().is_some_and(char::is_whitespace) {
                            // if no input exists for current argument just append
                            input.to_owned() + " " + selection
                        } else {
                            // if something was already typed for this argument removed then append
                            let parts = input.split_ascii_whitespace().collect_vec();
                            parts.iter().take(parts.len().saturating_sub(1)).join(" ")
                                + " "
                                + selection
                        }
                    } else {
                        input.to_string()
                    };
                    expand_command(&new_input, get_parser(state)).expanded
                };

                if response.ctx.input(|i| i.key_pressed(Key::Tab)) {
                    let mut new_input = append_suggestion(input);
                    let parsed = parse_command(&new_input, get_parser(state));
                    if let Err(ParseError::MissingParameters) = parsed {
                        new_input += " ";
                    }
                    *input = new_input;
                    set_cursor_to_pos(input.chars().count(), ui);
                    run_fuzzy_parser(input, state, msgs);
                }

                if response.lost_focus() && response.ctx.input(|i| i.key_pressed(Key::Enter)) {
                    let expanded = append_suggestion(input);
                    let parsed = (
                        expanded.clone(),
                        parse_command(&expanded, get_parser(state)),
                    );

                    if let Ok(cmd) = parsed.1 {
                        msgs.push(Message::ShowCommandPrompt(None));
                        msgs.push(Message::CommandPromptClear);
                        msgs.push(Message::CommandPromptPushPrevious(parsed.0));
                        msgs.push(cmd);
                        run_fuzzy_parser("", state, msgs);
                    } else {
                        *input = parsed.0 + " ";
                        // move cursor to end of input
                        set_cursor_to_pos(input.chars().count(), ui);
                        // run fuzzy parser since setting the cursor swallows the `changed` flag
                        run_fuzzy_parser(input, state, msgs);
                    }
                }

                response.request_focus();

                // draw current expansion of input and selected suggestions
                let expanded = expand_command(input, get_parser(state)).expanded;
                if !expanded.is_empty() {
                    ui.horizontal(|ui| {
                        let label = ui.label(
                            RichText::new("Expansion").color(
                                state
                                    .user
                                    .config
                                    .theme
                                    .primary_ui_color
                                    .foreground
                                    .gamma_multiply(0.5),
                            ),
                        );
                        ui.vertical(|ui| {
                            ui.add_space(label.rect.height() / 2.0);
                            ui.separator()
                        });
                    });

                    ui.allocate_ui_with_layout(
                        ui.available_size(),
                        egui::Layout::top_down(Align::LEFT).with_cross_justify(true),
                        |ui| {
                            ui.add(SuggestionLabel::new(
                                RichText::new(expanded.clone())
                                    .size(14.0)
                                    .family(FontFamily::Monospace)
                                    .color(
                                        state
                                            .user
                                            .config
                                            .theme
                                            .accent_info
                                            .background
                                            .gamma_multiply(0.75),
                                    ),
                                false,
                            ))
                        },
                    );
                }

                let text_style = TextStyle::Button;
                let row_height = ui.text_style_height(&text_style);
                egui::ScrollArea::vertical()
                    .scroll_bar_visibility(ScrollBarVisibility::AlwaysVisible)
                    .show_rows(ui, row_height, suggestions.len(), |ui, row_range| {
                        for (idx, suggestion) in &suggestions[row_range] {
                            let idx = *idx;
                            let mut job = LayoutJob::default();
                            let selected = state.command_prompt.selected == idx;

                            let previous_cmds_len = state.command_prompt.previous_commands.len();
                            if idx == 0 && previous_cmds_len != 0 && input.is_empty() {
                                ui.horizontal(|ui| {
                                    let label = ui.label(
                                        RichText::new("Recently used").color(
                                            state
                                                .user
                                                .config
                                                .theme
                                                .primary_ui_color
                                                .foreground
                                                .gamma_multiply(0.5),
                                        ),
                                    );
                                    ui.vertical(|ui| {
                                        ui.add_space(label.rect.height() / 2.0);
                                        ui.separator()
                                    });
                                });
                            }

                            if (idx == previous_cmds_len.clamp(0, 3) && input.is_empty())
                                || (idx == 0 && !input.is_empty())
                            {
                                ui.horizontal(|ui| {
                                    let label = ui.label(
                                        RichText::new("Suggestions").color(
                                            state
                                                .user
                                                .config
                                                .theme
                                                .primary_ui_color
                                                .foreground
                                                .gamma_multiply(0.5),
                                        ),
                                    );
                                    ui.vertical(|ui| {
                                        ui.add_space(label.rect.height() / 2.0);
                                        ui.separator()
                                    });
                                });
                            }

                            for (c, highlight) in zip(suggestion.0.chars(), &suggestion.1) {
                                let mut tmp = [0u8; 4];
                                let sub_string = c.encode_utf8(&mut tmp);
                                job.append(
                                    sub_string,
                                    0.0,
                                    TextFormat {
                                        font_id: FontId::new(14.0, FontFamily::Monospace),
                                        color: if selected || *highlight {
                                            state.user.config.theme.accent_info.background
                                        } else {
                                            state.user.config.theme.primary_ui_color.foreground
                                        },
                                        ..Default::default()
                                    },
                                );
                            }

                            // make label full width of the palette
                            let resp = ui.allocate_ui_with_layout(
                                ui.available_size(),
                                egui::Layout::top_down(Align::LEFT).with_cross_justify(true),
                                |ui| ui.add(SuggestionLabel::new(job, selected)),
                            );

                            if state
                                .command_prompt
                                .new_selection
                                .is_some_and(|new_idx| idx == new_idx)
                            {
                                resp.response.scroll_to_me(Some(Align::Center));
                            }

                            if resp.inner.clicked() {
                                let new_input =
                                    if input.chars().last().is_some_and(char::is_whitespace) {
                                        // if no input exists for current argument just append
                                        input.to_owned() + " " + &suggestion.0
                                    } else {
                                        // if something was already typed for this argument removed then append
                                        let parts = input.split_ascii_whitespace().collect_vec();
                                        parts.iter().take(parts.len().saturating_sub(1)).join(" ")
                                            + " "
                                            + &suggestion.0
                                    };
                                let expanded =
                                    expand_command(&new_input, get_parser(state)).expanded;
                                let result = (
                                    expanded.clone(),
                                    parse_command(&expanded, get_parser(state)),
                                );

                                if let Ok(cmd) = result.1 {
                                    msgs.push(Message::ShowCommandPrompt(None));
                                    msgs.push(Message::CommandPromptClear);
                                    msgs.push(Message::CommandPromptPushPrevious(expanded));
                                    msgs.push(cmd);
                                    run_fuzzy_parser("", state, msgs);
                                } else {
                                    *input = result.0 + " ";
                                    set_cursor_to_pos(input.chars().count(), ui);
                                    // run fuzzy parser since setting the cursor swallows the `changed` flag
                                    run_fuzzy_parser(input, state, msgs);
                                }
                            }
                        }
                    });
            });
        });
}

// This SuggestionLabel is based on egui's SelectableLabel
#[must_use = "You should put this widget in an ui with `ui.add(widget);`"]
pub struct SuggestionLabel {
    text: egui::WidgetText,
    selected: bool,
}

impl SuggestionLabel {
    pub fn new(text: impl Into<egui::WidgetText>, selected: bool) -> Self {
        Self {
            text: text.into(),
            selected,
        }
    }
}

impl egui::Widget for SuggestionLabel {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let Self { text, selected: _ } = self;

        let button_padding = ui.spacing().button_padding;
        let total_extra = button_padding + button_padding;

        let wrap_width = ui.available_width() - total_extra.x;
        let text = text.into_galley(ui, None, wrap_width, egui::TextStyle::Button);

        let mut desired_size = total_extra + text.size();
        desired_size.y = desired_size.y.at_least(ui.spacing().interact_size.y);
        let (rect, response) = ui.allocate_at_least(desired_size, egui::Sense::click());

        if ui.is_rect_visible(response.rect) {
            let text_pos = ui
                .layout()
                .align_size_within_rect(text.size(), rect.shrink2(button_padding))
                .min;

            let visuals = ui.style().interact_selectable(&response, false);

            if response.hovered() || self.selected {
                let rect = rect.expand(visuals.expansion);

                ui.painter().rect(
                    rect,
                    visuals.corner_radius,
                    visuals.weak_bg_fill,
                    egui::Stroke::NONE,
                    egui::StrokeKind::Middle,
                );
            }

            ui.painter().galley(text_pos, text, visuals.text_color());
        }

        response
    }
}
