use std::path::PathBuf;

use crate::message::Message;
use crate::tests::snapshot::render_and_compare;
use crate::wcp::proto::{WcpCSMessage, WcpResponse, WcpSCMessage};
use crate::{State, WCP_CS_HANDLER, WCP_SC_HANDLER};

use color_eyre::eyre::bail;
use futures::Future;
use tokio::sync::mpsc::{Receiver, Sender};

async fn expect_ack(rx: &mut tokio::sync::mpsc::Receiver<WcpSCMessage>) -> color_eyre::Result<()> {
    match rx.recv().await {
        Some(WcpSCMessage::response(WcpResponse::ack)) => todo!(),
        Some(other) => bail!("Got {other:?}"),
        None => bail!("Sender disconnected"),
    }
}

macro_rules! expect_response {
    ($expected:pat, $rx:expr) => {
        let received = tokio::select! {
            result = $rx.recv() => {
                result
            }
            _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {
                bail!("Timeout waiting for {}", stringify!($expected))
            }
        };

        let Some($expected) = received else {
            bail!(
                "Got unexpected response {received:?} expected {}",
                stringify!(expected)
            )
        };
    };
}

fn run_wcp_test<C, F>(test_name: String, client: C)
where
    C: Fn(Sender<WcpCSMessage>, Receiver<WcpSCMessage>) -> F + Sync + Send + Clone + 'static,
    F: Future<Output = color_eyre::Result<()>> + Send + Sync,
{
    let test_name = format!("wcp/{test_name}");

    render_and_compare(&PathBuf::from(test_name), move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();

        // create state and add messages as batch commands
        let mut state = State::new_default_config().unwrap();

        let setup_msgs = vec![
            // hide GUI elements
            Message::ToggleMenu,
            Message::ToggleToolbar,
            Message::ToggleOverview,
        ];

        for msg in setup_msgs {
            state.update(msg);
        }

        let (runner_tx, mut runner_rx) = tokio::sync::oneshot::channel();

        println!("Starting test");

        let (sc_tx, sc_rx) = tokio::sync::mpsc::channel(100);
        state.sys.channels.wcp_s2c_sender = Some(sc_tx);
        let (cs_tx, cs_rx) = tokio::sync::mpsc::channel(100);
        state.sys.channels.wcp_c2s_receiver = Some(cs_rx);

        {
            let client = client.clone();
            runtime.spawn(async move {
                let result: color_eyre::Result<()> = async {
                    let tx = &WCP_CS_HANDLER.tx;
                    let mut rx = WCP_SC_HANDLER.rx.write().await;
                    client(cs_tx, sc_rx).await
                }
                .await;
                runner_tx.send(result).unwrap();
            });
        }

        runtime.block_on(async {
            // update state until all batch commands have been processed
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_millis(10)) => {
                        state.handle_async_messages();
                        state.handle_wcp_commands();
                    }
                    exit = &mut runner_rx => {
                        match exit {
                            Ok(Ok(())) => {
                                break;
                            }
                            Ok(Err(e)) => {
                                panic!("Runner exited with\n{e:#}")
                            }
                            Err(e) => {
                                panic!("Runner disconnected with\n{e:#}")
                            }
                        }
                    }

                }
            }

            state
        })
    });
}

macro_rules! wcp_test {
    ($test_name:ident, ($tx:ident, $rx:ident) $body:tt) => {
        #[test]
        fn $test_name() {
            async fn client($tx: Sender<WcpCSMessage>, mut $rx: Receiver<WcpSCMessage>) -> color_eyre::Result<()> $body

            run_wcp_test(stringify!($test_name).to_string(), client)
        }
    };
}

wcp_test! {greeting_works, (tx, rx) {
    tx
        .send(WcpCSMessage::greeting {
            version: "0".to_string(),
            commands: vec![],
        })
        .await?;

    expect_response!(
        WcpSCMessage::greeting {
            version: v,
            commands
        },
        rx
    );
    assert_eq!(v, "0");
    let e_commands = vec![
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
    ];
    assert_eq!(commands, e_commands);

    Ok(())
}}
