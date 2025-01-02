use std::path::PathBuf;

use crate::message::Message;
use crate::tests::snapshot::render_and_compare;
use crate::wcp::proto::{self, WcpCSMessage, WcpCommand, WcpEvent, WcpResponse, WcpSCMessage};
use crate::State;

use color_eyre::eyre::bail;
use color_eyre::Result;
use futures::Future;
use num::BigInt;
use tokio::sync::mpsc::{Receiver, Sender};

macro_rules! expect_response {
    ($rx:expr, $expected:pat$(,)?) => {
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

async fn expect_ack(rx: &mut tokio::sync::mpsc::Receiver<WcpSCMessage>) -> Result<()> {
    expect_response! {
        rx, WcpSCMessage::response(WcpResponse::ack)
    }
    Ok(())
}

fn run_wcp_test<C, F>(test_name: String, client: C)
where
    C: Fn(Sender<WcpCSMessage>, Receiver<WcpSCMessage>) -> F + Sync + Send + Clone + 'static,
    F: Future<Output = Result<()>> + Send + Sync,
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

        let (sc_tx, sc_rx) = tokio::sync::mpsc::channel(100);
        state.sys.channels.wcp_s2c_sender = Some(sc_tx);
        let (cs_tx, cs_rx) = tokio::sync::mpsc::channel(100);
        state.sys.channels.wcp_c2s_receiver = Some(cs_rx);

        {
            let client = client.clone();
            runtime.spawn(async move {
                let result: Result<()> = async { client(cs_tx, sc_rx).await }.await;
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
                                state.handle_async_messages();
                                state.handle_wcp_commands();
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

async fn send_commands(tx: &Sender<WcpCSMessage>, cmds: Vec<WcpCommand>) -> Result<()> {
    for cmd in cmds {
        tx.send(WcpCSMessage::command(cmd)).await?
    }
    Ok(())
}

async fn greet(tx: &Sender<WcpCSMessage>, rx: &mut Receiver<WcpSCMessage>) -> Result<()> {
    tx.send(WcpCSMessage::greeting {
        version: "0".to_string(),
        commands: vec![],
    })
    .await?;

    expect_response!(
        rx,
        WcpSCMessage::greeting {
            version: v,
            commands
        },
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
}

async fn load_file(
    tx: &Sender<WcpCSMessage>,
    rx: &mut Receiver<WcpSCMessage>,
    file: &str,
) -> Result<()> {
    greet(tx, rx).await?;

    tx.send(WcpCSMessage::command(proto::WcpCommand::load {
        source: file.to_string(),
    }))
    .await?;
    expect_ack(rx).await?;

    expect_response!(rx, WcpSCMessage::event(WcpEvent::waveforms_loaded));

    Ok(())
}

wcp_test! {greeting_works, (tx, rx) {
    greet(&tx, &mut rx).await?;

    Ok(())
}}

wcp_test! {
    loading_waveforms_works,
    (tx, rx) {
        load_file(&tx, &mut rx, "../examples/counter.vcd").await?;

        tx.send(WcpCSMessage::command(
            proto::WcpCommand::add_variables { names: vec![
                "tb._tmp",
                "tb.clk",
                "tb.overflow",
                "tb.reset"
            ].into_iter().map(str::to_string).collect()
        })).await?;
        expect_response!(rx, WcpSCMessage::response(WcpResponse::add_variables(indices)));

        assert_eq!(indices.len(), 4);

        Ok(())
    }
}

wcp_test! {
    add_scope,
    (tx, rx) {
        greet(&tx, &mut rx).await?;

        tx.send(WcpCSMessage::command(proto::WcpCommand::load {
            source: "../examples/counter.vcd".to_string()
        })).await?;
        expect_ack(&mut rx).await?;

        expect_response!(rx, WcpSCMessage::event(WcpEvent::waveforms_loaded));

        tx.send(WcpCSMessage::command(
            proto::WcpCommand::add_scope {scope: "tb".to_string()})).await?;
        expect_response!(rx, WcpSCMessage::response(WcpResponse::add_scope(indices)));

        assert_eq!(indices.len(), 4);

        Ok(())
    }
}

wcp_test! {
    color_variables,
    (tx, rx) {
        load_file(&tx, &mut rx, "../examples/counter.vcd").await?;

        tx.send(WcpCSMessage::command(
            proto::WcpCommand::add_variables { names: vec![
                "tb._tmp",
                "tb.clk",
                "tb.overflow",
                "tb.reset"
            ].into_iter().map(str::to_string).collect()
        })).await?;

        expect_response!(rx, WcpSCMessage::response(WcpResponse::add_variables(refs)));

        for (i, c) in [(1, "Gray"), (2, "Yellow"), (3, "Blue")] {
            tx.send(WcpCSMessage::command(
                proto::WcpCommand::set_item_color { id: refs[i], color: c.to_string() }
            )).await?;
            expect_ack(&mut rx).await?;
        }

        Ok(())
    }
}

wcp_test! {
    remove_2_variables,
    (tx, rx) {
        load_file(&tx, &mut rx, "../examples/counter.vcd").await?;

        send_commands(&tx, vec![
            WcpCommand::add_scope {scope: "tb".to_string()},
        ]).await?;
        expect_response!(rx, WcpSCMessage::response(WcpResponse::add_scope(refs)));

        send_commands(&tx, vec![
            WcpCommand::remove_items { ids: vec![refs[1], refs[2]] }
        ]).await?;

        expect_ack(&mut rx).await?;

        Ok(())
    }
}

wcp_test! {
    focus_item,
    (tx, rx) {
        load_file(&tx, &mut rx, "../examples/counter.vcd").await?;

        send_commands(&tx, vec![
            WcpCommand::add_scope {scope: "tb".to_string()},
        ]).await?;
        expect_response!(rx, WcpSCMessage::response(WcpResponse::add_scope(refs)));

        send_commands(&tx, vec![
            WcpCommand::focus_item { id: refs[1] }
        ]).await?;
        expect_ack(&mut rx).await
    }
}

wcp_test! {
    clear,
    (tx, rx) {
        load_file(&tx, &mut rx, "../examples/counter.vcd").await?;

        send_commands(&tx, vec![
            WcpCommand::add_scope {scope: "tb".to_string()},
            WcpCommand::clear,
        ]).await?;
        expect_response!(rx, WcpSCMessage::response(WcpResponse::add_scope(_)));
        expect_ack(&mut rx).await
    }
}

wcp_test! {
    set_viewport_to,
    (tx, rx) {
        load_file(&tx, &mut rx, "../examples/counter.vcd").await?;

        send_commands(&tx, vec![
            WcpCommand::add_scope {scope: "tb".to_string()},
            WcpCommand::set_viewport_to { timestamp: BigInt::from(70) },
        ]).await?;
        expect_response!(rx, WcpSCMessage::response(WcpResponse::add_scope(_)));
        expect_ack(&mut rx).await?;
        Ok(())
    }
}

wcp_test! {
    zoom_to_fit,
    (tx, rx) {
        load_file(&tx, &mut rx, "../examples/counter.vcd").await?;

        send_commands(&tx, vec![
            WcpCommand::add_scope {scope: "tb".to_string()},
            WcpCommand::set_viewport_to { timestamp: BigInt::from(70) },
            WcpCommand::zoom_to_fit { viewport_idx: 0 }
        ]).await?;
        expect_response!(rx, WcpSCMessage::response(WcpResponse::add_scope(_)));
        expect_ack(&mut rx).await?;
        expect_ack(&mut rx).await?;

        Ok(())
    }
}

wcp_test! {
    get_item_info,
    (tx, rx) {
        load_file(&tx, &mut rx, "../examples/counter.vcd").await?;

        send_commands(&tx, vec![
            WcpCommand::add_scope {scope: "tb".to_string()},
        ]).await?;
        expect_response!(rx, WcpSCMessage::response(WcpResponse::add_scope(items)));

        send_commands(&tx, vec![
            WcpCommand::get_item_info { ids: items }
        ]).await?;

        expect_response!(rx, WcpSCMessage::response(WcpResponse::get_item_info(info)));
        let expected = vec![
            proto::ItemInfo { name: "overflow".to_string(),
                 t: "Variable".to_string(),
                 id: proto::DisplayedItemRef(1)
             },
             proto::ItemInfo { name: "clk".to_string(),
                 t: "Variable".to_string(),
                 id: proto::DisplayedItemRef(2)
             },
             proto::ItemInfo { name: "reset".to_string(),
                 t: "Variable".to_string(),
                 id: proto::DisplayedItemRef(3)
             },
             proto::ItemInfo { name: "_tmp".to_string(),
                 t: "Variable".to_string(),
                 id: proto::DisplayedItemRef(4)
             }
        ];
        assert_eq!(info, expected);

        Ok(())
    }
}

wcp_test! {
    get_item_info_invalid_id,
    (tx, rx) {
        load_file(&tx, &mut rx, "../examples/counter.vcd").await?;

        send_commands(&tx, vec![
            WcpCommand::add_scope {scope: "tb".to_string()},
        ]).await?;
        expect_response!(rx, WcpSCMessage::response(WcpResponse::add_scope(_)));

        send_commands(&tx, vec![
            WcpCommand::get_item_info { ids: vec![proto::DisplayedItemRef(usize::MAX)] }
        ]).await?;

        expect_response!(rx, WcpSCMessage::error{error, arguments: _, message});
        assert_eq!(error, "get_item_info");
        assert_eq!(message, "No item with id DisplayedItemRef(18446744073709551615)");

        Ok(())
    }
}
