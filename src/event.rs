use crate::{parameter::ParameterUpdate, PpqTime, Samples};

/// Events sent to the plugin from the host. Can be passed into the `process` function or queued
/// for the next process call with `queue_event`.
#[derive(Debug, Clone, Default)]
#[repr(C)]
pub struct HostIssuedEvent {
    pub event_type: HostIssuedEventType,
    /// Time in samples from start of next block.
    pub block_time: Samples,
    pub ppq_time: PpqTime,
    pub bus_index: usize,
    /// Whether event was issued from a live input (e.g. MIDI controller).
    pub is_live: bool,
}

#[repr(C)]
#[derive(Debug, Clone)]
pub enum HostIssuedEventType {
    Midi(MidiEvent),
    Parameter(ParameterUpdate),
    NoteExpression {
        note_id: i32,
        expression_type: NoteExpressionType,
        value: f64,
    },
}

#[repr(C)]
#[derive(Debug, Clone)]
pub enum NoteExpressionType {
    Volume,
    Pan,
    Tuning,
    Vibrato,
    Expression,
    Brightness,
}

impl Default for HostIssuedEventType {
    fn default() -> Self {
        HostIssuedEventType::Midi(MidiEvent::default())
    }
}

// TODO: Refactor into enum to remove unnecessary fields
#[repr(C)]
#[derive(Debug, Clone, Default)]
pub struct MidiEvent {
    pub note_length: Samples,
    pub midi_data: [u8; 3],
    pub detune: f32,
    pub note_id: i32,
}

/// Events sent to the host from the plugin. Queued in the plugin and the consumed from the `get_events` function.
#[derive(Debug, Clone)]
#[repr(C)]
pub enum PluginIssuedEvent {
    /// Plugin changed it's latency. New latency is in samples.
    ChangeLatency(usize),
    /// Plugin changed its editor window size. 0 is width, 1 is height.
    ResizeWindow(usize, usize),
    Parameter(ParameterUpdate),
    // AddNoteLabels(HeaplessVec<NoteLabel, 128>),
    // RemoveNoteLabels(HeaplessVec<u32, 128>),
    UpdateDisplay,
    IOChanged,
    RequestEditorOpen,
    RequestEditorClose,
    /// Tail length in samples. This is how long the plugin will continue to produce audio after
    /// the last input sample (i.e. reverb tail).
    TailLengthChanged(usize),
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct NoteLabel {
    note: u8,
    label: *const std::ffi::c_char,
}

unsafe impl Send for NoteLabel {}
unsafe impl Sync for NoteLabel {}

impl NoteLabel {
    pub fn consume(self) -> (u8, String) {
        let label = unsafe { std::ffi::CStr::from_ptr(self.label) }
            .to_string_lossy()
            .into_owned();

        // TODO: Free label

        (self.note, label)
    }
}
