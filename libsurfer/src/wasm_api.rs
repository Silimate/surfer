// The functions here are only used
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use std::collections::VecDeque;
use std::sync::Arc;

use futures::executor::block_on;
use lazy_static::lazy_static;
use log::info;
use log::{error, warn};
use num::BigInt;
use tokio::sync::Mutex;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

use crate::channels::{GlobalChannelTx, SCHandler};
use crate::displayed_item::DisplayedItemRef;
use crate::graphics::Anchor;
use crate::graphics::Direction;
use crate::graphics::GrPoint;
use crate::graphics::Graphic;
use crate::graphics::GraphicId;
use crate::graphics::GraphicsY;
use crate::logs;
use crate::setup_custom_font;
use crate::wasm_panic;
use crate::wasm_util;
use crate::wave_container::VariableRefExt;
use crate::wave_source::CxxrtlKind;
use crate::DisplayedItem;
use crate::Message;
use crate::StartupParams;
use crate::State;
use crate::EGUI_CONTEXT;
use crate::WCP_CS_HANDLER;
use crate::WCP_SC_HANDLER;

lazy_static! {
    pub(crate) static ref MESSAGE_QUEUE: Mutex<Vec<Message>> = Mutex::new(vec![]);
    static ref QUERY_QUEUE: tokio::sync::Mutex<VecDeque<Callback>> =
        tokio::sync::Mutex::new(VecDeque::new());
    pub(crate) static ref CXXRTL_SC_HANDLER: SCHandler = SCHandler::new();
    pub(crate) static ref CXXRTL_CS_HANDLER: GlobalChannelTx<String> = GlobalChannelTx::new();
}

struct Callback {
    function: Box<dyn FnOnce(&State) + Send + Sync>,
    executed: tokio::sync::oneshot::Sender<()>,
}

pub fn try_repaint() {
    if let Some(ctx) = EGUI_CONTEXT.read().unwrap().as_ref() {
        ctx.request_repaint();
    } else {
        warn!("Attempted to request surfer repaint but surfer has not given us EGUI_CONTEXT yet")
    }
}

/// Your handle to the web app from JavaScript.
#[derive(Clone)]
#[wasm_bindgen]
pub struct WebHandle {
    runner: eframe::WebRunner,
}

#[wasm_bindgen]
impl WebHandle {
    /// Installs a panic hook, then returns.
    #[allow(clippy::new_without_default)]
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        let web_log_config = fern::Dispatch::new()
            .level(log::LevelFilter::Info)
            .format(move |out, message, record| {
                out.finish(format_args!("[{}] {}", record.level(), message))
            })
            .chain(Box::new(eframe::WebLogger::new(log::LevelFilter::Debug)) as Box<dyn log::Log>);

        logs::setup_logging(web_log_config).unwrap();

        wasm_panic::set_once();

        Self {
            runner: eframe::WebRunner::new(),
        }
    }

    /// Call this once from JavaScript to start your app.
    #[wasm_bindgen]
    pub async fn start(
        &self,
        canvas: web_sys::HtmlCanvasElement,
    ) -> Result<(), wasm_bindgen::JsValue> {
        let web_options = eframe::WebOptions::default();

        let url = wasm_util::vcd_from_url();

        // NOTE: Safe unwrap, we're loading a system config which cannot be changed by the
        // user
        let mut state = State::new()
            .unwrap()
            .with_params(StartupParams::from_url(url));

        self.runner
            .start(
                canvas,
                web_options,
                Box::new(|cc| {
                    let ctx_arc = Arc::new(cc.egui_ctx.clone());
                    *EGUI_CONTEXT.write().unwrap() = Some(ctx_arc.clone());
                    state.sys.context = Some(ctx_arc.clone());
                    setup_custom_font(&cc.egui_ctx);
                    cc.egui_ctx
                        .set_visuals_of(egui::Theme::Dark, state.get_visuals());
                    cc.egui_ctx
                        .set_visuals_of(egui::Theme::Light, state.get_visuals());
                    Ok(Box::new(state))
                }),
            )
            .await
    }
}

// NOTE: Remember to add WASM_bindgen'd functions to the exports in Trunk.toml

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn inject_message(message: &str) {
    let deser = serde_json::from_str(message);

    match deser {
        Ok(message) => {
            block_on(MESSAGE_QUEUE.lock()).push(message);

            try_repaint()
        }
        Err(e) => {
            error!("When injecting message {message}:");
            error!(" Injection failed{e:#?}")
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub async fn id_of_name(name: String) -> Option<usize> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let result = Arc::new(tokio::sync::Mutex::new(None));
    let result_clone = result.clone();
    QUERY_QUEUE.lock().await.push_back(Callback {
        function: Box::new(move |state| {
            if let Some(waves) = &state.waves {
                *block_on(result_clone.lock()) = waves
                    .displayed_items
                    .iter()
                    .find(|(_id, item)| {
                        let item_name = match item {
                            DisplayedItem::Variable(var) => var.variable_ref.full_path_string(),
                            _ => item.name().to_string(),
                        };
                        item_name == name
                    })
                    .map(|(id, _)| *id)
            }
        }),
        executed: tx,
    });
    try_repaint();
    rx.await.unwrap();
    let ret = block_on(result.lock());
    ret.map(|x| x.0)
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub async fn draw_text_arrow(
    id: usize,
    from_item: String,
    from_time: u64,
    to_item: String,
    to_time: u64,
    text: String,
) {
    let from_id = id_of_name(from_item).await.map(DisplayedItemRef);
    let to_id = id_of_name(to_item).await.map(DisplayedItemRef);

    if let (Some(from_id), Some(to_id)) = (from_id, to_id) {
        block_on(MESSAGE_QUEUE.lock()).push(Message::AddGraphic(
            GraphicId(id),
            Graphic::TextArrow {
                from: (
                    GrPoint {
                        x: BigInt::from(from_time),
                        y: GraphicsY {
                            item: from_id,
                            anchor: Anchor::Center,
                        },
                    },
                    Direction::East,
                ),
                to: (
                    GrPoint {
                        x: BigInt::from(to_time),
                        y: GraphicsY {
                            item: to_id,
                            anchor: Anchor::Center,
                        },
                    },
                    Direction::West,
                ),
                text,
            },
        ));

        try_repaint()
    }
}

async fn perform_query<T>(query: Box<dyn FnOnce(&State) -> Option<T> + Send + Sync>) -> Option<T>
where
    T: Clone + Send + Sync + 'static,
{
    let (tx, rx) = tokio::sync::oneshot::channel();
    let result = Arc::new(tokio::sync::Mutex::new(None));
    let result_clone = result.clone();
    QUERY_QUEUE.lock().await.push_back(Callback {
        function: Box::new(move |state| *block_on(result_clone.lock()) = query(state)),
        executed: tx,
    });
    try_repaint();
    rx.await.unwrap();
    let ret = block_on(result.lock());
    ret.clone()
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub async fn index_of_name(name: String) -> Option<usize> {
    perform_query(Box::new(move |state| {
        if let Some(waves) = &state.waves {
            let mut result = None;
            for (idx, itemref) in waves.displayed_items_order.iter().enumerate() {
                if let Some(item) = waves.displayed_items.get(itemref) {
                    let item_name = match item {
                        DisplayedItem::Variable(var) => var.variable_ref.full_path_string(),
                        _ => item.name().to_string(),
                    };
                    if item_name == name {
                        result = Some(idx);
                    }
                }
            }
            result
        } else {
            None
        }
    }))
    .await
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub async fn waves_loaded() -> bool {
    perform_query(Box::new(move |state| Some(state.waves.is_some())))
        .await
        .unwrap_or(false)
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub async fn spade_loaded() -> bool {
    perform_query(Box::new(move |state| {
        Some(
            state
                .sys
                .translators
                .all_translator_names()
                .iter()
                .any(|n| *n == "spade"),
        )
    }))
    .await
    .unwrap_or(false)
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub async fn start_cxxrtl() {
    MESSAGE_QUEUE
        .lock()
        .await
        .push(Message::SetupCxxrtl(CxxrtlKind::Mailbox));
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub async fn cxxrtl_cs_message() -> Option<String> {
    CXXRTL_CS_HANDLER.rx.write().await.recv().await
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub async fn on_cxxrtl_sc_message(message: String) {
    CXXRTL_SC_HANDLER.tx.send(message).await.unwrap();
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub async fn start_wcp() {
    MESSAGE_QUEUE.lock().await.push(Message::SetupChannelWCP);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub async fn next_wcp_sc_message() -> Result<Option<String>, JsError> {
    WCP_SC_HANDLER
        .rx
        .write()
        .await
        .recv()
        .await
        .map(|msg| serde_json::to_string(&msg))
        .transpose()
        .map_err(|e| JsError::new(&format!("{e}")))
}

// TODO: Unify the names with cxxrtl here
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub async fn handle_wcp_cs_message(message: String) -> Result<(), JsError> {
    info!("[WCP] Received wcp cs message from WASM");
    let encoded = serde_json::from_str(&message).map_err(|e| JsError::new(&format!("{e}")))?;
    WCP_CS_HANDLER.tx.send(encoded).await?;
    Ok(())
}

impl State {
    pub(crate) fn handle_wasm_external_messages(&mut self) {
        while let Some(msg) = block_on(MESSAGE_QUEUE.lock()).pop() {
            self.update(msg);
        }

        while let Some(cb) = block_on(QUERY_QUEUE.lock()).pop_front() {
            (cb.function)(self);
            let _ = cb.executed.send(());
        }
    }
}
