use std::{collections::VecDeque, io::Write};

use color_eyre::{eyre::Context, Result};
use log::{error, info, trace};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::mpsc,
};

use crate::cxxrtl::cs_message::CSMessage;

use super::sc_message::SCMessage;

pub struct CxxrtlWorker<W, R> {
    write: W,
    read: R,
    read_buf: VecDeque<u8>,

    sc_channel: mpsc::Sender<SCMessage>,
    cs_channel: mpsc::Receiver<CSMessage>,
}

impl<W, R> CxxrtlWorker<W, R>
where
    W: AsyncWriteExt + Unpin,
    R: AsyncReadExt + Unpin,
{
    pub(crate) fn new(
        write: W,
        read: R,
        sc_channel: mpsc::Sender<SCMessage>,
        cs_channel: mpsc::Receiver<CSMessage>,
    ) -> Self {
        Self {
            write,
            read,
            read_buf: VecDeque::new(),
            sc_channel,
            cs_channel,
        }
    }

    pub(crate) async fn start(mut self) {
        info!("cxxrtl worker is up-and-running");
        let mut buf = [0; 1024];
        loop {
            tokio::select! {
                rx = self.cs_channel.recv() => {
                    if let Some(msg) = rx {
                        if let Err(e) =  self.send_message(msg).await {
                                error!("Failed to send message {e:#?}");
                            } else {
                            }
                    }
                }
                count = self.read.read(&mut buf) => {
                    match count {
                        Ok(count) => {
                            trace!("CXXRTL Read {count} from reader");
                            match self.process_stream(count, &mut buf).await {
                                Ok(msgs) => {
                                    for msg in msgs {
                                        self.sc_channel.send(msg).await.unwrap();
                                    }
                                }
                                Err(e) => {
                                    error!("Failed to process cxxrtl message ({e:#?})");
                                }
                            }
                        },
                        Err(e) => {
                            error!("Failed to read bytes from cxxrtl {e:#?}. Shutting down client");
                            break;
                        }
                    }
                }
            }
        }
    }

    async fn process_stream(
        &mut self,
        count: usize,
        buf: &mut [u8; 1024],
    ) -> Result<Vec<SCMessage>> {
        if count != 0 {
            self.read_buf
                .write_all(&buf[0..count])
                .context("Failed to read from cxxrtl tcp socket")?;
        }

        let mut new_messages = vec![];

        while let Some(idx) = self
            .read_buf
            .iter()
            .enumerate()
            .find(|(_i, c)| **c == b'\0')
        {
            let message = self.read_buf.drain(0..idx.0).collect::<Vec<_>>();
            // The newline should not be part of this or the next message message
            self.read_buf.pop_front();

            let decoded = serde_json::from_slice(&message).with_context(|| {
                format!(
                    "Failed to decode message from cxxrtl. Message: '{}'",
                    String::from_utf8_lossy(&message)
                )
            })?;

            trace!("cxxrtl: S>C: {decoded:?}");

            new_messages.push(decoded)
        }

        Ok(new_messages)
    }

    async fn send_message(&mut self, message: CSMessage) -> Result<()> {
        let encoded = serde_json::to_string(&message)
            .with_context(|| "Failed to encode message".to_string())?;
        self.write.write_all(encoded.as_bytes()).await?;
        self.write.write_all(&[b'\0']).await?;
        self.write.flush().await?;

        trace!("cxxrtl: C>S: {encoded}");

        Ok(())
    }
}
