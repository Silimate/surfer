use futures::executor::block_on;
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};
use tokio::sync::mpsc;

use color_eyre::Result;
use log::{error, info};
use num::{
    bigint::{ToBigInt, ToBigUint},
    BigUint,
};
use serde::Deserialize;
use surfer_translation_types::VariableEncoding;

use crate::{
    cxxrtl::{
        command::CxxrtlCommand,
        cs_message::CSMessage,
        query_container::QueryContainer,
        sc_message::{
            CommandResponse, CxxrtlSimulationStatus, Event, SCMessage, SimulationStatusType,
        },
        timestamp::CxxrtlTimestamp,
    },
    message::Message,
    wave_container::{
        QueryResult, ScopeId, ScopeRef, SimulationStatus, VarId, VariableMeta, VariableRef,
        VariableRefExt,
    },
};
use crate::{wave_container::ScopeRefExt, OUTSTANDING_TRANSACTIONS};

const DEFAULT_REFERENCE: &str = "ALL_VARIABLES";

type Callback = Box<dyn FnOnce(CommandResponse, &mut CxxrtlData) + Sync + Send>;

#[derive(Deserialize, Debug, Clone)]
pub(crate) struct CxxrtlScope {}

#[derive(Deserialize, Debug, Clone)]
pub struct CxxrtlItem {
    pub width: u32,
}

/// A piece of data which we cache from Cxxrtl
pub enum CachedData<T> {
    /// The data cache is invalidated, the previously held data if it is still useful is
    /// kept
    Uncached { prev: Option<Arc<T>> },
    /// The data cache is invalidated, and a request has been made for new data. However,
    /// the new data has not been received yet. If the previous data is not useless, it
    /// can be stored here
    Waiting { prev: Option<Arc<T>> },
    /// The cache is up-to-date
    Filled(Arc<T>),
}

impl<T> CachedData<T> {
    fn empty() -> Self {
        Self::Uncached { prev: None }
    }

    fn make_uncached(&self) -> Self {
        // Since the internals here are all Arc, clones are cheap
        match &self {
            CachedData::Uncached { prev } => CachedData::Uncached { prev: prev.clone() },
            CachedData::Waiting { prev } => CachedData::Uncached { prev: prev.clone() },
            CachedData::Filled(prev) => CachedData::Uncached {
                prev: Some(prev.clone()),
            },
        }
    }

    pub fn filled(t: T) -> Self {
        Self::Filled(Arc::new(t))
    }

    fn get(&self) -> Option<Arc<T>> {
        match self {
            CachedData::Uncached { prev } => prev.clone(),
            CachedData::Waiting { prev } => prev.clone(),
            CachedData::Filled(val) => Some(val.clone()),
        }
    }
}

impl<T> CachedData<T>
where
    T: Clone,
{
    /// Return the current value from the cache if it is there. If the cache is
    /// Uncached run `f` to fetch the new value. The function must make sure that
    /// the cache is updated eventually. The state is changed to `Waiting`
    fn fetch_if_needed(&mut self, f: impl FnOnce()) -> Option<Arc<T>> {
        if let CachedData::Uncached { .. } = self {
            f();
        }
        match self {
            CachedData::Uncached { prev } => {
                let result = prev.as_ref().cloned();
                *self = CachedData::Waiting { prev: prev.clone() };
                result
            }
            CachedData::Waiting { prev } => prev.clone(),
            CachedData::Filled(val) => Some(val.clone()),
        }
    }
}

pub struct CxxrtlData {
    scopes_cache: CachedData<HashMap<ScopeRef, CxxrtlScope>>,
    module_item_cache: HashMap<ScopeRef, CachedData<HashMap<VariableRef, CxxrtlItem>>>,
    all_items_cache: CachedData<HashMap<VariableRef, CxxrtlItem>>,

    /// We use the CachedData system to keep track of if we have sent a query request,
    /// but the actual data is stored in the interval_query_cache.
    ///
    /// The held value in the query result is the end timestamp of the current current
    /// interval_query_cache
    query_result: CachedData<CxxrtlTimestamp>,
    interval_query_cache: QueryContainer,

    loaded_signals: Vec<VariableRef>,
    signal_index_map: HashMap<VariableRef, usize>,

    simulation_status: CachedData<CxxrtlSimulationStatus>,

    msg_channel: std::sync::mpsc::Sender<Message>,
}

impl CxxrtlData {
    pub fn trigger_redraw(&self) {
        self.msg_channel.send(Message::InvalidateDrawCommands).unwrap();
        if let Some(ctx) = crate::EGUI_CONTEXT.read().unwrap().as_ref() {
            ctx.request_repaint();
        }
    }

    pub fn on_simulation_status_update(&mut self, status: CxxrtlSimulationStatus) {
        self.simulation_status = CachedData::filled(status);
        self.trigger_redraw();
        self.invalidate_query_result();
    }

    pub fn invalidate_query_result(&mut self) {
        self.query_result = self.query_result.make_uncached();
        self.trigger_redraw();
        // self.interval_query_cache.invalidate();
    }
}

macro_rules! expect_response {
    ($expected:pat, $response:expr) => {
        let $expected = $response else {
            log::error!(
                "Got unexpected response. Got {:?} expected {}",
                $response,
                stringify!(expected)
            );
            return;
        };
    };
}

struct CSSender {
    cs_messages: mpsc::Sender<String>,
    callback_queue: VecDeque<Callback>,
}

impl CSSender {
    fn run_command<F>(&mut self, command: CxxrtlCommand, f: F)
    where
        F: 'static + FnOnce(CommandResponse, &mut CxxrtlData) + Sync + Send,
    {
        self.callback_queue.push_back(Box::new(f));
        let json = serde_json::to_string(&CSMessage::command(command))
            .expect("Failed to encode cxxrtl command");
        block_on(self.cs_messages.send(json)).unwrap();
    }
}

pub struct CxxrtlContainer {
    data: CxxrtlData,
    sending: CSSender,
    sc_messages: mpsc::Receiver<String>,
    disconnected_reported: bool,
    msg_channel: std::sync::mpsc::Sender<Message>,
}

impl CxxrtlContainer {
    async fn new(
        msg_channel: std::sync::mpsc::Sender<Message>,
        sending: CSSender,
        sc_messages: mpsc::Receiver<String>,
    ) -> Result<Self> {
        info!("Sending cxxrtl greeting");
        sending
            .cs_messages
            .send(serde_json::to_string(&CSMessage::greeting { version: 0 }).unwrap())
            .await
            .unwrap();

        let data = CxxrtlData {
            scopes_cache: CachedData::empty(),
            module_item_cache: HashMap::new(),
            all_items_cache: CachedData::empty(),
            query_result: CachedData::empty(),
            interval_query_cache: QueryContainer::empty(),
            loaded_signals: vec![],
            signal_index_map: HashMap::new(),
            simulation_status: CachedData::empty(),
            msg_channel: msg_channel.clone(),
        };

        let result = Self {
            data,
            sc_messages,
            sending,
            disconnected_reported: false,
            msg_channel,
        };

        info!("cxxrtl connected");

        Ok(result)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn new_tcp(
        addr: &str,
        msg_channel: std::sync::mpsc::Sender<Message>,
    ) -> Result<Self> {
        use crate::cxxrtl::io_worker;

        let stream = tokio::net::TcpStream::connect(addr)
            .await
            .with_context(|| format!("Failed to connect to {addr}"))?;

        let (read, write) = tokio::io::split(stream);

        let (cs_tx, cs_rx) = mpsc::channel(100);
        let (sc_tx, sc_rx) = mpsc::channel(100);
        tokio::spawn(io_worker::CxxrtlWorker::new(write, read, sc_tx, cs_rx).start());

        let result = Self::new(
            msg_channel,
            CSSender {
                cs_messages: cs_tx,
                callback_queue: VecDeque::new(),
            },
            sc_rx,
        )
        .await;

        result
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn new_wasm_mailbox(msg_channel: std::sync::mpsc::Sender<Message>) -> Result<Self> {
        use color_eyre::eyre::anyhow;

        use crate::wasm_api::{CXXRTL_CS_HANDLER, CXXRTL_SC_HANDLER};

        let result = Self::new(
            msg_channel,
            CSSender {
                cs_messages: CXXRTL_CS_HANDLER.tx.clone(),
                callback_queue: VecDeque::new(),
            },
            CXXRTL_SC_HANDLER
                .rx
                .write()
                .await
                .take()
                .ok_or_else(|| anyhow!("The wasm mailbox has already been consumed."))?,
        )
        .await;

        result
    }

    pub fn tick(&mut self) {
        loop {
            match self.sc_messages.try_recv() {
                Ok(s) => {
                    OUTSTANDING_TRANSACTIONS.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                    info!("CXXRTL S>C: {s}");
                    let msg = match serde_json::from_str::<SCMessage>(&s) {
                        Ok(msg) => msg,
                        Err(e) => {
                            error!("Got an unrecognised message from the cxxrtl server {e}");
                            continue;
                        }
                    };
                    match msg {
                        SCMessage::greeting { .. } => {
                            info!("Received cxxrtl greeting")
                        }
                        SCMessage::response(response) => {
                            if let Some(cb) = self.sending.callback_queue.pop_front() {
                                cb(response, &mut self.data)
                            } else {
                                error!("Got a CXXRTL message with no corresponding callback")
                            };
                        }
                        SCMessage::error(e) => {
                            error!("CXXRTL error: '{}'", e.message);
                            self.sending.callback_queue.pop_front();
                        }
                        SCMessage::event(event) => match event {
                            Event::simulation_paused { time, cause: _ } => {
                                self.data
                                    .on_simulation_status_update(CxxrtlSimulationStatus {
                                        status: SimulationStatusType::paused,
                                        latest_time: time,
                                    });
                            }
                            Event::simulation_finished { time } => {
                                self.data
                                    .on_simulation_status_update(CxxrtlSimulationStatus {
                                        status: SimulationStatusType::finished,
                                        latest_time: time,
                                    });
                            }
                        },
                    }
                }
                Err(mpsc::error::TryRecvError::Empty) => {
                    break;
                }
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    OUTSTANDING_TRANSACTIONS.store(0, std::sync::atomic::Ordering::SeqCst);
                    if !self.disconnected_reported {
                        error!("CXXRTL sender disconnected");
                        self.disconnected_reported = true;
                    }
                    break;
                }
            }
        }
    }

    fn get_scopes(&mut self) -> Arc<HashMap<ScopeRef, CxxrtlScope>> {
        self.data
            .scopes_cache
            .fetch_if_needed(|| {
                self.sending.run_command(
                    CxxrtlCommand::list_scopes { scope: None },
                    |response, data| {
                        expect_response!(CommandResponse::list_scopes { scopes }, response);

                        let scopes = scopes
                            .into_iter()
                            .map(|(name, s)| {
                                (
                                    ScopeRef {
                                        strs: name
                                            .split(' ')
                                            .map(std::string::ToString::to_string)
                                            .collect(),
                                        id: ScopeId::None,
                                    },
                                    s,
                                )
                            })
                            .collect();

                        data.scopes_cache = CachedData::filled(scopes);
                    },
                );
            })
            .unwrap_or_else(|| Arc::new(HashMap::new()))
    }

    /// Fetches the details on a specific item. For now, this fetches *all* items, but looks
    /// up the specific item before returning. This is done in order to not have to return
    /// the whole Item list since we need to lock the data structure to get that.
    fn fetch_item(&mut self, var: &VariableRef) -> Option<CxxrtlItem> {
        self.data
            .all_items_cache
            .fetch_if_needed(|| {
                self.sending.run_command(
                    CxxrtlCommand::list_items { scope: None },
                    |response, data| {
                        expect_response!(CommandResponse::list_items { items }, response);

                        let items = Self::item_list_to_hash_map(items);

                        data.all_items_cache = CachedData::filled(items);
                    },
                );
            })
            .and_then(|d| d.get(var).cloned())
    }

    fn fetch_all_items(&mut self) -> Option<Arc<HashMap<VariableRef, CxxrtlItem>>> {
        self.data
            .all_items_cache
            .fetch_if_needed(|| {
                self.sending.run_command(
                    CxxrtlCommand::list_items { scope: None },
                    |response, data| {
                        expect_response!(CommandResponse::list_items { items }, response);

                        let items = Self::item_list_to_hash_map(items);

                        data.all_items_cache = CachedData::filled(items);
                    },
                );
            })
            .clone()
    }

    fn fetch_items_in_module(&mut self, scope: &ScopeRef) -> Arc<HashMap<VariableRef, CxxrtlItem>> {
        let result = self
            .data
            .module_item_cache
            .entry(scope.clone())
            .or_insert(CachedData::empty())
            .fetch_if_needed(|| {
                let scope = scope.clone();
                self.sending.run_command(
                    CxxrtlCommand::list_items {
                        scope: Some(scope.cxxrtl_repr()),
                    },
                    move |response, data| {
                        expect_response!(CommandResponse::list_items { items }, response);

                        let items = Self::item_list_to_hash_map(items);

                        data.module_item_cache
                            .insert(scope.clone(), CachedData::filled(items));
                    },
                );
            });

        result.unwrap_or_default()
    }

    fn item_list_to_hash_map(
        items: HashMap<String, CxxrtlItem>,
    ) -> HashMap<VariableRef, CxxrtlItem> {
        items
            .into_iter()
            .filter_map(|(k, v)| {
                let sp = k.split(' ').collect::<Vec<_>>();

                if sp.is_empty() {
                    error!("Found an empty variable name and scope");
                    None
                } else {
                    Some((
                        VariableRef {
                            path: ScopeRef::from_strs(
                                &sp[0..sp.len() - 1]
                                    .iter()
                                    .map(std::string::ToString::to_string)
                                    .collect::<Vec<_>>(),
                            ),
                            name: sp.last().unwrap().to_string(),
                            id: VarId::None,
                        },
                        v,
                    ))
                }
            })
            .collect()
    }

    fn scopes(&mut self) -> Option<Arc<HashMap<ScopeRef, CxxrtlScope>>> {
        Some(self.get_scopes())
    }

    pub fn modules(&mut self) -> Vec<ScopeRef> {
        if let Some(scopes) = &self.scopes() {
            scopes.iter().map(|(k, _)| k.clone()).collect()
        } else {
            vec![]
        }
    }

    pub fn root_modules(&mut self) -> Vec<ScopeRef> {
        // In the cxxrtl protocol, the root scope is always ""
        if self.scopes().is_some() {
            vec![ScopeRef {
                strs: vec![],
                id: ScopeId::None,
            }]
        } else {
            vec![]
        }
    }

    pub fn module_exists(&mut self, module: &ScopeRef) -> bool {
        self.scopes().is_some_and(|s| s.contains_key(module))
    }

    pub fn child_scopes(&mut self, parent: &ScopeRef) -> Vec<ScopeRef> {
        self.scopes()
            .map(|scopes| {
                scopes
                    .keys()
                    .filter_map(|scope| {
                        if scope.strs().len() == parent.strs().len() + 1 {
                            if scope.strs()[0..parent.strs().len()]
                                == parent.strs()[0..parent.strs().len()]
                            {
                                Some(scope.clone())
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn variables_in_module(&mut self, module: &ScopeRef) -> Vec<VariableRef> {
        self.fetch_items_in_module(module).keys().cloned().collect()
    }

    pub fn no_variables_in_module(&mut self, module: &ScopeRef) -> bool {
        self.fetch_items_in_module(module).is_empty()
    }

    pub fn variable_meta(&mut self, variable: &VariableRef) -> Result<VariableMeta> {
        Ok(self
            .fetch_item(variable)
            .map(|item| VariableMeta {
                var: variable.clone(),
                num_bits: Some(item.width),
                variable_type: None,
                index: None,
                direction: None,
                enum_map: Default::default(),
                encoding: VariableEncoding::BitVector,
            })
            .unwrap_or_else(|| VariableMeta {
                var: variable.clone(),
                num_bits: None,
                variable_type: None,
                index: None,
                direction: None,
                enum_map: Default::default(),
                encoding: VariableEncoding::BitVector,
            }))
    }

    pub fn max_displayed_timestamp(&self) -> Option<CxxrtlTimestamp> {
        self.data.query_result.get().map(|t| (*t).clone())
    }

    pub fn max_timestamp(&mut self) -> Option<CxxrtlTimestamp> {
        self.raw_simulation_status().map(|s| s.latest_time)
    }

    pub fn query_variable(
        &mut self,
        variable: &VariableRef,
        time: &BigUint,
    ) -> Option<QueryResult> {
        // Before we can query any signals, we need some other data available. If we don't have
        // that we'll early return with no value
        let max_timestamp = self.max_timestamp()?;
        let info = self.fetch_all_items()?;
        let loaded_signals = self.data.loaded_signals.clone();

        let res = self
            .data
            .query_result
            .fetch_if_needed(|| {
                info!("Running query variable");

                self.sending.run_command(
                    CxxrtlCommand::query_interval {
                        interval: (CxxrtlTimestamp::zero(), max_timestamp.clone()),
                        collapse: true,
                        items: Some(DEFAULT_REFERENCE.to_string()),
                        item_values_encoding: "base64(u32)",
                        diagnostics: false,
                    },
                    move |response, data| {
                        expect_response!(CommandResponse::query_interval { samples }, response);

                        data.query_result = CachedData::filled(max_timestamp);
                        data.interval_query_cache.populate(
                            loaded_signals.clone(),
                            info,
                            samples,
                            data.msg_channel.clone(),
                        );
                    },
                );
            })
            .map(|_cached| {
                // If we get here, the cache is valid and we we should look into the
                // interval_query_cache for the query result
                self.data
                    .interval_query_cache
                    .query(variable, time.to_bigint().unwrap())
            })
            .unwrap_or_default();
        Some(res)
    }

    pub fn load_variables<S: AsRef<VariableRef>, T: Iterator<Item = S>>(&mut self, variables: T) {
        let data = &mut self.data;
        for variable in variables {
            let varref = variable.as_ref().clone();

            if !data.signal_index_map.contains_key(&varref) {
                let idx = data.loaded_signals.len();
                data.signal_index_map.insert(varref.clone(), idx);
                data.loaded_signals.push(varref.clone());
            }
        }

        self.sending.run_command(
            CxxrtlCommand::reference_items {
                reference: DEFAULT_REFERENCE.to_string(),
                items: data
                    .loaded_signals
                    .iter()
                    .map(|s| vec![s.cxxrtl_repr()])
                    .collect(),
            },
            |_response, data| {
                info!("Item references updated");
                data.invalidate_query_result();
            },
        );
    }

    fn raw_simulation_status(&mut self) -> Option<CxxrtlSimulationStatus> {
        self.data
            .simulation_status
            .fetch_if_needed(|| {
                self.sending
                    .run_command(CxxrtlCommand::get_simulation_status, |response, data| {
                        expect_response!(CommandResponse::get_simulation_status(status), response);

                        data.on_simulation_status_update(status);
                    });
            })
            .map(|s| s.as_ref().clone())
    }

    pub fn simulation_status(&mut self) -> Option<SimulationStatus> {
        self.raw_simulation_status().map(|s| match s.status {
            SimulationStatusType::running => SimulationStatus::Running,
            SimulationStatusType::paused => SimulationStatus::Paused,
            SimulationStatusType::finished => SimulationStatus::Finished,
        })
    }

    pub fn unpause(&mut self) {
        let duration = self
            .raw_simulation_status()
            .map(|s| {
                CxxrtlTimestamp::from_femtoseconds(
                    s.latest_time.as_femtoseconds() + 100_000_000u32.to_biguint().unwrap(),
                )
            })
            .unwrap_or_else(|| {
                CxxrtlTimestamp::from_femtoseconds(100_000_000u32.to_biguint().unwrap())
            });

        let cmd = CxxrtlCommand::run_simulation {
            until_time: Some(duration),
            until_diagnostics: vec![],
            sample_item_values: true,
        };

        self.sending.run_command(cmd, |_, data| {
            data.simulation_status = CachedData::filled(CxxrtlSimulationStatus {
                status: SimulationStatusType::running,
                latest_time: CxxrtlTimestamp::zero(),
            });
            info!("Unpausing simulation");
        });
    }

    pub fn pause(&mut self) {
        self.sending
            .run_command(CxxrtlCommand::pause_simulation, |response, data| {
                expect_response!(CommandResponse::pause_simulation { time }, response);

                data.on_simulation_status_update(CxxrtlSimulationStatus {
                    status: SimulationStatusType::paused,
                    latest_time: time,
                });
            });
    }
}
