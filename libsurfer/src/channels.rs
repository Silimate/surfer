use tokio::sync::{
    mpsc::{
        self,
        error::{SendError, TryRecvError},
    },
    RwLock,
};

use crate::{EGUI_CONTEXT, OUTSTANDING_TRANSACTIONS};

pub struct IngressReceiver<T> {
    sc_messages: mpsc::Receiver<T>,
}

impl<T> IngressReceiver<T> {
    pub fn new(sc_messages: mpsc::Receiver<T>) -> Self {
        Self { sc_messages }
    }

    pub fn try_recv(&mut self) -> Result<T, TryRecvError> {
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

pub struct IngressSender<T> {
    sc_messages: mpsc::Sender<T>,
}

impl<T> IngressSender<T> {
    pub fn new(sc_messages: mpsc::Sender<T>) -> Self {
        Self { sc_messages }
    }

    pub async fn send(&self, message: T) -> Result<(), SendError<T>> {
        let result = self.sc_messages.send(message).await;
        OUTSTANDING_TRANSACTIONS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if let Some(ctx) = EGUI_CONTEXT.read().unwrap().as_ref() {
            ctx.request_repaint();
        }
        result
    }
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) struct IngressHandler<T> {
    pub tx: IngressSender<T>,
    pub rx: RwLock<Option<IngressReceiver<T>>>,
}
impl<T> IngressHandler<T> {
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel(100);
        Self {
            tx: IngressSender::new(tx),
            rx: RwLock::new(Some(IngressReceiver::new(rx))),
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) struct GlobalChannelTx<T> {
    pub tx: mpsc::Sender<T>,
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub rx: RwLock<mpsc::Receiver<T>>,
}
#[cfg(target_arch = "wasm32")]
impl<T> GlobalChannelTx<T> {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel(100);
        Self {
            tx,
            rx: RwLock::new(rx),
        }
    }
}
