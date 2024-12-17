use num::BigInt;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(transparent)]
pub struct DisplayedItemRef(pub usize);

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct ItemInfo {
    pub name: String,
    #[serde(rename = "type")]
    pub t: String,
    pub id: usize,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(tag = "command")]
#[allow(non_camel_case_types)]
pub enum WcpResponse {
    get_item_list(Vec<String>),
    get_item_info(Vec<ItemInfo>),
    add_variables(Vec<DisplayedItemRef>),
    // FIXME: Looks like this adds multiple scopes
    add_scope(Vec<DisplayedItemRef>),
    ack,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(tag = "event")]
pub enum WcpEvent {
    #[allow(non_camel_case_types)]
    waveforms_loaded,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(tag = "type")]
#[allow(non_camel_case_types)]
pub enum WcpSCMessage {
    greeting {
        version: String,
        commands: Vec<String>,
    },
    response(WcpResponse),
    error {
        error: String,
        arguments: Vec<String>,
        message: String,
    },
    event(WcpEvent),
}

impl WcpSCMessage {
    pub fn create_greeting(version: usize, commands: Vec<String>) -> Self {
        Self::greeting {
            version: version.to_string(),
            commands,
        }
    }

    pub fn create_error(error: String, arguments: Vec<String>, message: String) -> Self {
        Self::error {
            error,
            arguments,
            message,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(tag = "command")]
#[allow(non_camel_case_types)]
pub enum WcpCommand {
    /// Responds with [WcpResponse::get_item_list] which contains a list of the items
    /// in the currently loaded waveforms
    get_item_list,
    /// Responds with [WcpResponse::get_item_info] which contains information about
    /// each item specified in `ids` in the same order as in the `ids` array.
    /// Responds with an error if any of the specified IDs are not items in the currently loaded
    /// waveform.
    // TODO: This should actually respond with an error in that case
    // TODO: Use DisplayedItemRef here
    get_item_info { ids: Vec<String> },
    /// Changes the color of the specified item to the specified color.
    /// Responds with [WcpResponse::ack]
    /// Responds with an error if the `id` does not exist in the currently loaded waveform.
    set_item_color { id: DisplayedItemRef, color: String },
    /// Adds the specified variables to the view.
    /// Responds with [WcpResponse::add_variables] which contains a list of the item references
    /// that can be used to reference the added items later
    /// Responds with an error if no waveforms are loaded
    add_variables { names: Vec<String> },
    /// Adds all variables in the specified scope to the view.
    /// Responds with [WcpResponse::add_variables] which contains a list of the item references
    /// that can be used to reference the added items later
    /// Responds with an error if no waveforms are loaded
    add_scope { scope: String },
    /// Reloads the waveform from disk if this is possible for the current waveform format.
    /// If it is not possible, this has no effect.
    /// Responds instantly with [WcpResponse::ack]
    /// Once the waveforms have been loaded, a separate event is triggered
    reload,
    /// Moves the viewport to center it on the specified timestamp. Does not affect the zoom
    /// level.
    /// Responds with [WcpResponse::ack]
    set_viewport_to { timestamp: BigInt },
    /// Removes the specified items from the view.
    /// Responds with [WcpResponse::ack]
    /// Does not error if some of the IDs do not exist
    remove_items { ids: Vec<DisplayedItemRef> },
    /// Sets the specified ID as the _focused_ item.
    /// Responds with [WcpResponse::ack]
    /// Responds with an error if no waveforms are loaded or if the item reference
    /// does not exist
    // FIXME: What does this mean in the context of the protocol in general, feels kind
    // of like a Surfer specific thing. Do we have a use case for it
    focus_item { id: DisplayedItemRef },
    /// Removes all currently displayed items
    /// Responds with [WcpResponse::ack]
    clear,
    /// Loads a waveform from the specified file.
    /// Responds instantly with [WcpResponse::ack]
    /// Once the file is loaded, a [WcpEvent::waveform_loaded] is emitted.
    load { source: String },
    /// Zooms out fully to fit the whole waveform in the view
    /// Responds instantly with [WcpResponse::ack]
    zoom_to_fit { viewport_idx: usize },
    /// Shut down the WCP server.
    // TODO: What does this mean? Does it kill the server, the current connection or surfer itself?
    shutdowmn,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(tag = "type")]
#[allow(non_camel_case_types)]
pub enum WcpCSMessage {
    #[serde(rename = "greeting")]
    greeting {
        version: String,
        commands: Vec<String>,
    },
    command(WcpCommand),
}

impl WcpCSMessage {
    pub fn create_greeting(version: usize, commands: Vec<String>) -> Self {
        Self::greeting {
            version: version.to_string(),
            commands,
        }
    }
}
