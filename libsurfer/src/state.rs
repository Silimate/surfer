use std::{
    collections::{HashMap, HashSet, VecDeque},
    mem,
    path::PathBuf,
};

use crate::displayed_item_tree::VisibleItemIndex;
#[cfg(feature = "spade")]
use crate::translation::spade::SpadeTranslator;
use crate::{
    channels::IngressReceiver,
    command_prompt::get_parser,
    config,
    data_container::DataContainer,
    dialog::ReloadWaveformDialog,
    displayed_item_tree::DisplayedItemTree,
    message::Message,
    system_state::SystemState,
    time::{TimeStringFormatting, TimeUnit},
    transaction_container::TransactionContainer,
    variable_name_filter::VariableNameFilterType,
    viewport::Viewport,
    wasm_util::perform_work,
    wave_container::{ScopeRef, VariableRef, WaveContainer},
    wave_data::WaveData,
    wave_source::{LoadOptions, WaveFormat, WaveSource},
    CanvasState, StartupParams,
};
use color_eyre::{eyre::Context, Result};
use egui::{
    style::{Selection, WidgetVisuals, Widgets},
    Rounding, Stroke, Visuals,
};
use fzcmd::parse_command;
use itertools::Itertools;
use log::{error, info, trace, warn};
use ron::ser::PrettyConfig;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct State {
    #[serde(skip)]
    pub config: config::SurferConfig,

    /// Overrides for the config show_* fields. Defaults to `config.show_*` if not present
    pub(crate) show_hierarchy: Option<bool>,
    pub(crate) show_menu: Option<bool>,
    pub(crate) show_ticks: Option<bool>,
    pub(crate) show_toolbar: Option<bool>,
    pub(crate) show_tooltip: Option<bool>,
    pub(crate) show_scope_tooltip: Option<bool>,
    pub(crate) show_default_timeline: Option<bool>,
    pub(crate) show_overview: Option<bool>,
    pub(crate) show_statusbar: Option<bool>,
    pub(crate) align_names_right: Option<bool>,
    pub(crate) show_variable_indices: Option<bool>,
    pub(crate) show_variable_direction: Option<bool>,
    pub(crate) show_empty_scopes: Option<bool>,
    pub(crate) show_parameters_in_scopes: Option<bool>,

    pub(crate) waves: Option<WaveData>,
    pub(crate) drag_started: bool,
    pub(crate) drag_source_idx: Option<VisibleItemIndex>,
    pub(crate) drag_target_idx: Option<crate::displayed_item_tree::TargetPosition>,

    pub(crate) previous_waves: Option<WaveData>,

    /// Count argument for movements
    pub(crate) count: Option<String>,

    // Vector of translators which have failed at the `translates` function for a variable.
    pub(crate) blacklisted_translators: HashSet<(VariableRef, String)>,

    pub(crate) show_about: bool,
    pub(crate) show_keys: bool,
    pub(crate) show_gestures: bool,
    pub(crate) show_quick_start: bool,
    pub(crate) show_license: bool,
    pub(crate) show_performance: bool,
    pub(crate) show_logs: bool,
    pub(crate) show_cursor_window: bool,
    pub(crate) wanted_timeunit: TimeUnit,
    pub(crate) time_string_format: Option<TimeStringFormatting>,
    pub(crate) show_url_entry: bool,
    /// Show a confirmation dialog asking the user for confirmation
    /// that surfer should reload changed files from disk.
    #[serde(skip, default)]
    pub(crate) show_reload_suggestion: Option<ReloadWaveformDialog>,
    pub(crate) variable_name_filter_focused: bool,
    pub(crate) variable_name_filter_type: VariableNameFilterType,
    pub(crate) variable_name_filter_case_insensitive: bool,
    pub(crate) rename_target: Option<VisibleItemIndex>,
    //Sidepanel width
    pub(crate) sidepanel_width: Option<f32>,
    /// UI zoom factor if set by the user
    pub(crate) ui_zoom_factor: Option<f32>,

    // Path of last saved-to state file
    // Do not serialize as this causes a few issues and doesn't help:
    // - We need to set it on load of a state anyways since the file could have been renamed
    // - Bad interoperatility story between native and wasm builds
    // - Sequencing issue in serialization, due to us having to run that async
    #[serde(skip)]
    pub state_file: Option<PathBuf>,

    /// Internal state that does not persist between sessions and is not serialized
    #[serde(skip, default = "SystemState::new")]
    pub sys: SystemState,
}

impl State {
    pub fn new() -> Result<State> {
        Self::new_inner(false)
    }

    #[cfg(test)]
    pub(crate) fn new_default_config() -> Result<State> {
        Self::new_inner(true)
    }

    fn new_inner(force_default_config: bool) -> Result<State> {
        let config = config::SurferConfig::new(force_default_config)
            .with_context(|| "Failed to load config file")?;
        let result = State {
            sys: SystemState::new(),
            config,
            waves: None,
            previous_waves: None,
            count: None,
            blacklisted_translators: HashSet::new(),
            show_about: false,
            show_keys: false,
            show_gestures: false,
            show_performance: false,
            show_license: false,
            show_logs: false,
            show_cursor_window: false,
            wanted_timeunit: TimeUnit::None,
            time_string_format: None,
            show_url_entry: false,
            show_quick_start: false,
            show_reload_suggestion: None,
            rename_target: None,
            variable_name_filter_focused: false,
            variable_name_filter_type: VariableNameFilterType::Fuzzy,
            variable_name_filter_case_insensitive: true,
            ui_zoom_factor: None,
            show_hierarchy: None,
            show_menu: None,
            show_ticks: None,
            show_toolbar: None,
            show_tooltip: None,
            show_scope_tooltip: None,
            show_default_timeline: None,
            show_overview: None,
            show_statusbar: None,
            show_variable_direction: None,
            align_names_right: None,
            show_variable_indices: None,
            show_empty_scopes: None,
            show_parameters_in_scopes: None,
            drag_started: false,
            drag_source_idx: None,
            drag_target_idx: None,
            state_file: None,
            sidepanel_width: None,
        };

        Ok(result)
    }

    pub fn with_params(mut self, args: StartupParams) -> Self {
        self.previous_waves = self.waves;
        self.waves = None;

        // Long running translators which we load in a thread
        {
            #[cfg(feature = "spade")]
            let sender = self.sys.channels.msg_sender.clone();
            #[cfg(not(feature = "spade"))]
            let _ = self.sys.channels.msg_sender.clone();
            let waves = args.waves.clone();
            perform_work(move || {
                #[cfg(feature = "spade")]
                SpadeTranslator::load(&waves, &args.spade_top, &args.spade_state, sender);
                #[cfg(not(feature = "spade"))]
                if let (Some(_), Some(_)) = (args.spade_top, args.spade_state) {
                    info!("Surfer is not compiled with spade support, ignoring spade_top and spade_state");
                }
            });
        }

        // we turn the waveform argument and any startup command file into batch commands
        self.sys.batch_commands = VecDeque::new();

        match args.waves {
            Some(WaveSource::Url(url)) => {
                self.add_startup_message(Message::LoadWaveformFileFromUrl(
                    url,
                    LoadOptions::clean(),
                ));
            }
            Some(WaveSource::File(file)) => {
                self.add_startup_message(Message::LoadFile(file, LoadOptions::clean()));
            }
            Some(WaveSource::Data) => error!("Attempted to load data at startup"),
            Some(WaveSource::Cxxrtl(url)) => {
                self.add_startup_message(Message::SetupCxxrtl(url));
            }
            Some(WaveSource::DragAndDrop(_)) => {
                error!("Attempted to load from drag and drop at startup (how?)");
            }
            None => {}
        }

        self.add_startup_commands(args.startup_commands);

        self
    }

    pub fn add_startup_commands<I: IntoIterator<Item = String>>(&mut self, commands: I) {
        let parsed = self.parse_startup_commands(commands);
        for msg in parsed {
            self.sys.batch_commands.push_back(msg);
            self.sys.batch_commands_completed = false;
        }
    }

    pub fn add_startup_messages<I: IntoIterator<Item = Message>>(&mut self, messages: I) {
        for msg in messages {
            self.sys.batch_commands.push_back(msg);
            self.sys.batch_commands_completed = false;
        }
    }

    pub fn add_startup_message(&mut self, msg: Message) {
        self.add_startup_messages([msg]);
    }

    pub fn wcp(&mut self) {
        self.handle_wcp_commands();
    }

    pub(crate) fn add_scope(&mut self, scope: ScopeRef, recursive: bool) {
        let Some(waves) = self.waves.as_mut() else {
            warn!("Adding scope without waves loaded");
            return;
        };

        let wave_cont = waves.inner.as_waves().unwrap();

        let children = wave_cont.child_scopes(&scope);
        let variables = wave_cont
            .variables_in_scope(&scope)
            .iter()
            .sorted_by(|a, b| numeric_sort::cmp(&a.name, &b.name))
            .cloned()
            .collect_vec();

        // TODO add parameter to add_variables, insert to (self.drag_target_idx, self.drag_source_idx)
        if let (Some(cmd), _) = waves.add_variables(&self.sys.translators, variables, None) {
            self.load_variables(cmd);
        }

        if recursive {
            if let Ok(children) = children {
                for child in children {
                    self.add_scope(child, true);
                }
            }
        }
        self.invalidate_draw_commands();
    }

    pub(crate) fn on_waves_loaded(
        &mut self,
        filename: WaveSource,
        format: WaveFormat,
        new_waves: Box<WaveContainer>,
        load_options: LoadOptions,
    ) {
        info!("{format} file loaded");
        let viewport = Viewport::new();
        let viewports = [viewport].to_vec();

        let (new_wave, load_commands) = if load_options.keep_variables && self.waves.is_some() {
            self.waves.take().unwrap().update_with_waves(
                new_waves,
                filename,
                format,
                &self.sys.translators,
                load_options.keep_unavailable,
            )
        } else if let Some(old) = self.previous_waves.take() {
            old.update_with_waves(
                new_waves,
                filename,
                format,
                &self.sys.translators,
                load_options.keep_unavailable,
            )
        } else {
            (
                WaveData {
                    inner: DataContainer::Waves(*new_waves),
                    source: filename,
                    format,
                    active_scope: None,
                    items_tree: DisplayedItemTree::default(),
                    displayed_items: HashMap::new(),
                    viewports,
                    cursor: None,
                    markers: HashMap::new(),
                    focused_item: None,
                    focused_transaction: (None, None),
                    default_variable_name_type: self.config.default_variable_name_type,
                    display_variable_indices: self.show_variable_indices(),
                    scroll_offset: 0.,
                    drawing_infos: vec![],
                    top_item_draw_offset: 0.,
                    total_height: 0.,
                    display_item_ref_counter: 0,
                    old_num_timestamps: None,
                    graphics: HashMap::new(),
                },
                None,
            )
        };
        if let Some(cmd) = load_commands {
            self.load_variables(cmd);
        }
        self.invalidate_draw_commands();

        // Set time unit to the file time unit before consuming new_wave
        self.wanted_timeunit = new_wave.inner.metadata().timescale.unit;

        self.waves = Some(new_wave);
    }

    pub(crate) fn on_transaction_streams_loaded(
        &mut self,
        filename: WaveSource,
        format: WaveFormat,
        new_ftr: TransactionContainer,
        _loaded_options: LoadOptions,
    ) {
        info!("Transaction streams are loaded.");

        let viewport = Viewport::new();
        let viewports = [viewport].to_vec();

        let new_transaction_streams = WaveData {
            inner: DataContainer::Transactions(new_ftr),
            source: filename,
            format,
            active_scope: None,
            items_tree: DisplayedItemTree::default(),
            displayed_items: HashMap::new(),
            viewports,
            cursor: None,
            markers: HashMap::new(),
            focused_item: None,
            focused_transaction: (None, None),
            default_variable_name_type: self.config.default_variable_name_type,
            display_variable_indices: self.show_variable_indices(),
            scroll_offset: 0.,
            drawing_infos: vec![],
            top_item_draw_offset: 0.,
            total_height: 0.,
            display_item_ref_counter: 0,
            old_num_timestamps: None,
            graphics: HashMap::new(),
        };

        self.invalidate_draw_commands();

        self.config.theme.alt_frequency = 0;
        self.wanted_timeunit = new_transaction_streams.inner.metadata().timescale.unit;
        self.waves = Some(new_transaction_streams);
    }

    pub(crate) fn handle_async_messages(&mut self) {
        let mut msgs = vec![];
        loop {
            match self.sys.channels.msg_receiver.try_recv() {
                Ok(msg) => msgs.push(msg),
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    trace!("Message sender disconnected");
                    break;
                }
            }
        }

        while let Some(msg) = msgs.pop() {
            self.update(msg);
        }
    }

    /// After user messages are addressed, we try to execute batch commands as they are ready to run
    pub(crate) fn handle_batch_commands(&mut self) {
        // we only execute commands while we aren't waiting for background operations to complete
        while self.can_start_batch_command() {
            if let Some(cmd) = self.sys.batch_commands.pop_front() {
                info!("Applying startup command: {cmd:?}");
                self.update(cmd);
            } else {
                break; // no more messages
            }
        }

        // if there are no messages and all operations have completed, we are done
        if !self.sys.batch_commands_completed
            && self.sys.batch_commands.is_empty()
            && self.can_start_batch_command()
        {
            self.sys.batch_commands_completed = true;
        }
    }

    /// Returns whether it is OK to start a new batch command.
    pub(crate) fn can_start_batch_command(&self) -> bool {
        // if the progress tracker is none -> all operations have completed
        self.sys.progress_tracker.is_none()
    }

    pub fn get_visuals(&self) -> Visuals {
        let widget_style = WidgetVisuals {
            bg_fill: self.config.theme.secondary_ui_color.background,
            fg_stroke: Stroke {
                color: self.config.theme.secondary_ui_color.foreground,
                width: 1.0,
            },
            weak_bg_fill: self.config.theme.secondary_ui_color.background,
            bg_stroke: Stroke {
                color: self.config.theme.border_color,
                width: 1.0,
            },
            rounding: Rounding::same(2.),
            expansion: 0.0,
        };

        Visuals {
            override_text_color: Some(self.config.theme.foreground),
            extreme_bg_color: self.config.theme.secondary_ui_color.background,
            panel_fill: self.config.theme.secondary_ui_color.background,
            window_fill: self.config.theme.primary_ui_color.background,
            window_rounding: Rounding::ZERO,
            menu_rounding: Rounding::ZERO,
            window_stroke: Stroke {
                width: 1.0,
                color: self.config.theme.border_color,
            },
            selection: Selection {
                bg_fill: self.config.theme.selected_elements_colors.background,
                stroke: Stroke {
                    color: self.config.theme.selected_elements_colors.foreground,
                    width: 1.0,
                },
            },
            widgets: Widgets {
                noninteractive: widget_style,
                inactive: widget_style,
                hovered: widget_style,
                active: widget_style,
                open: widget_style,
            },
            ..Visuals::dark()
        }
    }

    pub(crate) fn encode_state(&self) -> Option<String> {
        let opt = ron::Options::default();
        opt.to_string_pretty(self, PrettyConfig::default())
            .context("Failed to encode state")
            .map_err(|e| error!("Failed to encode state. {e:#?}"))
            .ok()
    }

    pub(crate) fn load_state(&mut self, mut loaded_state: crate::State, path: Option<PathBuf>) {
        // first swap everything, fix special cases afterwards
        mem::swap(self, &mut loaded_state);

        // system state is not exported and instance specific, swap back
        // we need to do this before fixing wave files which e.g. use the translator list
        mem::swap(&mut self.sys, &mut loaded_state.sys);
        // the config is also not exported and instance specific, swap back
        mem::swap(&mut self.config, &mut loaded_state.config);

        // swap back waves for inner, source, format since we want to keep the file
        // fix up all wave references from paths if a wave is loaded
        mem::swap(&mut loaded_state.waves, &mut self.waves);
        let load_commands = if let (Some(waves), Some(new_waves)) =
            (&mut self.waves, &mut loaded_state.waves)
        {
            mem::swap(&mut waves.active_scope, &mut new_waves.active_scope);
            let items = std::mem::take(&mut new_waves.displayed_items);
            let items_tree = std::mem::take(&mut new_waves.items_tree);
            let load_commands = waves.update_with_items(&items, items_tree, &self.sys.translators);

            mem::swap(&mut waves.viewports, &mut new_waves.viewports);
            mem::swap(&mut waves.cursor, &mut new_waves.cursor);
            mem::swap(&mut waves.markers, &mut new_waves.markers);
            mem::swap(&mut waves.focused_item, &mut new_waves.focused_item);
            waves.default_variable_name_type = new_waves.default_variable_name_type;
            waves.scroll_offset = new_waves.scroll_offset;
            load_commands
        } else {
            None
        };
        if let Some(load_commands) = load_commands {
            self.load_variables(load_commands);
        };

        // reset drag to avoid confusion
        self.drag_started = false;
        self.drag_source_idx = None;
        self.drag_target_idx = None;

        // reset previous_waves & count to prevent unintuitive state here
        self.previous_waves = None;
        self.count = None;

        // use just loaded path since path is not part of the export as it might have changed anyways
        self.state_file = path;
        self.rename_target = None;

        self.invalidate_draw_commands();
        if let Some(waves) = &mut self.waves {
            waves.update_viewports();
        }
    }

    /// Returns true if the waveform and all requested signals have been loaded.
    /// Used for testing to make sure the GUI is at its final state before taking a
    /// snapshot.
    pub fn waves_fully_loaded(&self) -> bool {
        self.waves
            .as_ref()
            .is_some_and(|w| w.inner.is_fully_loaded())
    }

    /// Returns true once all batch commands have been completed and their effects are all executed.
    pub fn batch_commands_completed(&self) -> bool {
        debug_assert!(
            self.sys.batch_commands_completed || !self.sys.batch_commands.is_empty(),
            "completed implies no commands"
        );
        self.sys.batch_commands_completed
    }

    fn parse_startup_commands<I: IntoIterator<Item = String>>(&mut self, cmds: I) -> Vec<Message> {
        trace!("Parsing startup commands");
        let parsed = cmds
            .into_iter()
            // Add line numbers
            .enumerate()
            // trace
            .map(|(no, line)| {
                trace!("{no: >2} {line}");
                (no, line)
            })
            // Make the line numbers start at 1 as is tradition
            .map(|(no, line)| (no + 1, line))
            .map(|(no, line)| (no, line.trim().to_string()))
            // NOTE: Safe unwrap. Split will always return one element
            .map(|(no, line)| (no, line.split('#').next().unwrap().to_string()))
            .filter(|(_no, line)| !line.is_empty())
            .flat_map(|(no, line)| {
                line.split(';')
                    .map(|cmd| (no, cmd.to_string()))
                    .collect::<Vec<_>>()
            })
            .filter_map(|(no, command)| {
                parse_command(&command, get_parser(self))
                    .map_err(|e| {
                        error!("Error on startup commands line {no}: {e:#?}");
                        e
                    })
                    .ok()
            })
            .collect::<Vec<_>>();

        parsed
    }

    /// Returns the current canvas state
    pub(crate) fn current_canvas_state(waves: &WaveData, message: String) -> CanvasState {
        CanvasState {
            message,
            focused_item: waves.focused_item,
            focused_transaction: waves.focused_transaction.clone(),
            items_tree: waves.items_tree.clone(),
            displayed_items: waves.displayed_items.clone(),
            markers: waves.markers.clone(),
        }
    }

    /// Push the current canvas state to the undo stack
    pub(crate) fn save_current_canvas(&mut self, message: String) {
        if let Some(waves) = &self.waves {
            self.sys
                .undo_stack
                .push(State::current_canvas_state(waves, message));

            if self.sys.undo_stack.len() > self.config.undo_stack_size {
                self.sys.undo_stack.remove(0);
            }
            self.sys.redo_stack.clear();
        }
    }

    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) fn start_wcp_server(&mut self, address: Option<String>, initiate: bool) {
        use std::thread;

        use wcp::wcp_server::WcpServer;

        use crate::wcp;

        if self.sys.wcp_server_thread.as_ref().is_some()
            || self
                .sys
                .wcp_running_signal
                .load(std::sync::atomic::Ordering::Relaxed)
        {
            warn!("WCP HTTP server is already running");
            return;
        }
        // TODO: Consider an unbounded channel?
        let (wcp_s2c_sender, wcp_s2c_receiver) = tokio::sync::mpsc::channel(100);
        let (wcp_c2s_sender, wcp_c2s_receiver) = tokio::sync::mpsc::channel(100);
        self.sys.channels.wcp_c2s_receiver = Some(IngressReceiver::new(wcp_s2c_receiver));
        self.sys.channels.wcp_s2c_sender = Some(wcp_c2s_sender);
        let stop_signal_copy = self.sys.wcp_stop_signal.clone();
        stop_signal_copy.store(false, std::sync::atomic::Ordering::Relaxed);
        let running_signal_copy = self.sys.wcp_running_signal.clone();
        running_signal_copy.store(true, std::sync::atomic::Ordering::Relaxed);

        let ctx = self.sys.context.clone();
        let address = address.unwrap_or(self.config.wcp.address.clone());
        self.sys.wcp_server_address = Some(address.clone());
        self.sys.wcp_server_thread = Some(thread::spawn(move || {
            let server = WcpServer::new(
                address,
                initiate,
                wcp_s2c_sender,
                wcp_c2s_receiver,
                stop_signal_copy,
                running_signal_copy,
                ctx,
            );
            match server {
                Ok(mut server) => server.run(),
                Err(m) => {
                    error!("Could not start WCP server. {m:?}")
                }
            }
        }));
    }

    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) fn stop_wcp_server(&mut self) {
        // stop wcp server if there is one running

        use std::net::TcpStream;
        if let Some(address) = &self.sys.wcp_server_address {
            if self.sys.wcp_server_thread.is_some() {
                // signal the server to stop
                self.sys
                    .wcp_stop_signal
                    .store(true, std::sync::atomic::Ordering::Relaxed);
                // wake up server to register stop signal
                let _ = TcpStream::connect(address);

                self.sys.wcp_server_thread = None;
                self.sys.wcp_server_address = None;
                self.sys.channels.wcp_s2c_sender = None;
                self.sys.channels.wcp_c2s_receiver = None;
                info!("Stopped WCP server");
            }
        }
    }
}

// Impl needed since for loading we need to put State into a Message
// Snip out the actual contents to not completely spam the terminal
impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "State {{ <snipped> }}")
    }
}
