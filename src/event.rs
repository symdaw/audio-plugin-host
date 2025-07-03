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
    pub is_live: bool,
    pub dont_record: bool,
}

#[repr(C)]
#[derive(Debug, Clone)]
pub enum HostIssuedEventType {
    Midi(MidiEvent),
    Parameter(ParameterUpdate),
}

impl Default for HostIssuedEventType {
    fn default() -> Self {
        HostIssuedEventType::Midi(MidiEvent::default())
    }
}

#[repr(C)]
#[derive(Debug, Clone, Default)]
pub struct MidiEvent {
    pub note_length: Samples,
    pub midi_data: [u8; 3],
    pub detune: f32,
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
