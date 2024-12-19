use std::{collections::VecDeque, io::Write};

use color_eyre::{eyre::Context, Result};
use log::{error, info, trace};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::mpsc,
};

use crate::channels::SCSender;

pub struct CxxrtlWorker<W, R> {
    write: W,
    read: R,
    read_buf: VecDeque<u8>,

    sc_channel: SCSender,
    cs_channel: mpsc::Receiver<String>,
}

impl<W, R> CxxrtlWorker<W, R>
where
    W: AsyncWriteExt + Unpin,
    R: AsyncReadExt + Unpin,
{
    pub(crate) fn new(
        write: W,
        read: R,
        sc_channel: SCSender,
        cs_channel: mpsc::Receiver<String>,
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

    async fn process_stream(&mut self, count: usize, buf: &mut [u8; 1024]) -> Result<Vec<String>> {
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
            // The null byte should not be part of this or the next message message
            self.read_buf.pop_front();

            new_messages
                .push(String::from_utf8(message).context("Got non-utf8 characters from cxxrtl")?)
        }

        Ok(new_messages)
    }

    async fn send_message(&mut self, message: String) -> Result<()> {
        self.write.write_all(message.as_bytes()).await?;
        self.write.write_all(b"\0").await?;
        self.write.flush().await?;

        Ok(())
    }
}
