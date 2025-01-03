#![cfg_attr(not(target_arch = "wasm32"), deny(unused_crate_dependencies))]

#[cfg(not(target_arch = "wasm32"))]
mod main_impl {
    use camino::Utf8PathBuf;
    use clap::Parser;
    use color_eyre::eyre::Context;
    use color_eyre::Result;
    use egui::Vec2;
    use libsurfer::{
        file_watcher::FileWatcher, logs, message::Message, run_egui,
        wave_source::string_to_wavesource, wave_source::WaveSource, StartupParams, State,
    };
    use log::error;

    #[derive(clap::Subcommand)]
    enum Commands {
        #[cfg(not(target_arch = "wasm32"))]
        /// starts surfer in headless mode so that a user can connect to it
        Server {
            /// port on which server will listen
            #[clap(long)]
            port: Option<u16>,
            /// token used by the client to authenticate to the server
            #[clap(long)]
            token: Option<String>,
            /// waveform file that we want to serve
            #[arg(long)]
            file: String,
        },
    }

    #[derive(clap::Parser, Default)]
    #[command(version, about)]
    struct Args {
        /// Waveform file in VCD, FST, or GHW format.
        wave_file: Option<String>,
        #[cfg(feature = "spade")]
        #[clap(long)]
        /// Load Spade state file
        spade_state: Option<Utf8PathBuf>,
        #[cfg(feature = "spade")]
        #[clap(long)]
        /// Specify Spade top-level entity
        spade_top: Option<String>,
        /// Path to a file containing 'commands' to run after a waveform has been loaded.
        /// The commands are the same as those used in the command line interface inside the program.
        /// Commands are separated by lines or ;. Empty lines are ignored. Line comments starting with
        /// `#` are supported
        /// NOTE: This feature is not permanent, it will be removed once a solid scripting system
        /// is implemented.
        #[clap(long, short, verbatim_doc_comment)]
        command_file: Option<Utf8PathBuf>,
        /// Alias for --command_file to support VUnit
        #[clap(long)]
        script: Option<Utf8PathBuf>,

        #[clap(long, short)]
        /// Load previously saved state file
        state_file: Option<Utf8PathBuf>,

        #[command(subcommand)]
        command: Option<Commands>,
    }

    impl Args {
        pub fn command_file(&self) -> &Option<Utf8PathBuf> {
            if self.script.is_some() && self.command_file.is_some() {
                error!("At most one of --command_file and --script can be used");
                return &None;
            }
            if self.command_file.is_some() {
                &self.command_file
            } else {
                &self.script
            }
        }
    }

    #[allow(dead_code)] // NOTE: Only used in desktop version
    fn startup_params_from_args(args: Args) -> StartupParams {
        let startup_commands = if let Some(cmd_file) = args.command_file() {
            std::fs::read_to_string(cmd_file)
                .map_err(|e| error!("Failed to read commands from {cmd_file}. {e:#?}"))
                .ok()
                .map(|file_content| {
                    file_content
                        .lines()
                        .map(std::string::ToString::to_string)
                        .collect()
                })
                .unwrap_or_default()
        } else {
            vec![]
        };
        StartupParams {
            #[cfg(feature = "spade")]
            spade_state: args.spade_state,
            #[cfg(feature = "spade")]
            spade_top: args.spade_top,
            #[cfg(not(feature = "spade"))]
            spade_state: None,
            #[cfg(not(feature = "spade"))]
            spade_top: None,
            waves: args.wave_file.map(|s| string_to_wavesource(&s)),
            startup_commands,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn main() -> Result<()> {
        logs::start_logging()?;

        // https://tokio.rs/tokio/topics/bridging
        // We want to run the gui in the main thread, but some long running tasks like
        // loading VCDs should be done asynchronously. We can't just use std::thread to
        // do that due to wasm support, so we'll start a tokio runtime
        let runtime = tokio::runtime::Builder::new_current_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();

        // parse arguments
        let args = Args::parse();
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(Commands::Server { port, token, file }) = args.command {
            let default_port = 8911; // FIXME: make this more configurable
            let res = runtime.block_on(surver::server_main(
                port.unwrap_or(default_port),
                token,
                file,
                None,
            ));
            return res;
        }

        let _enter = runtime.enter();

        std::thread::spawn(move || {
            runtime.block_on(async {
                loop {
                    tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
                }
            });
        });

        let state_file = args.state_file.clone();
        let startup_params = startup_params_from_args(args);
        let waves = startup_params.waves.clone();

        let state = match &state_file {
            Some(file) => std::fs::read_to_string(file)
                .with_context(|| format!("Failed to read state from {file}"))
                .and_then(|content| {
                    ron::from_str::<State>(&content)
                        .with_context(|| format!("Failed to decode state from {file}"))
                })
                .map(|mut s| {
                    s.state_file = Some(file.into());
                    s
                })
                .or_else(|e| {
                    error!("Failed to read state file. Opening fresh session\n{e:#?}");
                    State::new()
                })?,
            None => State::new()?,
        }
        .with_params(startup_params);

        // install a file watcher that emits a `SuggestReloadWaveform` message
        // whenever the user-provided file changes.
        let _watcher = match waves {
            Some(WaveSource::File(path)) => {
                let sender = state.sys.channels.msg_sender.clone();
                FileWatcher::new(&path, move || {
                    match sender.send(Message::SuggestReloadWaveform) {
                        Ok(_) => {}
                        Err(err) => {
                            error!("Message ReloadWaveform did not send:\n{err}")
                        }
                    }
                })
                .inspect_err(|err| error!("Cannot set up the file watcher:\n{err}"))
                .ok()
            }
            _ => None,
        };

        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title("Surfer")
                .with_inner_size(Vec2::new(
                    state.config.layout.window_width as f32,
                    state.config.layout.window_height as f32,
                )),
            ..Default::default()
        };

        eframe::run_native("Surfer", options, Box::new(|cc| Ok(run_egui(cc, state)?))).unwrap();

        Ok(())
    }
}

#[cfg(target_arch = "wasm32")]
mod main_impl {
    use eframe::wasm_bindgen::JsCast;
    use eframe::web_sys;
    use libsurfer::wasm_api::WebHandle;

    // Calling main is not the intended way to start surfer, instead, it should be
    // started by `wasm_util::WebHandle`
    pub(crate) fn main() -> color_eyre::Result<()> {
        let document = web_sys::window()
            .expect("No window")
            .document()
            .expect("No document");
        let canvas = document
            .get_element_by_id("the_canvas_id")
            .expect("Failed to find the_canvas_id")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("the_canvas_id was not a HtmlCanvasElement");

        wasm_bindgen_futures::spawn_local(async {
            let wh = WebHandle::new();
            wh.start(canvas).await.expect("Failed to start surfer");
        });

        Ok(())
    }
}

fn main() -> color_eyre::Result<()> {
    main_impl::main()
}
