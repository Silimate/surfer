use tokio::sync::{
    mpsc::{
        self,
        error::{SendError, TryRecvError},
    },
    RwLock,
};

use crate::{EGUI_CONTEXT, OUTSTANDING_TRANSACTIONS};

pub struct SCReceiver {
    sc_messages: mpsc::Receiver<String>,
}

impl SCReceiver {
    pub fn new(sc_messages: mpsc::Receiver<String>) -> Self {
        Self { sc_messages }
    }

    pub async fn recv(&mut self) -> Option<String> {
        let result = self.sc_messages.recv().await;
        OUTSTANDING_TRANSACTIONS.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        result
    }

    pub fn try_recv(&mut self) -> Result<String, TryRecvError> {
        let result = self.sc_messages.try_recv();
        match result {
            Ok(result) => {
                OUTSTANDING_TRANSACTIONS.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                Ok(result)
            }
            Err(TryRecvError::Empty) => Err(TryRecvError::Empty),
            Err(_) => {
                OUTSTANDING_TRANSACTIONS.store(0, std::sync::atomic::Ordering::SeqCst);
                Err(TryRecvError::Disconnected)
            }
        }
    }
}

pub struct SCSender {
    sc_messages: mpsc::Sender<String>,
}

impl SCSender {
    pub fn new(sc_messages: mpsc::Sender<String>) -> Self {
        Self { sc_messages }
    }

    pub async fn send(&self, message: String) -> Result<(), SendError<String>> {
        let result = self.sc_messages.send(message).await;
        OUTSTANDING_TRANSACTIONS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if let Some(ctx) = EGUI_CONTEXT.read().unwrap().as_ref() {
            ctx.request_repaint();
        }
        result
    }
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) struct SCHandler {
    pub tx: SCSender,
    pub rx: RwLock<Option<mpsc::Receiver<String>>>,
}
impl SCHandler {
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel(100);
        Self {
            tx: SCSender::new(tx),
            rx: RwLock::new(Some(rx)),
        }
    }
}

pub(crate) struct GlobalChannelTx<T> {
    pub tx: mpsc::Sender<T>,
    pub rx: RwLock<mpsc::Receiver<T>>,
}
impl<T> GlobalChannelTx<T> {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel(100);
        Self {
            tx,
            rx: RwLock::new(rx),
        }
    }
}

pub(crate) struct GlobalChannelRx<T> {
    pub tx: mpsc::Sender<T>,
    pub rx: RwLock<Option<mpsc::Receiver<T>>>,
}
impl<T> GlobalChannelRx<T> {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel(100);
        Self {
            tx,
            rx: RwLock::new(Some(rx)),
        }
    }
}
