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

use crate::displayed_item_tree::{ItemIndex, VisibleItemIndex};
use crate::graphics::{Graphic, GraphicId};
use crate::transaction_container::{
    StreamScopeRef, TransactionContainer, TransactionRef, TransactionStreamRef,
};
use crate::translation::DynTranslator;
use crate::viewport::ViewportStrategy;
use crate::wave_data::ScopeType;
use crate::wave_source::CxxrtlKind;
use crate::{
    clock_highlighting::ClockHighlightType,
    config::ArrowKeyBindings,
    displayed_item::{DisplayedFieldRef, DisplayedItemRef},
    time::{TimeStringFormatting, TimeUnit},
    variable_name_type::VariableNameType,
    wave_container::{ScopeRef, VariableRef, WaveContainer},
    wave_source::{LoadOptions, OpenMode},
    wellen::LoadSignalsResult,
    MoveDir, ReloadWaveformDialog, VariableNameFilterType, WaveSource,
};
use crate::{config::HierarchyStyle, wave_source::WaveFormat};

type CommandCount = usize;

pub enum HeaderResult {
    /// result of locally parsing the header of a waveform file with wellen from a file
    LocalFile(Box<wellen::viewers::HeaderResult<std::io::BufReader<std::fs::File>>>),
    /// result of locally parsing the header of a waveform file with wellen from bytes
    LocalBytes(Box<wellen::viewers::HeaderResult<std::io::Cursor<Vec<u8>>>>),
    /// result of querying a remote surfer server
    Remote(
        std::sync::Arc<wellen::Hierarchy>,
        wellen::FileFormat,
        String,
    ),
}

pub enum BodyResult {
    /// result of locally parsing the body of a waveform file with wellen
    Local(wellen::viewers::BodyResult),
    /// result of querying a remote surfer server
    Remote(Vec<wellen::Time>, String),
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub enum AsyncJob {
    SaveState,
}

#[derive(Debug, Deserialize)]
pub enum Message {
    SetActiveScope(ScopeType),
    AddVariables(Vec<VariableRef>),
    AddScope(ScopeRef, bool),
    AddCount(char),
    AddStreamOrGenerator(TransactionStreamRef),
    AddStreamOrGeneratorFromName(Option<StreamScopeRef>, String),
    AddAllFromStreamScope(String),
    InvalidateCount,
    RemoveItemByIndex(VisibleItemIndex),
    RemoveItems(Vec<DisplayedItemRef>),
    FocusItem(VisibleItemIndex),
    ItemSelectRange(VisibleItemIndex),
    ItemSelectAll,
    UnfocusItem,
    RenameItem(Option<VisibleItemIndex>),
    MoveFocus(MoveDir, CommandCount, bool),
    MoveFocusedItem(MoveDir, CommandCount),
    FocusTransaction(Option<TransactionRef>, Option<Transaction>),
    VerticalScroll(MoveDir, CommandCount),
    ScrollToItem(usize),
    SetScrollOffset(f32),
    VariableFormatChange(Option<DisplayedFieldRef>, String),
    ItemSelectionClear,
    ItemColorChange(Option<VisibleItemIndex>, Option<String>),
    ItemBackgroundColorChange(Option<VisibleItemIndex>, Option<String>),
    ItemNameChange(Option<VisibleItemIndex>, Option<String>),
    ChangeVariableNameType(Option<VisibleItemIndex>, VariableNameType),
    ForceVariableNameTypes(VariableNameType),
    SetNameAlignRight(bool),
    SetClockHighlightType(ClockHighlightType),
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
    CursorSet(BigInt),
    #[serde(skip)]
    SurferServerStatus(web_time::Instant, String, Status),
    LoadFile(Utf8PathBuf, LoadOptions),
    LoadWaveformFileFromUrl(String, LoadOptions),
    LoadFromData(Vec<u8>, LoadOptions),
    #[cfg(feature = "python")]
    LoadPythonTranslator(Utf8PathBuf),
    /// Load a spade translator using the specified top and the specified state encoded as ron.
    LoadSpadeTranslator {
        top: String,
        #[debug(skip)]
        state: String,
    },
    SetupCxxrtl(CxxrtlKind),
    #[serde(skip)]
    WaveHeaderLoaded(
        web_time::Instant,
        WaveSource,
        LoadOptions,
        #[debug(skip)] HeaderResult,
    ),
    #[serde(skip)]
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
    FileDropped(DroppedFile),
    #[serde(skip)]
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
    SetTimeUnit(TimeUnit),
    SetTimeStringFormatting(Option<TimeStringFormatting>),
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
    LoadState(crate::State, Option<PathBuf>),
    SetStateFile(PathBuf),
    SetAboutVisible(bool),
    SetKeyHelpVisible(bool),
    SetGestureHelpVisible(bool),
    SetQuickStartVisible(bool),
    SetUrlEntryVisible(bool),
    SetLicenseVisible(bool),
    SetRenameItemVisible(bool),
    SetLogsVisible(bool),
    SetDragStart(Option<Pos2>),
    SetFilterFocused(bool),
    SetVariableNameFilterType(VariableNameFilterType),
    SetVariableNameFilterCaseInsensitive(bool),
    SetUIZoomFactor(f32),
    SetPerformanceVisible(bool),
    SetContinuousRedraw(bool),
    SetCursorWindowVisible(bool),
    ToggleFullscreen,
    SetHierarchyStyle(HierarchyStyle),
    SetArrowKeyBindings(ArrowKeyBindings),
    // Second argument is position to insert after, None inserts after focused item,
    // or last if no focused item
    AddDivider(Option<String>, Option<VisibleItemIndex>),
    // Argument is position to insert after, None inserts after focused item,
    // or last if no focused item
    AddTimeLine(Option<VisibleItemIndex>),
    ToggleTickLines,
    ToggleVariableTooltip,
    ToggleScopeTooltip,
    AddMarker {
        time: BigInt,
        name: Option<String>,
    },
    /// Set a marker at a specific position. If it doesn't exist, it will be created
    SetMarker {
        id: u8,
        time: BigInt,
    },
    RemoveMarker(u8),
    MoveMarkerToCursor(u8),
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
    VariableValueToClipbord(Option<VisibleItemIndex>),
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
    AsyncDone(AsyncJob),
}
