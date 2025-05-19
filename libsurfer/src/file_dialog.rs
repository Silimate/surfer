#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

#[cfg(not(target_arch = "wasm32"))]
use camino::Utf8PathBuf;
use rfd::AsyncFileDialog;
use serde::Deserialize;

use crate::async_util::perform_async_work;
use crate::message::Message;
use crate::wave_source::{LoadOptions, STATE_FILE_EXTENSION};
use crate::SystemState;

#[derive(Debug, Deserialize)]
pub enum OpenMode {
    Open,
    Switch,
}

impl SystemState {
    #[cfg(not(target_arch = "wasm32"))]
    fn file_dialog<F>(&mut self, filter: (String, Vec<String>), message: F)
    where
        F: FnOnce(PathBuf) -> Message + Send + 'static,
    {
        let sender = self.channels.msg_sender.clone();

        perform_async_work(async move {
            if let Some(file) = create_file_dialog(filter, "Open waveform file")
                .pick_file()
                .await
            {
                sender.send(message(file.path().to_path_buf())).unwrap();
            }
        });
    }

    #[cfg(target_arch = "wasm32")]
    fn file_dialog<F>(&mut self, filter: (String, Vec<String>), message: F)
    where
        F: FnOnce(Vec<u8>) -> Message + Send + 'static,
    {
        let sender = self.channels.msg_sender.clone();

        perform_async_work(async move {
            if let Some(file) = create_file_dialog(filter, "Open waveform file")
                .pick_file()
                .await
            {
                sender.send(message(file.read().await)).unwrap();
            }
        });
    }

    pub fn open_file_dialog(&mut self, mode: OpenMode) {
        let keep_unavailable = self.user.config.behavior.keep_during_reload;
        let keep_variables = match mode {
            OpenMode::Open => false,
            OpenMode::Switch => true,
        };

        #[cfg(not(target_arch = "wasm32"))]
        let message = move |file: PathBuf| {
            Message::LoadFile(
                Utf8PathBuf::from_path_buf(file).unwrap(),
                LoadOptions {
                    keep_variables,
                    keep_unavailable,
                },
            )
        };

        #[cfg(target_arch = "wasm32")]
        let message = move |file: Vec<u8>| {
            Message::LoadFromData(
                file,
                LoadOptions {
                    keep_variables,
                    keep_unavailable,
                },
            )
        };

        self.file_dialog(
            (
                "Waveform/Transaction-files (*.vcd, *.fst, *.ghw, *.ftr)".to_string(),
                vec![
                    "vcd".to_string(),
                    "fst".to_string(),
                    "ghw".to_string(),
                    "ftr".to_string(),
                ],
            ),
            message,
        );
    }

    #[cfg(feature = "python")]
    pub fn open_python_file_dialog(&mut self) {
        self.file_dialog(
            ("Python files (*.py)".to_string(), vec!["py".to_string()]),
            |file| Message::LoadPythonTranslator(Utf8PathBuf::from_path_buf(file).unwrap()),
        );
    }
}

pub async fn save_state_dialog() -> Option<rfd::FileHandle> {
    create_file_dialog(
        (
            format!("Surfer state files (*.{})", STATE_FILE_EXTENSION),
            ([STATE_FILE_EXTENSION.to_string()]).to_vec(),
        ),
        "Save state",
    )
    .save_file()
    .await
}

pub async fn load_state_dialog() -> Option<rfd::FileHandle> {
    create_file_dialog(
        (
            format!("Surfer state files (*.{})", STATE_FILE_EXTENSION),
            ([STATE_FILE_EXTENSION.to_string()]).to_vec(),
        ),
        "Load state",
    )
    .pick_file()
    .await
}

fn create_file_dialog(filter: (String, Vec<String>), title: &'static str) -> AsyncFileDialog {
    AsyncFileDialog::new()
        .set_title(title)
        .add_filter(filter.0, &filter.1)
        .add_filter("All files", &["*"])
}
