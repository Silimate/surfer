use serde::{Deserialize, Serialize};

use super::wcp_handler::Vecs;

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(tag = "type")]
pub enum WcpSCMessage {
    #[serde(rename = "greeting")]
    Greeting {
        version: String,
        commands: Vec<String>,
    },
    #[serde(rename = "response")]
    Response { command: String, arguments: Vecs },
    #[serde(rename = "error")]
    Error {
        error: String,
        arguments: Vec<String>,
        message: String,
    },
    #[serde(rename = "event")]
    Event {
        event: String,
        arguments: Vec<String>,
    },
}

impl WcpSCMessage {
    pub fn create_greeting(version: usize, commands: Vec<String>) -> Self {
        Self::Greeting {
            version: version.to_string(),
            commands,
        }
    }
    pub fn create_response(command: String, arguments: Vecs) -> Self {
        Self::Response { command, arguments }
    }
    pub fn create_error(error: String, arguments: Vec<String>, message: String) -> Self {
        Self::Error {
            error,
            arguments,
            message,
        }
    }
    pub fn _create_event(event: String, arguments: Vec<String>) -> Self {
        Self::Event { event, arguments }
    }
}
