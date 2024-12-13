use num::BigInt;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(tag = "command")]
pub enum WcpCommand {
    #[serde(rename = "get_item_list")]
    GetItemList,
    #[serde(rename = "get_item_info")]
    GetItemInfo { ids: Vec<String> },
    #[serde(rename = "set_item_color")]
    SetItemColor { id: String, color: String },
    #[serde(rename = "add_variables")]
    AddVariables { names: Vec<String> },
    #[serde(rename = "add_scope")]
    AddScope { scope: String },
    #[serde(rename = "reload")]
    Reload,
    #[serde(rename = "set_viewport_to")]
    SetViewportTo { timestamp: BigInt },
    #[serde(rename = "remove_items")]
    RemoveItems { ids: Vec<String> },
    #[serde(rename = "focus_item")]
    FocusItem { id: String },
    #[serde(rename = "clear")]
    Clear,
    #[serde(rename = "load")]
    Load { source: String },
    #[serde(rename = "zoom_to_fit")]
    ZoomToFit { viewport_idx: usize },
    #[serde(rename = "shutdown")]
    Shutdown,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(tag = "type")]
pub enum WcpCSMessage {
    #[serde(rename = "greeting")]
    Greeting {
        version: String,
        commands: Vec<String>,
    },
    #[serde(rename = "command")]
    Command(WcpCommand),
}

impl WcpCSMessage {
    pub fn create_greeting(version: usize, commands: Vec<String>) -> Self {
        Self::Greeting {
            version: version.to_string(),
            commands,
        }
    }
}
