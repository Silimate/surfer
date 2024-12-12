use color_eyre::eyre::Result;
use eframe::egui::Context;
use itertools::Itertools;
use serde::Deserialize;
use serde_json::Error as serde_Error;
use std::{
    io::prelude::*,
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use log::{error, info, warn};
use tokio::sync::mpsc::Receiver;
use tokio::sync::mpsc::Sender;

use super::{cs_message::WcpCommand, WcpCSMessage, WcpSCMessage};

pub struct WcpServer {
    listener: TcpListener,
    sender: Sender<WcpCSMessage>,
    receiver: Receiver<WcpSCMessage>,
    stop_signal: Arc<AtomicBool>,
    running_signal: Arc<AtomicBool>,
    ctx: Option<Arc<Context>>,
}

impl WcpServer {
    pub fn new(
        address: String,
        s2c_sender: Sender<WcpCSMessage>,
        c2s_receiver: Receiver<WcpSCMessage>,
        stop_signal: Arc<AtomicBool>,
        running_signal: Arc<AtomicBool>,
        ctx: Option<Arc<Context>>,
    ) -> Result<Self> {
        let listener = TcpListener::bind(address)?;
        info!(
            "WCP Server listening on port {}",
            listener.local_addr().unwrap()
        );
        Ok(WcpServer {
            listener,
            sender: s2c_sender,
            receiver: c2s_receiver,
            stop_signal,
            running_signal,
            ctx,
        })
    }

    pub fn run(&mut self) {
        let commands = vec![
            "add_variables",
            "set_viewport_to",
            "cursor_set",
            "reload",
            "add_scopes",
            "get_item_list",
            "set_item_color",
            "get_item_info",
            "clear_item",
            "focus_item",
            "clear",
            "load",
            "zoom_to_fit",
        ]
        .into_iter()
        .map(str::to_string)
        .collect_vec();

        let greeting = WcpSCMessage::create_greeting(0, commands);

        info!("WCP Listening on Port {:#?}", self.listener);
        let listener = self.listener.try_clone().unwrap();

        for stream in listener.incoming() {
            // check if the server should stop
            if self.stop_signal.load(Ordering::Relaxed) {
                break;
            }

            match stream {
                Ok(mut stream) => {
                    info!("WCP New connection: {}", stream.peer_addr().unwrap());
                    if let Err(error) = stream.set_read_timeout(Some(Duration::from_secs(2))) {
                        error!("Failed to set timeout: {error:#?}")
                    }

                    //send greeting
                    if let Err(error) = serde_json::to_writer(&stream, &greeting) {
                        warn!("WCP Sending of greeting failed: {error:#?}")
                    }
                    let _ = stream.write(b"\0");
                    stream.flush().unwrap();

                    //handle connection from client
                    match self.handle_client(stream) {
                        Err(error) => warn!("WCP Client disconnected with error: {error:#?}"),
                        Ok(()) => info!("WCP client disconnected"),
                    }
                }
                Err(ref e)
                    if [std::io::ErrorKind::WouldBlock, std::io::ErrorKind::TimedOut]
                        .contains(&e.kind()) =>
                {
                    continue
                }
                Err(e) => warn!("WCP Connection failed: {e}"),
            }
        }
        info!("WCP shutting down");
        self.running_signal.store(false, Ordering::Relaxed);
    }

    fn handle_client(&mut self, mut stream: TcpStream) -> Result<(), serde_Error> {
        loop {
            // check if the server should stop
            if self.stop_signal.load(Ordering::Relaxed) {
                return Err(serde_Error::io(std::io::Error::new(
                    std::io::ErrorKind::ConnectionAborted,
                    "Server terminated",
                )));
            }
            //get message from client
            let msg: WcpCSMessage = match self.get_json_message(&stream) {
                Ok(msg) => msg,
                Err(e) => {
                    match e.classify() {
                        //error when the client disconnects
                        serde_json::error::Category::Eof => return Err(e),
                        _ => match e.io_error_kind() {
                            Some(std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                                continue;
                            }
                            //if different error get next message and send error
                            _ => {
                                warn!("WCP S>C error: {e:?}\n");

                                let _ = serde_json::to_writer(
                                    &stream,
                                    &WcpSCMessage::create_error(
                                        "parsing error".to_string(),
                                        vec![],
                                        "parsing error".to_string(),
                                    ),
                                );
                                continue;
                            }
                        },
                    }
                }
            };

            if let WcpCSMessage::Command(WcpCommand::Shutdown) = msg {
                return Ok(());
            }

            if let Err(e) = self.sender.blocking_send(msg) {
                error!("Failed to send wcp message into main thread {e}")
            };

            // request repaint of the Surfer UI
            if let Some(ctx) = &self.ctx {
                ctx.request_repaint();
            }

            // FIXME: Handle timeout
            let resp = match self.receiver.blocking_recv() {
                Some(resp) => resp,
                None => {
                    warn!("WCP No response from handler");
                    WcpSCMessage::create_error(
                        "No response".to_string(),
                        vec![],
                        "No response from handler".to_string(),
                    )
                }
            };
            //send response back to client
            serde_json::to_writer(&stream, &resp)?;
            let _ = stream.write(b"\0");
            let _ = stream.flush();
        }
    }

    fn get_json_message(&mut self, mut stream: &TcpStream) -> Result<WcpCSMessage, serde_Error> {
        let mut de = serde_json::Deserializer::from_reader(&mut stream);
        let cmd = WcpCSMessage::deserialize(&mut de);
        let mut buffer = [0; 1];
        if let Ok(0) = stream.read(&mut buffer) {
            return Ok(WcpCSMessage::Command(WcpCommand::Shutdown));
        }
        if buffer[0] != 0 {
            warn!(
                "WCP read wrong terminating byte. Expected '0' got '{}' instead",
                buffer[0]
            );
        }
        cmd
    }
}
