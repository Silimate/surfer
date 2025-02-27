use crate::message::Message;
use crate::wcp::proto::WcpSCMessage;
use crate::State;

use serde::Deserialize;
use serde_json::Error as serde_Error;
use test_log::test;

use lazy_static::lazy_static;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::{
    io::{self, Read},
    net::{Shutdown, TcpListener, TcpStream},
    thread,
    time::{Duration, Instant},
};

struct DebugReader<R: std::io::Read> {
    r: R,
}

impl<R: std::io::Read> std::io::Read for DebugReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let count = self.r.read(buf)?;
        println!(
            "Read {} ({:?})",
            String::from_utf8_lossy(&buf[0..count]),
            &buf[0..count]
        );
        Ok(count)
    }
}

struct SkipNullReader<R: std::io::Read> {
    r: R,
}

impl<R: std::io::Read> std::io::Read for SkipNullReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let count = self.r.read(&mut buf[0..1])?;
        if count == 1 && buf[0] == b'\0' {
            self.r.read(buf)
        } else {
            Ok(count)
        }
    }
}

fn get_test_port() -> usize {
    lazy_static! {
        static ref PORT_NUM: Arc<Mutex<usize>> = Arc::new(Mutex::new(54321));
    }
    let mut port = PORT_NUM.lock().unwrap();
    *port += 1;
    *port
}

fn consume_stream(mut stream: &TcpStream) -> () {
    let mut buf = [0; 1024];

    stream.set_nonblocking(true).ok();
    let _ = stream.read(&mut buf);
    stream.set_nonblocking(false).ok();
}

fn get_json_response(mut stream: &TcpStream) -> Result<WcpSCMessage, serde_Error> {
    let mut de = serde_json::Deserializer::from_reader(DebugReader {
        r: SkipNullReader { r: &mut stream },
    });
    let message = WcpSCMessage::deserialize(&mut de);
    // Need to eat the message separator
    consume_stream(stream);
    message
}

fn connect(port: usize) -> TcpStream {
    let timeout = Duration::from_secs(2);
    let now = Instant::now();
    loop {
        assert!(now.elapsed() <= timeout);
        if let Ok(c) = TcpStream::connect(format!("127.0.0.1:{port}")) {
            return c;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn expect_disconnect(mut stream: &TcpStream) {
    loop {
        let mut buf = [0; 1024];
        let size = stream.read(&mut buf).unwrap();
        if size == 0 {
            break;
        }
    }
}

#[test]
fn stop_and_reconnect() {
    let mut state = State::new_default_config().unwrap();
    let port = get_test_port();
    for _ in 0..2 {
        state.update(Message::StartWcpServer {
            address: Some(format!("127.0.0.1:{port}").to_string()),
            initiate: false,
        });
        let stream = connect(port);
        get_json_response(&stream).expect("failed to get WCP greeting");
        state.update(Message::StopWcpServer);
        expect_disconnect(&stream);
        loop {
            if !state.sys.wcp_running_signal.load(Ordering::Relaxed) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

#[test]
fn reconnect() {
    let mut state = State::new_default_config().unwrap();
    let port = get_test_port();
    state.update(Message::StartWcpServer {
        address: Some(format!("127.0.0.1:{port}").to_string()),
        initiate: false,
    });
    for _ in 0..2 {
        let stream = connect(port);
        get_json_response(&stream).expect("failed to get WCP greeting");
        stream
            .shutdown(Shutdown::Both)
            .expect("failed to shutdown TCP session");
    }
}

#[test]
fn initiate() {
    let mut state = State::new_default_config().unwrap();
    let port = get_test_port();
    let address = format!("127.0.0.1:{port}").to_string();
    let listener = TcpListener::bind(address.clone());
    state.update(Message::StartWcpServer {
        address: Some(address),
        initiate: true,
    });
    if let Some(stream) = listener.unwrap().incoming().next() {
        let stream = stream.unwrap();
        get_json_response(&stream).expect("failed to get WCP greeting");
        stream
            .shutdown(Shutdown::Both)
            .expect("failed to shutdown TCP session");
    } else {
        panic!("Failed to connect");
    }
}

fn is_connected(stream: &TcpStream) -> bool {
    let mut buf = [0; 1];

    // Set non-blocking mode
    stream.set_nonblocking(true).ok();

    // Try to peek at the stream
    let result = stream.peek(&mut buf);

    // Reset back to blocking mode if needed
    stream.set_nonblocking(false).ok();

    match result {
        Ok(0) => false, // Connection closed (EOF)
        Ok(_) => true,  // Data available
        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => true, // No data but still connected
        Err(_) => false, // Other error, likely disconnected
    }
}

#[test]
#[ignore = "This test is long running and disabled by default"]
fn long_pause() {
    let mut state = State::new_default_config().unwrap();
    let port = get_test_port();
    state.update(Message::StartWcpServer {
        address: Some(format!("127.0.0.1:{port}").to_string()),
        initiate: false,
    });
    let stream = connect(port);
    get_json_response(&stream).expect("failed to get WCP greeting");

    // confirm that we can be silent for a while and still be connected
    std::thread::sleep(Duration::from_millis(10000));
    if !is_connected(&stream) {
        panic!("No longer connected");
    }
    stream
        .shutdown(Shutdown::Both)
        .expect("failed to shutdown TCP session");
}
