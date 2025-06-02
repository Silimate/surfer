//! Command prompt handling.
use std::{fs, str::FromStr};

use crate::config::ArrowKeyBindings;
use crate::displayed_item_tree::{Node, VisibleItemIndex};
use crate::fzcmd::{Command, ParamGreed};
use crate::hierarchy::HierarchyStyle;
use crate::lazy_static;
use crate::message::MessageTarget;
use crate::transaction_container::StreamScopeRef;
use crate::wave_container::{ScopeRef, ScopeRefExt, VariableRef, VariableRefExt};
use crate::wave_data::ScopeType;
use crate::wave_source::LoadOptions;
use crate::{
    clock_highlighting::ClockHighlightType,
    displayed_item::DisplayedItem,
    message::Message,
    util::{alpha_idx_to_uint_idx, uint_idx_to_alpha_idx},
    variable_name_type::VariableNameType,
    SystemState,
};
use itertools::Itertools;
use log::warn;

type RestCommand = Box<dyn Fn(&str) -> Option<Command<Message>>>;

fn is_wave_file_extension(ext: &str) -> bool {
    ext == "vcd" || ext == "fst" || ext == "ghw"
}

/// Split part of a query at whitespace
///
/// fzcmd splits at regex "words" which does not include special characters
/// like '#'. This function can be used instead via `ParamGreed::Custom(&separate_at_space)`
fn separate_at_space(query: &str) -> (String, String, String, String) {
    use regex::Regex;
    lazy_static! {
        static ref RE: Regex = Regex::new(r#"(\s*)(\S*)(\s?)(.*)"#).unwrap();
    }

    let captures = RE.captures_iter(query).next().unwrap();

    (
        captures[1].into(),
        captures[2].into(),
        captures[3].into(),
        captures[4].into(),
    )
}

pub fn get_parser(state: &SystemState) -> Command<Message> {
    fn single_word(
        suggestions: Vec<String>,
        rest_command: RestCommand,
    ) -> Option<Command<Message>> {
        Some(Command::NonTerminal(
            ParamGreed::Rest,
            suggestions,
            Box::new(move |query, _| rest_command(query)),
        ))
    }

    fn optional_single_word(
        suggestions: Vec<String>,
        rest_command: RestCommand,
    ) -> Option<Command<Message>> {
        Some(Command::NonTerminal(
            ParamGreed::OptionalWord,
            suggestions,
            Box::new(move |query, _| rest_command(query)),
        ))
    }

    fn single_word_delayed_suggestions(
        suggestions: Box<dyn Fn() -> Vec<String>>,
        rest_command: RestCommand,
    ) -> Option<Command<Message>> {
        Some(Command::NonTerminal(
            ParamGreed::Rest,
            suggestions(),
            Box::new(move |query, _| rest_command(query)),
        ))
    }

    let scopes = match &state.user.waves {
        Some(v) => v.inner.scope_names(),
        None => vec![],
    };
    let variables = match &state.user.waves {
        Some(v) => v.inner.variable_names(),
        None => vec![],
    };
    let displayed_items = match &state.user.waves {
        Some(v) => v
            .items_tree
            .iter_visible()
            .enumerate()
            .map(
                |(
                    vidx,
                    Node {
                        item_ref: item_id, ..
                    },
                )| {
                    let idx = VisibleItemIndex(vidx);
                    let item = &v.displayed_items[item_id];
                    match item {
                        DisplayedItem::Variable(var) => format!(
                            "{}_{}",
                            uint_idx_to_alpha_idx(idx, v.displayed_items.len()),
                            var.variable_ref.full_path_string()
                        ),
                        _ => format!(
                            "{}_{}",
                            uint_idx_to_alpha_idx(idx, v.displayed_items.len()),
                            item.name()
                        ),
                    }
                },
            )
            .collect_vec(),
        None => vec![],
    };
    let variables_in_active_scope = state
        .user
        .waves
        .as_ref()
        .and_then(|waves| {
            waves
                .active_scope
                .as_ref()
                .map(|scope| waves.inner.variables_in_scope(scope))
        })
        .unwrap_or_default();

    let color_names = state.user.config.theme.colors.keys().cloned().collect_vec();
    let format_names: Vec<String> = state
        .translators
        .all_translator_names()
        .into_iter()
        .map(&str::to_owned)
        .collect();

    let active_scope = state
        .user
        .waves
        .as_ref()
        .and_then(|w| w.active_scope.clone());

    let is_transaction_container = state
        .user
        .waves
        .as_ref()
        .is_some_and(|w| w.inner.is_transactions());

    fn files_with_ext(matches: fn(&str) -> bool) -> Vec<String> {
        if let Ok(res) = fs::read_dir(".") {
            res.map(|res| res.map(|e| e.path()).unwrap_or_default())
                .filter(|file| {
                    file.extension()
                        .is_some_and(|extension| (matches)(extension.to_str().unwrap_or("")))
                })
                .map(|file| file.into_os_string().into_string().unwrap())
                .collect::<Vec<String>>()
        } else {
            vec![]
        }
    }

    fn all_wave_files() -> Vec<String> {
        files_with_ext(is_wave_file_extension)
    }

    let markers = if let Some(waves) = &state.user.waves {
        waves
            .items_tree
            .iter()
            .map(|Node { item_ref, .. }| waves.displayed_items.get(item_ref))
            .filter_map(|item| match item {
                Some(DisplayedItem::Marker(marker)) => Some((marker.name.clone(), marker.idx)),
                _ => None,
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    fn parse_marker(query: &str, markers: &[(Option<String>, u8)]) -> Option<u8> {
        if let Some(id_str) = query.strip_prefix("#") {
            let id = id_str.parse::<u8>().ok()?;
            Some(id)
        } else {
            markers.iter().find_map(|(name, idx)| {
                if name.is_some() && name.as_ref().unwrap() == query {
                    Some(*idx)
                } else {
                    None
                }
            })
        }
    }

    fn marker_suggestions(markers: &[(Option<String>, u8)]) -> Vec<String> {
        markers
            .iter()
            .flat_map(|(name, idx)| {
                [name.clone(), Some(format!("#{idx}"))]
                    .into_iter()
                    .flatten()
            })
            .collect()
    }

    let wcp_start_or_stop = if state
        .wcp_running_signal
        .load(std::sync::atomic::Ordering::Relaxed)
    {
        "wcp_server_stop"
    } else {
        "wcp_server_start"
    };
    #[cfg(target_arch = "wasm32")]
    let _ = wcp_start_or_stop;

    let keep_during_reload = state.user.config.behavior.keep_during_reload;
    let commands = if state.user.waves.is_some() {
        vec![
            "load_file",
            "load_url",
            #[cfg(not(target_arch = "wasm32"))]
            "load_state",
            "switch_file",
            "variable_add",
            "generator_add",
            "item_focus",
            "item_set_color",
            "item_set_background_color",
            "item_set_format",
            "item_unset_color",
            "item_unset_background_color",
            "item_unfocus",
            "item_rename",
            "zoom_fit",
            "scope_add",
            "scope_add_recursive",
            "scope_select",
            "stream_add",
            "stream_select",
            "divider_add",
            "config_reload",
            "theme_select",
            #[cfg(not(target_arch = "wasm32"))]
            "reload",
            "remove_unavailable",
            "show_controls",
            "show_mouse_gestures",
            "show_quick_start",
            "show_logs",
            #[cfg(feature = "performance_plot")]
            "show_performance",
            "scroll_to_start",
            "scroll_to_end",
            "goto_start",
            "goto_end",
            "zoom_in",
            "zoom_out",
            "toggle_menu",
            "toggle_side_panel",
            "toggle_fullscreen",
            "toggle_tick_lines",
            "variable_add_from_scope",
            "generator_add_from_stream",
            "variable_set_name_type",
            "variable_force_name_type",
            "preference_set_clock_highlight",
            "preference_set_hierarchy_style",
            "preference_set_arrow_key_bindings",
            "goto_cursor",
            "goto_marker",
            "dump_tree",
            "group_marked",
            "group_dissolve",
            "group_fold_recursive",
            "group_unfold_recursive",
            "group_fold_all",
            "group_unfold_all",
            "save_state",
            "save_state_as",
            "timeline_add",
            "cursor_set",
            "marker_set",
            "marker_remove",
            "show_marker_window",
            "viewport_add",
            "viewport_remove",
            "transition_next",
            "transition_previous",
            "transaction_next",
            "transaction_prev",
            "copy_value",
            "pause_simulation",
            "unpause_simulation",
            "undo",
            "redo",
            #[cfg(not(target_arch = "wasm32"))]
            wcp_start_or_stop,
            #[cfg(not(target_arch = "wasm32"))]
            "exit",
        ]
    } else {
        vec![
            "load_file",
            "load_url",
            #[cfg(not(target_arch = "wasm32"))]
            "load_state",
            "config_reload",
            "theme_select",
            "toggle_menu",
            "toggle_side_panel",
            "toggle_fullscreen",
            "show_controls",
            "show_mouse_gestures",
            "show_quick_start",
            "show_logs",
            #[cfg(feature = "performance_plot")]
            "show_performance",
            #[cfg(not(target_arch = "wasm32"))]
            wcp_start_or_stop,
            #[cfg(not(target_arch = "wasm32"))]
            "exit",
        ]
    };
    let mut theme_names = state.user.config.theme.theme_names.clone();
    let state_file = state.user.state_file.clone();
    theme_names.insert(0, "default".to_string());
    Command::NonTerminal(
        ParamGreed::Word,
        commands.into_iter().map(std::convert::Into::into).collect(),
        Box::new(move |query, _| {
            let variables_in_active_scope = variables_in_active_scope.clone();
            let markers = markers.clone();
            let scopes = scopes.clone();
            let active_scope = active_scope.clone();
            let is_transaction_container = is_transaction_container;
            match query {
                "load_file" => single_word_delayed_suggestions(
                    Box::new(all_wave_files),
                    Box::new(|word| {
                        Some(Command::Terminal(Message::LoadFile(
                            word.into(),
                            LoadOptions::clean(),
                        )))
                    }),
                ),
                "switch_file" => single_word_delayed_suggestions(
                    Box::new(all_wave_files),
                    Box::new(|word| {
                        Some(Command::Terminal(Message::LoadFile(
                            word.into(),
                            LoadOptions {
                                keep_variables: true,
                                keep_unavailable: false,
                            },
                        )))
                    }),
                ),
                "load_url" => Some(Command::NonTerminal(
                    ParamGreed::Rest,
                    vec![],
                    Box::new(|query, _| {
                        Some(Command::Terminal(Message::LoadWaveformFileFromUrl(
                            query.to_string(),
                            LoadOptions::clean(), // load_url does not indicate any format restrictions
                        )))
                    }),
                )),
                "config_reload" => Some(Command::Terminal(Message::ReloadConfig)),
                "theme_select" => single_word(
                    theme_names.clone(),
                    Box::new(|word| {
                        Some(Command::Terminal(Message::SelectTheme(Some(
                            word.to_owned(),
                        ))))
                    }),
                ),
                "scroll_to_start" | "goto_start" => {
                    Some(Command::Terminal(Message::GoToStart { viewport_idx: 0 }))
                }
                "scroll_to_end" | "goto_end" => {
                    Some(Command::Terminal(Message::GoToEnd { viewport_idx: 0 }))
                }
                "zoom_in" => Some(Command::Terminal(Message::CanvasZoom {
                    mouse_ptr: None,
                    delta: 0.5,
                    viewport_idx: 0,
                })),
                "zoom_out" => Some(Command::Terminal(Message::CanvasZoom {
                    mouse_ptr: None,
                    delta: 2.0,
                    viewport_idx: 0,
                })),
                "zoom_fit" => Some(Command::Terminal(Message::ZoomToFit { viewport_idx: 0 })),
                "toggle_menu" => Some(Command::Terminal(Message::ToggleMenu)),
                "toggle_side_panel" => Some(Command::Terminal(Message::ToggleSidePanel)),
                "toggle_fullscreen" => Some(Command::Terminal(Message::ToggleFullscreen)),
                "toggle_tick_lines" => Some(Command::Terminal(Message::ToggleTickLines)),
                // scope commands
                "scope_add" | "module_add" | "stream_add" | "scope_add_recursive" => {
                    let recursive = query == "scope_add_recursive";
                    if is_transaction_container {
                        if recursive {
                            warn!("Cannot recursively add transaction containers");
                        }
                        single_word(
                            scopes,
                            Box::new(|word| {
                                Some(Command::Terminal(Message::AddAllFromStreamScope(
                                    word.to_string(),
                                )))
                            }),
                        )
                    } else {
                        single_word(
                            scopes,
                            Box::new(move |word| {
                                Some(Command::Terminal(Message::AddScope(
                                    ScopeRef::from_hierarchy_string(word),
                                    recursive,
                                )))
                            }),
                        )
                    }
                }
                "scope_select" | "stream_select" => {
                    if is_transaction_container {
                        single_word(
                            scopes.clone(),
                            Box::new(|word| {
                                let scope = if word == "tr" {
                                    ScopeType::StreamScope(StreamScopeRef::Root)
                                } else {
                                    ScopeType::StreamScope(StreamScopeRef::Empty(word.to_string()))
                                };
                                Some(Command::Terminal(Message::SetActiveScope(scope)))
                            }),
                        )
                    } else {
                        single_word(
                            scopes.clone(),
                            Box::new(|word| {
                                Some(Command::Terminal(Message::SetActiveScope(
                                    ScopeType::WaveScope(ScopeRef::from_hierarchy_string(word)),
                                )))
                            }),
                        )
                    }
                }
                "reload" => Some(Command::Terminal(Message::ReloadWaveform(
                    keep_during_reload,
                ))),
                "remove_unavailable" => Some(Command::Terminal(Message::RemovePlaceholders)),
                // Variable commands
                "variable_add" | "generator_add" => {
                    if is_transaction_container {
                        single_word(
                            variables.clone(),
                            Box::new(|word| {
                                Some(Command::Terminal(Message::AddStreamOrGeneratorFromName(
                                    None,
                                    word.to_string(),
                                )))
                            }),
                        )
                    } else {
                        single_word(
                            variables.clone(),
                            Box::new(|word| {
                                Some(Command::Terminal(Message::AddVariables(vec![
                                    VariableRef::from_hierarchy_string(word),
                                ])))
                            }),
                        )
                    }
                }
                "variable_add_from_scope" | "generator_add_from_stream" => single_word(
                    variables_in_active_scope
                        .into_iter()
                        .map(|s| s.name())
                        .collect(),
                    Box::new(move |name| {
                        active_scope.as_ref().map(|scope| match scope {
                            ScopeType::WaveScope(w) => Command::Terminal(Message::AddVariables(
                                vec![VariableRef::new(w.clone(), name.to_string())],
                            )),
                            ScopeType::StreamScope(stream_scope) => {
                                Command::Terminal(Message::AddStreamOrGeneratorFromName(
                                    Some(stream_scope.clone()),
                                    name.to_string(),
                                ))
                            }
                        })
                    }),
                ),
                "item_set_color" => single_word(
                    color_names.clone(),
                    Box::new(|word| {
                        Some(Command::Terminal(Message::ItemColorChange(
                            MessageTarget::CurrentSelection,
                            Some(word.to_string()),
                        )))
                    }),
                ),
                "item_set_background_color" => single_word(
                    color_names.clone(),
                    Box::new(|word| {
                        Some(Command::Terminal(Message::ItemBackgroundColorChange(
                            MessageTarget::CurrentSelection,
                            Some(word.to_string()),
                        )))
                    }),
                ),
                "item_unset_color" => Some(Command::Terminal(Message::ItemColorChange(
                    MessageTarget::CurrentSelection,
                    None,
                ))),
                "item_set_format" => single_word(
                    format_names.clone(),
                    Box::new(|word| {
                        Some(Command::Terminal(Message::VariableFormatChange(
                            MessageTarget::CurrentSelection,
                            word.to_string(),
                        )))
                    }),
                ),
                "item_unset_background_color" => Some(Command::Terminal(
                    Message::ItemBackgroundColorChange(MessageTarget::CurrentSelection, None),
                )),
                "item_rename" => Some(Command::Terminal(Message::RenameItem(None))),
                "variable_set_name_type" => single_word(
                    vec![
                        "Local".to_string(),
                        "Unique".to_string(),
                        "Global".to_string(),
                    ],
                    Box::new(|word| {
                        Some(Command::Terminal(Message::ChangeVariableNameType(
                            MessageTarget::CurrentSelection,
                            VariableNameType::from_str(word).unwrap_or(VariableNameType::Local),
                        )))
                    }),
                ),
                "variable_force_name_type" => single_word(
                    vec![
                        "Local".to_string(),
                        "Unique".to_string(),
                        "Global".to_string(),
                    ],
                    Box::new(|word| {
                        Some(Command::Terminal(Message::ForceVariableNameTypes(
                            VariableNameType::from_str(word).unwrap_or(VariableNameType::Local),
                        )))
                    }),
                ),
                "item_focus" => single_word(
                    displayed_items.clone(),
                    Box::new(|word| {
                        // split off the idx which is always followed by an underscore
                        let alpha_idx: String = word.chars().take_while(|c| *c != '_').collect();
                        alpha_idx_to_uint_idx(alpha_idx)
                            .map(|idx| Command::Terminal(Message::FocusItem(idx)))
                    }),
                ),
                "transition_next" => single_word(
                    displayed_items.clone(),
                    Box::new(|word| {
                        // split off the idx which is always followed by an underscore
                        let alpha_idx: String = word.chars().take_while(|c| *c != '_').collect();
                        alpha_idx_to_uint_idx(alpha_idx).map(|idx| {
                            Command::Terminal(Message::MoveCursorToTransition {
                                next: true,
                                variable: Some(idx),
                                skip_zero: false,
                            })
                        })
                    }),
                ),
                "transition_previous" => single_word(
                    displayed_items.clone(),
                    Box::new(|word| {
                        // split off the idx which is always followed by an underscore
                        let alpha_idx: String = word.chars().take_while(|c| *c != '_').collect();
                        alpha_idx_to_uint_idx(alpha_idx).map(|idx| {
                            Command::Terminal(Message::MoveCursorToTransition {
                                next: false,
                                variable: Some(idx),
                                skip_zero: false,
                            })
                        })
                    }),
                ),
                "transaction_next" => {
                    Some(Command::Terminal(Message::MoveTransaction { next: true }))
                }
                "transaction_prev" => {
                    Some(Command::Terminal(Message::MoveTransaction { next: false }))
                }
                "copy_value" => single_word(
                    displayed_items.clone(),
                    Box::new(|word| {
                        // split off the idx which is always followed by an underscore
                        let alpha_idx: String = word.chars().take_while(|c| *c != '_').collect();
                        alpha_idx_to_uint_idx(alpha_idx).map(|idx| {
                            Command::Terminal(Message::VariableValueToClipbord(
                                MessageTarget::Explicit(idx),
                            ))
                        })
                    }),
                ),
                "preference_set_clock_highlight" => single_word(
                    ["Line", "Cycle", "None"]
                        .iter()
                        .map(std::string::ToString::to_string)
                        .collect_vec(),
                    Box::new(|word| {
                        Some(Command::Terminal(Message::SetClockHighlightType(
                            ClockHighlightType::from_str(word).unwrap_or(ClockHighlightType::Line),
                        )))
                    }),
                ),
                "preference_set_hierarchy_style" => single_word(
                    enum_iterator::all::<HierarchyStyle>()
                        .map(|o| o.to_string())
                        .collect_vec(),
                    Box::new(|word| {
                        Some(Command::Terminal(Message::SetHierarchyStyle(
                            HierarchyStyle::from(word.to_string()),
                        )))
                    }),
                ),
                "preference_set_arrow_key_bindings" => single_word(
                    enum_iterator::all::<ArrowKeyBindings>()
                        .map(|o| o.to_string())
                        .collect_vec(),
                    Box::new(|word| {
                        Some(Command::Terminal(Message::SetArrowKeyBindings(
                            ArrowKeyBindings::from(word.to_string()),
                        )))
                    }),
                ),
                "item_unfocus" => Some(Command::Terminal(Message::UnfocusItem)),
                "divider_add" => optional_single_word(
                    vec![],
                    Box::new(|word| {
                        Some(Command::Terminal(Message::AddDivider(
                            Some(word.into()),
                            None,
                        )))
                    }),
                ),
                "timeline_add" => Some(Command::Terminal(Message::AddTimeLine(None))),
                "goto_cursor" => Some(Command::Terminal(Message::GoToCursorIfNotInView)),
                "goto_marker" => single_word(
                    marker_suggestions(&markers),
                    Box::new(move |name| {
                        parse_marker(name, &markers)
                            .map(|idx| Command::Terminal(Message::GoToMarkerPosition(idx, 0)))
                    }),
                ),
                "dump_tree" => Some(Command::Terminal(Message::DumpTree)),
                "group_marked" => optional_single_word(
                    vec![],
                    Box::new(|name| {
                        let trimmed = name.trim();
                        Some(Command::Terminal(Message::GroupNew {
                            name: (!trimmed.is_empty()).then_some(trimmed.to_owned()),
                            before: None,
                            items: None,
                        }))
                    }),
                ),
                "group_dissolve" => Some(Command::Terminal(Message::GroupDissolve(None))),
                "group_fold_recursive" => {
                    Some(Command::Terminal(Message::GroupFoldRecursive(None)))
                }
                "group_unfold_recursive" => {
                    Some(Command::Terminal(Message::GroupUnfoldRecursive(None)))
                }
                "group_fold_all" => Some(Command::Terminal(Message::GroupFoldAll)),
                "group_unfold_all" => Some(Command::Terminal(Message::GroupUnfoldAll)),
                "show_controls" => Some(Command::Terminal(Message::SetKeyHelpVisible(true))),
                "show_mouse_gestures" => {
                    Some(Command::Terminal(Message::SetGestureHelpVisible(true)))
                }
                "show_quick_start" => Some(Command::Terminal(Message::SetQuickStartVisible(true))),
                #[cfg(feature = "performance_plot")]
                "show_performance" => optional_single_word(
                    vec![],
                    Box::new(|word| {
                        if word == "redraw" {
                            Some(Command::Terminal(Message::Batch(vec![
                                Message::SetPerformanceVisible(true),
                                Message::SetContinuousRedraw(true),
                            ])))
                        } else {
                            Some(Command::Terminal(Message::SetPerformanceVisible(true)))
                        }
                    }),
                ),
                "cursor_set" => single_word(
                    vec![],
                    Box::new(|time_str| match time_str.parse() {
                        Ok(time) => Some(Command::Terminal(Message::Batch(vec![
                            Message::CursorSet(time),
                            Message::GoToCursorIfNotInView,
                        ]))),
                        _ => None,
                    }),
                ),
                "marker_set" => Some(Command::NonTerminal(
                    ParamGreed::Custom(&separate_at_space),
                    // FIXME use once fzcmd does not enforce suggestion match, as of now we couldn't add a marker (except the first)
                    // marker_suggestions(&markers),
                    vec![],
                    Box::new(move |name, _| {
                        let marker_id = parse_marker(name, &markers);
                        let name = name.to_owned();

                        Some(Command::NonTerminal(
                            ParamGreed::Word,
                            vec![],
                            Box::new(move |time_str, _| {
                                let time = time_str.parse().ok()?;
                                match marker_id {
                                    Some(id) => {
                                        Some(Command::Terminal(Message::SetMarker { id, time }))
                                    }
                                    None => Some(Command::Terminal(Message::AddMarker {
                                        time,
                                        name: Some(name.clone()),
                                        move_focus: true,
                                    })),
                                }
                            }),
                        ))
                    }),
                )),
                "marker_remove" => Some(Command::NonTerminal(
                    ParamGreed::Rest,
                    marker_suggestions(&markers),
                    Box::new(move |name, _| {
                        let marker_id = parse_marker(name, &markers)?;
                        Some(Command::Terminal(Message::RemoveMarker(marker_id)))
                    }),
                )),
                "show_marker_window" => {
                    Some(Command::Terminal(Message::SetCursorWindowVisible(true)))
                }
                "show_logs" => Some(Command::Terminal(Message::SetLogsVisible(true))),
                "save_state" => Some(Command::Terminal(Message::SaveStateFile(
                    state_file.clone(),
                ))),
                "save_state_as" => single_word(
                    vec![],
                    Box::new(|word| {
                        Some(Command::Terminal(Message::SaveStateFile(Some(
                            std::path::Path::new(word).into(),
                        ))))
                    }),
                ),
                "load_state" => single_word(
                    vec![],
                    Box::new(|word| {
                        Some(Command::Terminal(Message::LoadStateFile(Some(
                            std::path::Path::new(word).into(),
                        ))))
                    }),
                ),
                "viewport_add" => Some(Command::Terminal(Message::AddViewport)),
                "viewport_remove" => Some(Command::Terminal(Message::RemoveViewport)),
                "pause_simulation" => Some(Command::Terminal(Message::PauseSimulation)),
                "unpause_simulation" => Some(Command::Terminal(Message::UnpauseSimulation)),
                "undo" => Some(Command::Terminal(Message::Undo(1))),
                "redo" => Some(Command::Terminal(Message::Redo(1))),
                "wcp_server_start" => Some(Command::Terminal(Message::StartWcpServer {
                    address: None,
                    initiate: false,
                })),
                "wcp_server_stop" => Some(Command::Terminal(Message::StopWcpServer)),
                "exit" => Some(Command::Terminal(Message::Exit)),
                _ => None,
            }
        }),
    )
}
