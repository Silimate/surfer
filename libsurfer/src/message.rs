use bytes::Bytes;
use camino::Utf8PathBuf;
use derive_more::Debug;
use egui::DroppedFile;
use emath::{Pos2, Vec2};
use ftr_parser::types::Transaction;
use num::BigInt;
use serde::Deserialize;
use std::path::PathBuf;
use surver::Status;

use crate::async_util::AsyncJob;
use crate::displayed_item_tree::{ItemIndex, VisibleItemIndex};
use crate::graphics::{Graphic, GraphicId};
use crate::state::UserState;
use crate::transaction_container::{
    StreamScopeRef, TransactionContainer, TransactionRef, TransactionStreamRef,
};
use crate::translation::DynTranslator;
use crate::viewport::ViewportStrategy;
use crate::wave_data::ScopeType;
use crate::{
    clock_highlighting::ClockHighlightType,
    config::ArrowKeyBindings,
    dialog::{OpenSiblingStateFileDialog, ReloadWaveformDialog},
    displayed_item::{DisplayedFieldRef, DisplayedItemRef},
    file_dialog::OpenMode,
    hierarchy::HierarchyStyle,
    time::{TimeStringFormatting, TimeUnit},
    variable_filter::VariableIOFilterType,
    variable_name_type::VariableNameType,
    wave_container::{ScopeRef, VariableRef, WaveContainer},
    wave_source::{CxxrtlKind, LoadOptions, WaveFormat},
    wellen::{BodyResult, HeaderResult, LoadSignalsResult},
    MoveDir, VariableNameFilterType, WaveSource,
};

type CommandCount = usize;

/// Encapsulates either a specific variable or all selected variables
#[derive(Debug, Deserialize, Clone)]
pub enum MessageTarget<T> {
    Explicit(T),
    CurrentSelection,
}

impl<T> From<MessageTarget<T>> for Option<T> {
    fn from(value: MessageTarget<T>) -> Self {
        match value {
            MessageTarget::Explicit(val) => Some(val),
            MessageTarget::CurrentSelection => None,
        }
    }
}

impl<T> From<Option<T>> for MessageTarget<T> {
    fn from(value: Option<T>) -> Self {
        match value {
            Some(val) => Self::Explicit(val),
            None => Self::CurrentSelection,
        }
    }
}

impl<T: Copy> Copy for MessageTarget<T> {}

#[derive(Debug, Deserialize)]
/// The design of Surfer relies on sending messages to trigger actions.
pub enum Message {
    /// Set active scope
    SetActiveScope(ScopeType),
    /// Add one or more variables to wave view.
    AddVariables(Vec<VariableRef>),
    /// Add scope to wave view. If second argument is true, add subscopes recursively.
    AddScope(ScopeRef, bool),
    /// Add a character to the repeat command counter.
    AddCount(char),
    AddStreamOrGenerator(TransactionStreamRef),
    AddStreamOrGeneratorFromName(Option<StreamScopeRef>, String),
    AddAllFromStreamScope(String),
    /// Reset the repeat command counter.
    InvalidateCount,
    RemoveItemByIndex(VisibleItemIndex),
    RemoveItems(Vec<DisplayedItemRef>),
    /// Focus a wave/item.
    FocusItem(VisibleItemIndex),
    ItemSelectRange(VisibleItemIndex),
    /// Select all waves/items.
    ItemSelectAll,
    /// Unfocus a wave/item.
    UnfocusItem,
    RenameItem(Option<VisibleItemIndex>),
    MoveFocus(MoveDir, CommandCount, bool),
    MoveFocusedItem(MoveDir, CommandCount),
    FocusTransaction(Option<TransactionRef>, Option<Transaction>),
    VerticalScroll(MoveDir, CommandCount),
    /// Scroll in vertical direction so that the item at a given location in the list is at the top (or visible).
    ScrollToItem(usize),
    SetScrollOffset(f32),
    /// Change format (translator) of a variable. Passing None as first element means all selected variables.
    VariableFormatChange(MessageTarget<DisplayedFieldRef>, String),
    ItemSelectionClear,
    /// Change color of waves/items. If first argument is None, change for selected items. If second argument is None, change to default value.
    ItemColorChange(MessageTarget<VisibleItemIndex>, Option<String>),
    /// Change background color of waves/items. If first argument is None, change for selected items. If second argument is None, change to default value.
    ItemBackgroundColorChange(MessageTarget<VisibleItemIndex>, Option<String>),
    ItemNameChange(Option<VisibleItemIndex>, Option<String>),
    /// Change scaling factor/height of waves/items. If first argument is None, change for selected items.
    ItemHeightScalingFactorChange(MessageTarget<VisibleItemIndex>, f32),
    /// Change variable name type of waves/items. If first argument is None, change for selected items.
    ChangeVariableNameType(MessageTarget<VisibleItemIndex>, VariableNameType),
    ForceVariableNameTypes(VariableNameType),
    /// Set or unset right alignment of names
    SetNameAlignRight(bool),
    SetClockHighlightType(ClockHighlightType),
    SetFillHighValues(bool),
    // Reset the translator for this variable back to default. Sub-variables,
    // i.e. those with the variable idx and a shared path are also reset
    ResetVariableFormat(DisplayedFieldRef),
    CanvasScroll {
        delta: Vec2,
        viewport_idx: usize,
    },
    CanvasZoom {
        mouse_ptr: Option<BigInt>,
        delta: f32,
        viewport_idx: usize,
    },
    ZoomToRange {
        start: BigInt,
        end: BigInt,
        viewport_idx: usize,
    },
    /// Set cursor at time.
    CursorSet(BigInt),
    #[serde(skip)]
    SurferServerStatus(web_time::Instant, String, Status),
    /// Load file from file path.
    LoadFile(Utf8PathBuf, LoadOptions),
    /// Load file from URL.
    LoadWaveformFileFromUrl(String, LoadOptions),
    /// Load file from data.
    LoadFromData(Vec<u8>, LoadOptions),
    #[cfg(feature = "python")]
    /// Load translator from Python file path.
    LoadPythonTranslator(Utf8PathBuf),
    /// Load a spade translator using the specified top and the specified state encoded as ron.
    LoadSpadeTranslator {
        top: String,
        #[debug(skip)]
        state: String,
    },
    SetupCxxrtl(CxxrtlKind),
    #[serde(skip)]
    /// Message sent when waveform file header is loaded.
    WaveHeaderLoaded(
        web_time::Instant,
        WaveSource,
        LoadOptions,
        #[debug(skip)] HeaderResult,
    ),
    #[serde(skip)]
    /// Message sent when waveform file body is loaded.
    WaveBodyLoaded(web_time::Instant, WaveSource, #[debug(skip)] BodyResult),
    #[serde(skip)]
    WavesLoaded(
        WaveSource,
        WaveFormat,
        #[debug(skip)] Box<WaveContainer>,
        LoadOptions,
    ),
    #[serde(skip)]
    SignalsLoaded(web_time::Instant, #[debug(skip)] LoadSignalsResult),
    #[serde(skip)]
    TransactionStreamsLoaded(
        WaveSource,
        WaveFormat,
        #[debug(skip)] TransactionContainer,
        LoadOptions,
    ),
    #[serde(skip)]
    Error(color_eyre::eyre::Error),
    #[serde(skip)]
    TranslatorLoaded(#[debug(skip)] Box<DynTranslator>),
    /// Take note that the specified translator errored on a `translates` call on the
    /// specified variable
    BlacklistTranslator(VariableRef, String),
    ShowCommandPrompt(Option<String>),
    /// Message sent when file is loadedropped onto Surfer.
    FileDropped(DroppedFile),
    #[serde(skip)]
    /// Message sent when download of a waveform file is complete.
    FileDownloaded(String, Bytes, LoadOptions),
    ReloadConfig,
    ReloadWaveform(bool),
    /// Suggest reloading the current waveform as the file on disk has changed.
    /// This should first take the user's confirmation before reloading the waveform.
    /// However, there is a configuration setting that the user can overwrite.
    #[serde(skip)]
    SuggestReloadWaveform,
    /// Close the 'reload_waveform' dialog.
    /// The `reload_file` boolean is the return value of the dialog.
    /// If `do_not_show_again` is true, the `reload_file` setting will be persisted.
    #[serde(skip)]
    CloseReloadWaveformDialog {
        reload_file: bool,
        do_not_show_again: bool,
    },
    /// Update the waveform dialog UI with the provided dialog model.
    #[serde(skip)]
    UpdateReloadWaveformDialog(ReloadWaveformDialog),
    // When a file is open, suggest opening state files in the same directory
    OpenSiblingStateFile(bool),
    #[serde(skip)]
    SuggestOpenSiblingStateFile,
    #[serde(skip)]
    CloseOpenSiblingStateFileDialog {
        load_state: bool,
        do_not_show_again: bool,
    },
    #[serde(skip)]
    UpdateOpenSiblingStateFileDialog(OpenSiblingStateFileDialog),
    RemovePlaceholders,
    ZoomToFit {
        viewport_idx: usize,
    },
    GoToStart {
        viewport_idx: usize,
    },
    GoToEnd {
        viewport_idx: usize,
    },
    GoToTime(Option<BigInt>, usize),
    ToggleMenu,
    ToggleToolbar,
    ToggleOverview,
    ToggleStatusbar,
    ToggleIndices,
    ToggleDirection,
    ToggleEmptyScopes,
    ToggleParametersInScopes,
    ToggleSidePanel,
    ToggleItemSelected(Option<VisibleItemIndex>),
    ToggleDefaultTimeline,
    ToggleTickLines,
    ToggleVariableTooltip,
    ToggleScopeTooltip,
    ToggleFullscreen,
    /// Set which time unit to use.
    SetTimeUnit(TimeUnit),
    /// Set how to format the time strings. Passing None resets it to default.
    SetTimeStringFormatting(Option<TimeStringFormatting>),
    SetHighlightFocused(bool),
    CommandPromptClear,
    CommandPromptUpdate {
        suggestions: Vec<(String, Vec<bool>)>,
    },
    CommandPromptPushPrevious(String),
    SelectPrevCommand,
    SelectNextCommand,
    OpenFileDialog(OpenMode),
    #[cfg(feature = "python")]
    OpenPythonPluginDialog,
    #[cfg(feature = "python")]
    ReloadPythonPlugin,
    SaveStateFile(Option<PathBuf>),
    LoadStateFile(Option<PathBuf>),
    LoadState(UserState, Option<PathBuf>),
    SetStateFile(PathBuf),
    SetAboutVisible(bool),
    SetKeyHelpVisible(bool),
    SetGestureHelpVisible(bool),
    SetQuickStartVisible(bool),
    SetUrlEntryVisible(bool),
    SetLicenseVisible(bool),
    SetRenameItemVisible(bool),
    SetLogsVisible(bool),
    SetMouseGestureDragStart(Option<Pos2>),
    SetMeasureDragStart(Option<Pos2>),
    SetFilterFocused(bool),
    SetVariableNameFilterType(VariableNameFilterType),
    SetVariableNameFilterCaseInsensitive(bool),
    SetVariableIOFilter(VariableIOFilterType, bool),
    SetVariableGroupByDirection(bool),
    SetUIZoomFactor(f32),
    SetPerformanceVisible(bool),
    SetContinuousRedraw(bool),
    SetCursorWindowVisible(bool),
    SetHierarchyStyle(HierarchyStyle),
    SetArrowKeyBindings(ArrowKeyBindings),
    // Second argument is position to insert after, None inserts after focused item,
    // or last if no focused item
    AddDivider(Option<String>, Option<VisibleItemIndex>),
    // Argument is position to insert after, None inserts after focused item,
    // or last if no focused item
    AddTimeLine(Option<VisibleItemIndex>),
    AddMarker {
        time: BigInt,
        name: Option<String>,
        move_focus: bool,
    },
    /// Set a marker at a specific position. If it doesn't exist, it will be created
    SetMarker {
        id: u8,
        time: BigInt,
    },
    /// Remove marker.
    RemoveMarker(u8),
    /// Set or move a marker to the position of the current cursor.
    MoveMarkerToCursor(u8),
    /// Scroll in horizontal direction so that the cursor is visible.
    GoToCursorIfNotInView,
    GoToMarkerPosition(u8, usize),
    MoveCursorToTransition {
        next: bool,
        variable: Option<VisibleItemIndex>,
        skip_zero: bool,
    },
    MoveTransaction {
        next: bool,
    },
    VariableValueToClipbord(MessageTarget<VisibleItemIndex>),
    VariableNameToClipboard(MessageTarget<VisibleItemIndex>),
    VariableFullNameToClipboard(MessageTarget<VisibleItemIndex>),
    InvalidateDrawCommands,
    AddGraphic(GraphicId, Graphic),
    RemoveGraphic(GraphicId),

    /// Variable dragging messages
    VariableDragStarted(VisibleItemIndex),
    VariableDragTargetChanged(crate::displayed_item_tree::TargetPosition),
    VariableDragFinished,
    AddDraggedVariables(Vec<VariableRef>),
    /// Unpauses the simulation if the wave source supports this kind of interactivity. Otherwise
    /// does nothing
    UnpauseSimulation,
    /// Pause the simulation if the wave source supports this kind of interactivity. Otherwise
    /// does nothing
    PauseSimulation,
    /// Expand the displayed item into subfields. Levels controls how many layers of subfields
    /// are expanded. 0 unexpands it completely
    ExpandDrawnItem {
        item: DisplayedItemRef,
        levels: usize,
    },

    SetViewportStrategy(ViewportStrategy),
    SetConfigFromString(String),
    AddCharToPrompt(char),

    /// Run more than one message in sequence
    Batch(Vec<Message>),
    AddViewport,
    RemoveViewport,
    /// Select Theme
    SelectTheme(Option<String>),
    /// Undo the last n changes
    Undo(usize),
    /// Redo the last n changes
    Redo(usize),
    DumpTree,
    GroupNew {
        name: Option<String>,
        before: Option<ItemIndex>,
        items: Option<Vec<DisplayedItemRef>>,
    },
    GroupDissolve(Option<DisplayedItemRef>),
    GroupFold(Option<DisplayedItemRef>),
    GroupUnfold(Option<DisplayedItemRef>),
    GroupFoldRecursive(Option<DisplayedItemRef>),
    GroupUnfoldRecursive(Option<DisplayedItemRef>),
    GroupFoldAll,
    GroupUnfoldAll,
    /// WCP Server
    StartWcpServer {
        address: Option<String>,
        initiate: bool,
    },
    StopWcpServer,
    /// Configures the WCP system to listen for messages over internal channels.
    /// This is used to start WCP on wasm
    SetupChannelWCP,
    /// Exit the application. This has no effect on wasm and closes the window
    /// on other platforms
    Exit,
    /// Should only used for tests. Expands the parameter section so that one can test the rendering.
    ExpandParameterSection,
    AsyncDone(AsyncJob),
}
