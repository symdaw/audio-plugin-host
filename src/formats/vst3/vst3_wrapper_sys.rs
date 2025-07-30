use std::{
    ffi::{c_char, c_void},
    path::Path,
};

use ringbuf::{traits::Producer};

use crate::{
    audio_bus::IOConfigutaion, event::{HostIssuedEvent, PluginIssuedEvent}, formats::{vst3::Vst3, Format, PluginDescriptor}, heapless_vec::HeaplessVec, parameter::Parameter, track::Track, ProcessDetails
};

#[link(name = "vst3wrapper", kind = "static")]
extern "C" {
    pub(super) fn load_plugin(path: *const c_char, id: *const c_char, vst3_instance: *const c_void) -> *const c_void;
    pub(super) fn show_gui(app: *const c_void, window_id: *const c_void) -> Dims;
    pub(super) fn hide_gui(app: *const c_void);
    pub(super) fn descriptor(app: *const c_void) -> FFIPluginDescriptor;
    pub(super) fn io_config(app: *const c_void) -> IOConfigutaion;
    pub(super) fn parameter_count(app: *const c_void) -> usize;
    pub(super) fn process(
        app: *const c_void,
        data: *const ProcessDetails,
        input: *mut *mut *mut f32,
        output: *mut *mut *mut f32,
        events: *mut HostIssuedEvent,
        events_len: i32,
    );
    pub(super) fn set_param_in_edit_controller(app: *const c_void, id: i32, value: f32);
    pub(super) fn get_parameter(app: *const c_void, id: i32) -> Parameter;

    pub(super) fn get_data(
        app: *const c_void,
        data_len: *mut i32,
        stream: *mut *const c_void,
    ) -> *const c_void;
    pub(super) fn get_controller_data(
        app: *const c_void,
        data_len: *mut i32,
        stream: *mut *const c_void,
    ) -> *const c_void;
    pub(super) fn free_data_stream(stream: *const c_void);
    pub(super) fn set_data(app: *const c_void, data: *const c_void, data_len: i32);
    pub(super) fn set_controller_data(app: *const c_void, data: *const c_void, data_len: i32);
    pub(super) fn set_processing(app: *const c_void, processing: bool);
    pub(super) fn get_latency(app: *const c_void) -> u32;
    pub(super) fn get_descriptors(path: *const c_char, plugins: *mut HeaplessVec<FFIPluginDescriptor, 10>);

    pub(super) fn vst3_set_sample_rate(app: *const c_void, sample_rate: i32);
    pub(super) fn set_track_details(app: *const c_void, details: *const Track);

    fn free_string(str: *const c_char);
}

#[no_mangle]
pub extern "C" fn send_event_to_host(
    event: *const PluginIssuedEvent,
    vst3_instance: *const c_void,
) {
    let event = unsafe { &*event };
    let vst3 = unsafe { &mut *(vst3_instance as *mut Vst3) };

    if let PluginIssuedEvent::Parameter(p) = event {
        let _ = vst3.param_updates_for_audio_processor.try_push(p.clone());
    }

    let _ = vst3._plugin_issued_events_producer.try_push(event.clone());
}

#[repr(C)]
#[allow(non_snake_case)]
#[derive(Debug, Copy, Clone)]
pub struct FFIPluginDescriptor {
    name: *const std::os::raw::c_char,
    vendor: *const std::os::raw::c_char,
    version: *const std::os::raw::c_char,
    id: *const std::os::raw::c_char,
    initial_latency: std::os::raw::c_int,
}

impl FFIPluginDescriptor {
    pub fn to_plugin_descriptor(self, plugin_path: &Path) -> PluginDescriptor {
        PluginDescriptor {
            name: load_and_free_c_string(self.name),
            vendor: load_and_free_c_string(self.vendor),
            version: load_and_free_c_string(self.version),
            id: load_and_free_c_string(self.id),
            initial_latency: self.initial_latency as usize,
            path: plugin_path.to_path_buf(),
            format: Format::Vst3,
        }
    }
}

#[repr(C)]
#[allow(non_snake_case)]
#[derive(Debug, Copy, Clone)]
pub(super) struct Dims {
    pub width: std::os::raw::c_int,
    pub height: std::os::raw::c_int,
}

fn load_and_free_c_string(s: *const c_char) -> String {
    if s.is_null() {
        return "?".to_string();
    }

    let c_str = unsafe { std::ffi::CStr::from_ptr(s) };
    let str = c_str.to_string_lossy().into_owned();
    unsafe { free_string(s) };
    str
}
