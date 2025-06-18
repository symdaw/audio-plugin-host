#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Parameter {
    pub id: i32,
    pub name: String,
    pub index: i32,
    /// Normalized parameter value in [0.0, 1.0].
    pub value: f32,
    /// Value as string formatted by the plugin. E.g. "0 dB", "50 Hz", etc.
    pub formatted_value: String,
    pub hidden: bool,
    pub can_automate: bool,
    pub is_wrap_around: bool,
    pub read_only: bool,
    /// Default normalized value if supported by the format. Not supported by VST2.
    pub default_value: Option<f32>,
}

#[derive(Debug, Clone)]
#[repr(C)]
pub struct ParameterUpdate {
    pub parameter_id: i32,
    pub parameter_index: i32,
    pub current_value: f32,
    /// Value at start of edit. For example, the value before the user started dragging a knob
    /// in the plugin editor. Not required to be set when sending events to the plugin; just
    /// used for implementing undo/redo in the host.
    pub initial_value: f32,
    ///  If `true`, the user has just released the control and this is the final value.
    pub end_edit: bool,
}

impl ParameterUpdate {
    pub fn new(id: i32, value: f32) -> Self {
        ParameterUpdate {
            parameter_id: id,
            parameter_index: -1,
            current_value: value,
            initial_value: f32::NAN,
            end_edit: false,
        }
    }
}

#[derive(Clone, Copy)]
#[repr(C)]
pub union ParameterIdentifier {
    raw: u64,
    vst2: i32,
    vst3: IndexIdPair,
}

impl ParameterIdentifier {
    pub fn raw(&self) -> u64 {
        unsafe { self.raw }
    }

    pub fn as_vst2(&self) -> i32 {
        unsafe { self.vst2 }
    }

    pub fn as_vst3(&self) -> IndexIdPair {
        unsafe { self.vst3 }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct IndexIdPair {
    pub index: i32,
    pub id: i32,
}
