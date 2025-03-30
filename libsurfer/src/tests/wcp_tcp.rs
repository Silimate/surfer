use crate::message::Message;
use crate::wave_source::LoadOptions;
use crate::wcp::proto::{WcpCSMessage, WcpCommand, WcpSCMessage};
use crate::SystemState;

use port_check::free_local_ipv4_port_in_range;
use serde_json::Error as serde_Error;
use test_log::test;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{sleep, timeout, Duration};

use itertools::Itertools;
use lazy_static::lazy_static;
use std::future::Future;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

fn get_test_port() -> u16 {
    lazy_static! {
        static ref PORT_NUM: Arc<Mutex<u16>> = Arc::new(Mutex::new(54321));
    }
    let mut port = PORT_NUM.lock().unwrap();
    let free = free_local_ipv4_port_in_range(*port + 1..65535u16);
    *port = free.unwrap();
    *port
}

async fn get_json_response(
    stream: &TcpStream,
    state: &mut SystemState,
) -> Result<WcpSCMessage, serde_Error> {
    loop {
        state.handle_wcp_commands();
        state.handle_async_messages();
        let mut data = vec![0; 1024];
        match stream.try_read(&mut data) {
            Ok(0) => panic!("EOF"),
            Ok(size) => {
                let cmd: Result<WcpSCMessage, _> = serde_json::from_slice(&data[..size - 1]);
                assert_eq!(data[size - 1], 0);
                return cmd;
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => (),
            Err(e) => panic!("Read failure: {e}"),
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn send_message(stream: &mut TcpStream, msg: &WcpCSMessage) {
    let msg = serde_json::to_string(msg).unwrap();
    stream
        .write_all(msg.as_bytes())
        .await
        .expect("Failed to message");
    stream.write_all(b"\0").await.expect("Failed to null byte");
    stream.flush().await.expect("Failed to flush");
}

async fn greet(stream: &mut TcpStream) {
    let commands = vec!["waveforms_loaded"]
        .into_iter()
        .map(str::to_string)
        .collect_vec();

    send_message(stream, &WcpCSMessage::create_greeting(0, commands)).await;
}

async fn connect(port: u16) -> TcpStream {
    let mut stream: TcpStream;
    loop {
        if let Ok(c) = TcpStream::connect(format!("127.0.0.1:{port}")).await {
            stream = c;
            break;
        }
        sleep(Duration::from_millis(100)).await;
    }
    greet(&mut stream).await;

    stream
}

async fn expect_disconnect(stream: &TcpStream) {
    loop {
        let mut buf = [0; 1024];
        stream.readable().await.expect("Stream was not readable");
        match stream.try_read(&mut buf) {
            Ok(0) => break,
            Ok(_) => continue,
            Err(_) => break,
        }
    }
}

fn run_test<F>(body: F)
where
    F: Future<Output = ()>,
{
    let runtime = tokio::runtime::Builder::new_current_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();

    runtime.block_on(async {
        if let Err(_) = timeout(Duration::from_secs(30), body).await {
            panic!("Test timed out");
        }
    });
}

#[test]
fn load() {
    run_test(async {
        let mut state = SystemState::new_default_config().unwrap();
        let port = get_test_port();
        state.update(Message::StartWcpServer {
            address: Some(format!("127.0.0.1:{port}").to_string()),
            initiate: false,
        });
        let mut stream = connect(port).await;
        get_json_response(&stream, &mut state)
            .await
            .expect("failed to get WCP greeting");
        send_message(
            &mut stream,
            &WcpCSMessage::command(WcpCommand::load {
                source: "../examples/counter.vcd".to_string(),
            }),
        )
        .await;
        // ack and waveforms_loaded messages
        get_json_response(&stream, &mut state)
            .await
            .expect("failed to get WCP greeting");
        get_json_response(&stream, &mut state)
            .await
            .expect("failed to get WCP greeting");
    });
}

#[test]
fn stop_and_reconnect() {
    run_test(async {
        let mut state = SystemState::new_default_config().unwrap();
        let port = get_test_port();
        for _ in 0..2 {
            state.update(Message::StartWcpServer {
                address: Some(format!("127.0.0.1:{port}").to_string()),
                initiate: false,
            });
            let stream = connect(port).await;
            get_json_response(&stream, &mut state)
                .await
                .expect("failed to get WCP greeting");
            state.update(Message::StopWcpServer);
            expect_disconnect(&stream).await;
            loop {
                if !state.wcp_running_signal.load(Ordering::Relaxed) {
                    break;
                }
                sleep(Duration::from_millis(100)).await;
            }
        }
    });
}

#[test]
fn reconnect() {
    run_test(async {
        let mut state = SystemState::new_default_config().unwrap();
        let port = get_test_port();
        state.update(Message::StartWcpServer {
            address: Some(format!("127.0.0.1:{port}").to_string()),
            initiate: false,
        });
        for _ in 0..2 {
            let stream = connect(port).await;
            get_json_response(&stream, &mut state)
                .await
                .expect("failed to get WCP greeting");
        }
    });
}

#[test]
fn initiate() {
    run_test(async {
        let mut state = SystemState::new_default_config().unwrap();
        let port = get_test_port();
        let address = format!("127.0.0.1:{port}").to_string();
        let listener = TcpListener::bind(address.clone()).await.unwrap();
        state.update(Message::StartWcpServer {
            address: Some(address),
            initiate: true,
        });
        if let Ok((mut stream, _addr)) = listener.accept().await {
            greet(&mut stream).await;
            get_json_response(&stream, &mut state)
                .await
                .expect("failed to get WCP greeting");
        } else {
            panic!("Failed to connect");
        }
    });
}

async fn is_connected(stream: &TcpStream) -> bool {
    let mut buf = [0; 1];
    let result = stream.try_read(&mut buf);

    match result {
        Ok(0) => false, // Connection closed (EOF)
        Ok(_) => true,  // Data available
        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => true, // No data but still connected
        Err(_) => false, // Other error, likely disconnected
    }
}

#[test]
#[ignore = "This test is long running and disabled by default"]
fn long_pause() {
    run_test(async {
        let mut state = SystemState::new_default_config().unwrap();
        let port = get_test_port();
        state.update(Message::StartWcpServer {
            address: Some(format!("127.0.0.1:{port}").to_string()),
            initiate: false,
        });
        let stream = connect(port).await;
        get_json_response(&stream, &mut state)
            .await
            .expect("failed to get WCP greeting");

        // confirm that we can be silent for a while and still be connected
        std::thread::sleep(Duration::from_secs(10));
        if !is_connected(&stream).await {
            panic!("No longer connected");
        }
    });
}

#[test]
fn start_stop() {
    run_test(async {
        let mut state = SystemState::new_default_config().unwrap();
        let port = get_test_port();
        state.update(Message::StartWcpServer {
            address: Some(format!("127.0.0.1:{port}").to_string()),
            initiate: false,
        });
        tokio::time::sleep(Duration::from_millis(1000)).await;
        state.update(Message::StopWcpServer);
        tokio::time::sleep(Duration::from_millis(1000)).await;
        if let Ok(_) = TcpStream::connect(format!("127.0.0.1:{port}")).await {
            panic!("Connected after stopping server");
        }
    });
}

#[test]
fn response_and_event() {
    run_test(async {
        let mut state = SystemState::new_default_config().unwrap();
        let port = get_test_port();
        state.update(Message::StartWcpServer {
            address: Some(format!("127.0.0.1:{port}").to_string()),
            initiate: false,
        });
        let msg_sender = state.channels.msg_sender.clone();
        let mut stream = connect(port).await;
        get_json_response(&stream, &mut state)
            .await
            .expect("failed to get WCP greeting");
        send_message(
            &mut stream,
            &(WcpCSMessage::command(WcpCommand::get_item_list)),
        )
        .await;
        get_json_response(&stream, &mut state)
            .await
            .expect("failed to get get_item_list response");
        msg_sender
            .send(Message::LoadFile(
                "../examples/counter.vcd".into(),
                LoadOptions::clean(),
            ))
            .unwrap();
        get_json_response(&stream, &mut state)
            .await
            .expect("failed to get waveforms_loaded response");
    });
}
