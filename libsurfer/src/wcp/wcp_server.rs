use bytes::{Buf, BytesMut};
use color_eyre::eyre::Result;
use eframe::egui::Context;
use serde::Serialize;
use serde_json::Error as serde_Error;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;
use tokio::net::tcp::{ReadHalf, WriteHalf};
use tokio::net::{TcpListener, TcpStream};

use log::{error, info, warn};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc::Receiver;
use tokio::sync::mpsc::Sender;

use super::{proto::WcpCSMessage, proto::WcpCommand, proto::WcpSCMessage};

struct WcpCSReader<'a> {
    reader: BufReader<ReadHalf<'a>>,
    buffer: BytesMut,
}

impl<'a> WcpCSReader<'a> {
    pub fn new(stream: ReadHalf<'a>) -> Self {
        WcpCSReader {
            reader: BufReader::new(stream),
            buffer: BytesMut::with_capacity(8 * 1024),
        }
    }

    pub async fn read_frame(&mut self) -> Result<Option<WcpCSMessage>, serde_Error> {
        loop {
            if let Some(frame) = self.try_decode_frame()? {
                return Ok(Some(frame));
            }

            match self.reader.read_buf(&mut self.buffer).await {
                Ok(0) => {
                    return Err(serde_Error::io(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "EOF",
                    )))
                }
                Ok(_) => (),
                Err(e) => return Err(serde_Error::io(e)),
            }
        }
    }

    fn try_decode_frame(&mut self) -> Result<Option<WcpCSMessage>, serde_Error> {
        match self.buffer.iter().position(|&x| x == 0) {
            Some(position) => {
                let frame_data = self.buffer.split_to(position);
                self.buffer.advance(1);
                let msg: Result<WcpCSMessage, _> = serde_json::from_slice(&frame_data);
                match msg {
                    Ok(msg) => Ok(Some(msg)),
                    Err(e) => Err(e),
                }
            }
            None => Ok(None),
        }
    }
}

pub struct WcpServer {
    listener: Option<TcpListener>,
    stream: Option<TcpStream>,
    sender: Sender<WcpCSMessage>,
    receiver: Receiver<WcpSCMessage>,
    stop_signal: Arc<AtomicBool>,
    running_signal: Arc<AtomicBool>,
    ctx: Option<Arc<Context>>,
}

impl WcpServer {
    pub async fn new(
        address: String,
        initiate: bool,
        c2s_sender: Sender<WcpCSMessage>,
        s2c_receiver: Receiver<WcpSCMessage>,
        stop_signal: Arc<AtomicBool>,
        running_signal: Arc<AtomicBool>,
        ctx: Option<Arc<Context>>,
    ) -> Result<Self> {
        let listener;
        let stream;
        if initiate {
            let the_stream = TcpStream::connect(address).await?;
            stream = Some(the_stream);
            listener = None;
        } else {
            let the_listener = TcpListener::bind(address).await?;
            info!(
                "WCP Server listening on port {}",
                the_listener.local_addr().unwrap()
            );
            listener = Some(the_listener);
            stream = None;
        }
        Ok(WcpServer {
            listener,
            stream,
            sender: c2s_sender,
            receiver: s2c_receiver,
            stop_signal,
            running_signal,
            ctx,
        })
    }

    pub async fn run(&mut self) {
        if self.listener.is_some() {
            self.listen().await;
        } else if self.stream.is_some() {
            self.initiate().await;
        } else {
            error!("Internal error: calling `run` with both listener and stream unset");
        }
        self.stop_signal.store(true, Ordering::Relaxed);
    }

    async fn listen(&mut self) {
        let listener = self.listener.take().unwrap();
        loop {
            let stop_signal_clone = self.stop_signal.clone();
            let stop_signal_waiter = async {
                while !stop_signal_clone.load(Ordering::Relaxed) {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            };

            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((stream, _addr)) => self.handle_connection(stream).await,
                        Err(ref e)
                            if [std::io::ErrorKind::WouldBlock, std::io::ErrorKind::TimedOut]
                                .contains(&e.kind()) =>
                        {
                            continue
                        }
                        Err(e) => warn!("WCP Connection failed: {e}"),
                    }
                }

                _ = stop_signal_waiter => {
                    break;
                }
            }
        }
        info!("WCP shutting down");
        self.running_signal.store(false, Ordering::Relaxed);
    }

    async fn initiate(&mut self) {
        let stream = self.stream.take().unwrap();
        match self.handle_client(stream).await {
            Err(error) => warn!("WCP Client disconnected with error: {error:#?}"),
            Ok(()) => info!("WCP client disconnected"),
        }
    }

    async fn handle_connection(&mut self, stream: TcpStream) {
        info!("WCP New connection: {}", stream.peer_addr().unwrap());

        //handle connection from client
        match self.handle_client(stream).await {
            Err(error) => warn!("WCP Client disconnected with error: {error:#?}"),
            Ok(()) => info!("WCP client disconnected"),
        }
    }

    async fn send_message<M: Serialize>(&mut self, stream: &mut WriteHalf<'_>, message: &M) {
        match serde_json::to_string(message) {
            Ok(message) => {
                if let Err(error) = stream.write_all(message.as_bytes()).await {
                    warn!("WCP Sending of message failed: {error:#?}")
                }
            }
            Err(error) => warn!("Serializing message failed: {error:#?}"),
        }
        if let Err(e) = stream.write_all(b"\0").await {
            warn!("Failed to send WCP message: {e:#?}");
        }
        if let Err(e) = stream.flush().await {
            warn!("Failed to send WCP message: {e:#?}");
        }
    }

    async fn handle_client(&mut self, mut stream: TcpStream) -> Result<(), serde_Error> {
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
        .collect();

        let (reader, mut writer) = stream.split();
        let mut reader = WcpCSReader::new(reader);

        //send greeting
        let greeting = WcpSCMessage::create_greeting(0, commands);
        self.send_message(&mut writer, &greeting).await;

        loop {
            let stop_signal_clone = self.stop_signal.clone();
            let stop_signal_waiter = async {
                while !stop_signal_clone.load(Ordering::Relaxed) {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            };

            tokio::select! {
                msg = reader.read_frame() => {
                    let msg = match msg? {
                        Some(msg) => msg,
                        None => continue,
                    };

                    if let WcpCSMessage::command(WcpCommand::shutdowmn) = msg {
                        return Ok(());
                    }

                    if let Err(e) = self.sender.send(msg).await {
                        error!("Failed to send wcp message into main thread {e}")
                    };

                    // request repaint of the Surfer UI
                    if let Some(ctx) = &self.ctx {
                        ctx.request_repaint();
                    }
                }

                s2c = self.receiver.recv() => {
                    if let Some(s2c) = s2c {
                        self.send_message(&mut writer, &s2c).await;
                    }
                }

                _ = stop_signal_waiter => {
                    return Err(serde_Error::io(std::io::Error::new(
                        std::io::ErrorKind::ConnectionAborted,
                        "Server terminated",
                    )));
                }
            }
        }
    }
}
