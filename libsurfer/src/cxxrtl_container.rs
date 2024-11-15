use futures::executor::block_on;
use std::{
    collections::{HashMap, VecDeque},
    io::Write,
    sync::Arc,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::mpsc,
    sync::RwLock,
};

use color_eyre::{eyre::Context, Result};
use log::{error, info, trace, warn};
use num::{
    bigint::{ToBigInt, ToBigUint},
    BigUint,
};
use serde::Deserialize;
use surfer_translation_types::VariableEncoding;

use crate::{cxxrtl::io_worker::CxxrtlWorker, wave_container::ScopeRefExt};
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

const DEFAULT_REFERENCE: &str = "ALL_VARIABLES";

pub type Callback = Box<dyn FnOnce(CommandResponse, &mut CxxrtlData) + Sync + Send>;

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
        match self {
            CachedData::Uncached { prev } => {
                f();
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
    pub fn on_simulation_status_update(&mut self, status: CxxrtlSimulationStatus) {
        self.simulation_status = CachedData::filled(status);
        let _ = self.msg_channel.send(Message::InvalidateDrawCommands);
        self.invalidate_query_result();
    }

    pub fn invalidate_query_result(&mut self) {
        self.query_result = self.query_result.make_uncached();
        let _ = self.msg_channel.send(Message::InvalidateDrawCommands);
        // self.interval_query_cache.invalidate();
    }
}

macro_rules! expect_response {
    ($expected:pat, $response:expr) => {
        let $expected = $response else {
            error!(
                "Got unexpected response. Got {:?} expected {}",
                $response,
                stringify!(expected)
            );
            return;
        };
    };
}

pub struct CxxrtlContainer {
    command_channel: mpsc::Sender<(CxxrtlCommand, Callback)>,
    data: Arc<RwLock<CxxrtlData>>,
}

impl CxxrtlContainer {
    async fn new(
        read: impl AsyncReadExt + Unpin + Send + 'static,
        mut write: impl AsyncWriteExt + Unpin + Send + 'static,
        msg_channel: std::sync::mpsc::Sender<Message>,
    ) -> Result<Self> {
        let greeting = serde_json::to_string(&CSMessage::greeting { version: 0 })
            .with_context(|| "Failed to encode greeting message".to_string())?;

        trace!("Sending greeting {greeting}");
        write.write_all(greeting.as_bytes()).await?;
        write.write_all(&[b'\0']).await?;
        write.flush().await?;

        let data = Arc::new(RwLock::new(CxxrtlData {
            scopes_cache: CachedData::empty(),
            module_item_cache: HashMap::new(),
            all_items_cache: CachedData::empty(),
            query_result: CachedData::empty(),
            interval_query_cache: QueryContainer::empty(),
            loaded_signals: vec![],
            signal_index_map: HashMap::new(),
            simulation_status: CachedData::empty(),
            msg_channel,
        }));

        let (tx, rx) = mpsc::channel(100);

        let data_ = data.clone();
        tokio::spawn(async move {
            CxxrtlWorker {
                read,
                write,
                read_buf: VecDeque::new(),
                command_channel: rx,
                data: data_,
                callback_queue: VecDeque::new(),
            }
            .start()
            .await;
        });

        let result = Self {
            command_channel: tx,
            data,
        };

        info!("cxxrtl connected");

        Ok(result)
    }

    pub async fn new_tcp(
        addr: &str,
        msg_channel: std::sync::mpsc::Sender<Message>,
    ) -> Result<Self> {
        let stream = tokio::net::TcpStream::connect(addr)
            .await
            .with_context(|| format!("Failed to connect to {addr}"))?;

        let (read, write) = tokio::io::split(stream);

        let result = Self::new(read, write, msg_channel).await;

        result
    }

    // TODO: Replace the channel with a tokio channel
    pub async fn new_stdio(
        binary: &str,
        msg_channel: std::sync::mpsc::Sender<Message>,
    ) -> Result<Self> {
        let mut child = tokio::process::Command::new(binary)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .context("Failed to spawn {binary}")?;

        Self::new(
            child.stdout.take().unwrap(),
            child.stdin.take().unwrap(),
            msg_channel,
        )
        .await
    }

    fn get_scopes(&mut self) -> Arc<HashMap<ScopeRef, CxxrtlScope>> {
        block_on(self.data.write())
            .scopes_cache
            .fetch_if_needed(|| {
                self.run_command(
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
        block_on(self.data.write())
            .all_items_cache
            .fetch_if_needed(|| {
                self.run_command(
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
        block_on(self.data.write())
            .all_items_cache
            .fetch_if_needed(|| {
                self.run_command(
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
        let result = block_on(self.data.write())
            .module_item_cache
            .entry(scope.clone())
            .or_insert(CachedData::empty())
            .fetch_if_needed(|| {
                let scope = scope.clone();
                self.run_command(
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
        block_on(self.data.read())
            .query_result
            .get()
            .map(|t| (*t).clone())
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
        let loaded_signals = block_on(self.data.read()).loaded_signals.clone();

        let s = &self;

        let mut data = block_on(self.data.write());
        let res = data
            .query_result
            .fetch_if_needed(move || {
                info!("Running query variable");

                s.run_command(
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
                            loaded_signals,
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
                data.interval_query_cache
                    .query(variable, time.to_bigint().unwrap())
            })
            .unwrap_or_default();
        Some(res)
    }

    pub fn load_variables<S: AsRef<VariableRef>, T: Iterator<Item = S>>(&mut self, variables: T) {
        let mut data = block_on(self.data.write());
        for variable in variables {
            let varref = variable.as_ref().clone();

            if !data.signal_index_map.contains_key(&varref) {
                let idx = data.loaded_signals.len();
                data.signal_index_map.insert(varref.clone(), idx);
                data.loaded_signals.push(varref.clone());
            }
        }

        self.run_command(
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

    fn raw_simulation_status(&self) -> Option<CxxrtlSimulationStatus> {
        block_on(self.data.write())
            .simulation_status
            .fetch_if_needed(|| {
                self.run_command(CxxrtlCommand::get_simulation_status, |response, data| {
                    expect_response!(CommandResponse::get_simulation_status(status), response);

                    data.on_simulation_status_update(status);
                });
            })
            .map(|s| s.as_ref().clone())
    }

    pub fn simulation_status(&self) -> Option<SimulationStatus> {
        self.raw_simulation_status().map(|s| match s.status {
            SimulationStatusType::running => SimulationStatus::Running,
            SimulationStatusType::paused => SimulationStatus::Paused,
            SimulationStatusType::finished => SimulationStatus::Finished,
        })
    }

    pub fn unpause(&self) {
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

        self.run_command(cmd, |_, data| {
            data.simulation_status = CachedData::filled(CxxrtlSimulationStatus {
                status: SimulationStatusType::running,
                latest_time: CxxrtlTimestamp::zero(),
            });
            info!("Unpausing simulation");
        });
    }

    pub fn pause(&self) {
        self.run_command(CxxrtlCommand::pause_simulation, |response, data| {
            expect_response!(CommandResponse::pause_simulation { time }, response);

            data.on_simulation_status_update(CxxrtlSimulationStatus {
                status: SimulationStatusType::paused,
                latest_time: time,
            });
        });
    }

    fn run_command<F>(&self, command: CxxrtlCommand, f: F)
    where
        F: 'static + FnOnce(CommandResponse, &mut CxxrtlData) + Sync + Send,
    {
        block_on(self.command_channel.send((command, Box::new(f))))
            .expect("CXXRTL command channel disconnected");
    }
}
