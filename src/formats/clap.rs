// This is only partially implemented

use std::ffi::{c_char, c_void, CStr};
use std::mem::zeroed;
use std::path::Path;

use clap_sys::audio_buffer::clap_audio_buffer;
use clap_sys::entry::clap_plugin_entry;
use clap_sys::events::*;
use clap_sys::ext::audio_ports::*;
use clap_sys::ext::audio_ports_config::*;
use clap_sys::ext::gui::*;
use clap_sys::ext::latency::*;
use clap_sys::ext::log::*;
use clap_sys::ext::params::*;
use clap_sys::ext::thread_check::{clap_host_thread_check, CLAP_EXT_THREAD_CHECK};
use clap_sys::ext::thread_pool::{clap_host_thread_pool, CLAP_EXT_THREAD_POOL};
use clap_sys::factory::plugin_factory::{clap_plugin_factory, CLAP_PLUGIN_FACTORY_ID};
use clap_sys::host::*;
use clap_sys::plugin::*;
use clap_sys::process::*;
use clap_sys::version::{clap_version_is_compatible, CLAP_VERSION};

use ringbuf::traits::Producer;
use ringbuf::HeapProd;

use crate::audio_bus::AudioBusDescriptor;
use crate::discovery::PluginDescriptor;
use crate::error::Error;
use crate::event::{HostIssuedEvent, PluginIssuedEvent};
use crate::formats::Common;
use crate::heapless_vec::{HeaplessString, HeaplessVec};
use crate::plugin::PluginInner;
use crate::thread_check::{is_main_thread, is_thread_checking_enabled};
use crate::{BlockSize, SampleRate, WindowIDType};

struct Clap {
    lib: libloading::Library,
    entry: *const clap_plugin_entry,
    factory: *const clap_plugin_factory,
    plugin: *const clap_plugin,
    host: Option<Box<clap_host>>,
    host_data: Option<Box<HostData>>,
    process: clap_process,
    block_size: BlockSize,
    sample_rate: SampleRate,
    in_events: EventBuffer,
    out_events: EventBuffer,
}

type EventBuffer = HeaplessVec<ClapEvent, 64>;

#[derive(Clone, Copy)]
union ClapEvent {
    header: clap_event_header,
    note: clap_event_note,
    note_expression: clap_event_note_expression,
    param_value: clap_event_param_value,
    param_mod: clap_event_param_mod,
    param_gesture: clap_event_param_gesture,
    // Note: This one is kinda long. Maybe add a special case for it
    // transport: clap_event_transport,
    midi: clap_event_midi,
    midi_sysex: clap_event_midi_sysex,
    midi2: clap_event_midi2,
}

// Everything in this must be thread-safe or not mutated.
struct HostData {
    plugin_issued_events_producer: HeapProd<PluginIssuedEvent>,
}

pub fn load(
    path: &Path,
    id: &str,
    common: Common,
) -> Result<(Box<dyn PluginInner>, PluginDescriptor), Error> {
    unsafe {
        let mut plugin = Clap::load_factory(path).map_err(|e| Error {
            message: format!("Failed to load CLAP plugin factory: {}", e),
        })?;
        let descriptors = plugin.get_descriptors(path).map_err(|e| Error {
            message: format!("Failed to get CLAP plugin descriptors: {}", e),
        })?;

        for descriptor in descriptors.iter() {
            if descriptor.id == id {
                plugin.load_plugin(id, common).map_err(|e| Error {
                    message: format!("Failed to load CLAP plugin: {}", e),
                })?;

                return Ok((Box::new(plugin), descriptor.clone()));
            }
        }

        Err(Error {
            message: format!("No CLAP plugin found with ID: {}", id),
        })
    }
}

pub(crate) fn get_descriptor(path: &Path) -> Vec<PluginDescriptor> {
    unsafe {
        Clap::load_factory(path)
            .and_then(|p| p.get_descriptors(path))
            .unwrap_or(vec![])
    }
}

impl Clap {
    unsafe fn load_factory(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let lib = libloading::Library::new(path)?;
        let entry_symbol: libloading::Symbol<*const clap_plugin_entry> = lib.get(b"clap_entry")?;
        let entry = *entry_symbol;

        let path_cstr = std::ffi::CString::new(path.to_str().unwrap()).unwrap();

        (*entry).init.unwrap()(path_cstr.as_ptr());
        let factory = (*entry).get_factory.unwrap()(CLAP_PLUGIN_FACTORY_ID.as_ptr())
            as *const clap_plugin_factory;

        Ok(Clap {
            lib,
            factory,
            entry,
            host: None,
            host_data: None,
            plugin: std::ptr::null_mut(),
            process: zeroed(),
            block_size: 512,
            sample_rate: 44100,
            in_events: HeaplessVec::new(),
            out_events: HeaplessVec::new(),
        })
    }

    fn reactivate(&mut self) {
        unsafe {
            let plugin = &*self.plugin;

            plugin.deactivate.unwrap()(self.plugin);

            if !plugin.activate.unwrap()(
                self.plugin,
                self.sample_rate as f64,
                self.block_size as u32,
                self.block_size as u32,
            ) {
                eprintln!("Failed to reactivate CLAP plugin");
            }

            self.resume();
        }
    }

    unsafe fn load_plugin(
        &mut self,
        id: &str,
        common: Common,
    ) -> Result<(), Box<dyn std::error::Error>> {
        assert!(!self.entry.is_null(), "CLAP entry is not initialized");
        assert!(!self.factory.is_null(), "CLAP factory is not initialized");

        if !clap_version_is_compatible((*self.entry).clap_version) {
            return Err(Box::new(Error {
                message: "Incompatible CLAP version".to_string(),
            }));
        }

        let host_name = std::ffi::CString::new(common.host.name).unwrap();
        let host_vendor = std::ffi::CString::new(common.host.vendor).unwrap();
        let host_version = std::ffi::CString::new(common.host.version).unwrap();
        let host_url = std::ffi::CString::new(common.host.url).unwrap();

        let mut host_data = Box::new(HostData {
            plugin_issued_events_producer: common.plugin_issued_events_producer,
        });

        let clap_host_ = Box::new(clap_host {
            clap_version: CLAP_VERSION,
            host_data: &mut *host_data as *mut _ as *mut c_void,
            name: host_name.as_ptr(),
            vendor: host_vendor.as_ptr(),
            url: host_url.as_ptr(),
            version: host_version.as_ptr(),
            get_extension: Some(clap_callback_get_extension),
            request_restart: Some(clap_callback_request_restart),
            request_process: Some(clap_callback_request_process),
            request_callback: Some(clap_callback_request_callback),
        });

        let id_c_str = std::ffi::CString::new(id).unwrap();

        let plugin =
            (*self.factory).create_plugin.unwrap()(self.factory, &*clap_host_, id_c_str.as_ptr());

        if !(*plugin).init.unwrap()(plugin) {
            return Err(Box::new(Error {
                message: format!("Failed to initialize CLAP plugin with ID: {}", id),
            }));
        }

        self.host = Some(clap_host_);
        self.host_data = Some(host_data);

        self.plugin = plugin;

        self.reactivate();

        Ok(())
    }

    unsafe fn get_descriptors(
        &self,
        path: &Path,
    ) -> Result<Vec<PluginDescriptor>, Box<dyn std::error::Error>> {
        let count = (*self.factory).get_plugin_count.unwrap()(self.factory);

        let mut descriptors = Vec::with_capacity(count as usize);

        for i in 0..count {
            let desc = (*self.factory).get_plugin_descriptor.unwrap()(self.factory, i);
            descriptors.push(PluginDescriptor {
                name: std::ffi::CStr::from_ptr((*desc).name)
                    .to_string_lossy()
                    .into_owned(),
                id: std::ffi::CStr::from_ptr((*desc).id)
                    .to_string_lossy()
                    .into_owned(),
                version: std::ffi::CStr::from_ptr((*desc).version)
                    .to_string_lossy()
                    .into_owned(),
                vendor: std::ffi::CStr::from_ptr((*desc).vendor)
                    .to_string_lossy()
                    .into_owned(),
                path: path.to_path_buf(),
                format: crate::discovery::Format::Clap,
                initial_latency: 0,
            });
        }

        Ok(descriptors)
    }
}

fn access_host_data<'a>(host: &'a mut clap_host) -> &'a mut HostData {
    unsafe {
        let host_data_ptr = (*host).host_data as *mut HostData;
        &mut *host_data_ptr
    }
}

#[no_mangle]
pub unsafe extern "C" fn clap_callback_request_restart(host: *const clap_host) {}

#[no_mangle]
pub unsafe extern "C" fn clap_callback_get_extension(
    _host: *const clap_host,
    ext: *const c_char,
) -> *const c_void {
    if CStr::from_ptr(ext) == CLAP_EXT_GUI {
        static GUI: clap_host_gui = clap_host_gui {
            resize_hints_changed: Some(clap_callback_do_nothing),
            request_resize: Some(clap_callback_resize_window),
            request_show: Some(clap_callback_send_request_editor_open),
            request_hide: Some(clap_callback_send_request_editor_close),
            closed: Some(clap_callback_do_gui_closed),
        };

        return &GUI as *const _ as *const c_void;
    } else if CStr::from_ptr(ext) == CLAP_EXT_LATENCY {
        static LATENCY: clap_host_latency = clap_host_latency {
            changed: Some(clap_callback_send_io_changed),
        };

        return &LATENCY as *const _ as *const c_void;
    } else if CStr::from_ptr(ext) == CLAP_EXT_LOG {
        static LOG: clap_host_log = clap_host_log {
            log: Some(clap_callback_log),
        };

        return &LOG as *const _ as *const c_void;
    } else if CStr::from_ptr(ext) == CLAP_EXT_AUDIO_PORTS {
        // static PORTS: clap_host_audio_ports = clap_host_audio_ports { is_rescan_flag_supported: todo!(), rescan: todo!() };
        //
        // return &PORTS as *const _ as *const c_void;
    } else if CStr::from_ptr(ext) == CLAP_EXT_AUDIO_PORTS_CONFIG {
        static PORTS_CONFIG: clap_host_audio_ports_config = clap_host_audio_ports_config {
            rescan: Some(clap_callback_send_io_changed),
        };

        return &PORTS_CONFIG as *const _ as *const c_void;
    } else if CStr::from_ptr(ext) == CLAP_EXT_THREAD_POOL {
        static POOL: clap_host_thread_pool = clap_host_thread_pool {
            request_exec: Some(clap_callback_thread_pool_request_exec),
        };

        return &POOL as *const _ as *const c_void;
    } else if CStr::from_ptr(ext) == CLAP_EXT_THREAD_CHECK {
        if is_thread_checking_enabled() {
            static THREAD_CHECK: clap_host_thread_check = clap_host_thread_check {
                is_main_thread: Some(clap_callback_is_main_thread),
                is_audio_thread: Some(clap_callback_is_audio_thread),
            };

            return &THREAD_CHECK as *const _ as *const c_void;
        }

    }

    println!(
        "[CLAP] Unimplemented extension: {}",
        std::ffi::CStr::from_ptr(ext).to_string_lossy()
    );

    std::ptr::null()
}

#[no_mangle]
pub unsafe extern "C" fn clap_callback_request_process(host: *const clap_host) {}

#[no_mangle]
pub unsafe extern "C" fn clap_callback_request_callback(host: *const clap_host) {}

#[no_mangle]
pub unsafe extern "C" fn clap_callback_resize_window(
    host: *const clap_host,
    w: u32,
    h: u32,
) -> bool {
    access_host_data(&mut *(host as *mut _))
        .plugin_issued_events_producer
        .try_push(PluginIssuedEvent::ResizeWindow(w as usize, h as usize))
        .is_ok()
}

#[no_mangle]
pub unsafe extern "C" fn clap_callback_send_io_changed(host: *const clap_host) {
    let _ = access_host_data(&mut *(host as *mut _))
        .plugin_issued_events_producer
        .try_push(PluginIssuedEvent::IOChanged);
}

#[no_mangle]
pub unsafe extern "C" fn clap_callback_is_main_thread(_host: *const clap_host) -> bool {
    is_main_thread()
}

#[no_mangle]
pub unsafe extern "C" fn clap_callback_is_audio_thread(_host: *const clap_host) -> bool {
    !is_main_thread()
}

#[no_mangle]
pub unsafe extern "C" fn clap_callback_send_request_editor_open(host: *const clap_host) -> bool {
    let _ = access_host_data(&mut *(host as *mut _))
        .plugin_issued_events_producer
        .try_push(PluginIssuedEvent::RequestEditorOpen);

    // Note: The host may not actually handle this. There may need to be some kind of "can do" for
    //       hosts.
    true
}

#[no_mangle]
pub unsafe extern "C" fn clap_callback_send_request_editor_close(host: *const clap_host) -> bool {
    let _ = access_host_data(&mut *(host as *mut _))
        .plugin_issued_events_producer
        .try_push(PluginIssuedEvent::RequestEditorClose);

    // Note: The host may not actually handle this. There may need to be some kind of "can do" for
    //       hosts.
    true
}

#[no_mangle]
pub unsafe extern "C" fn clap_callback_do_nothing(_host: *const clap_host) {}

#[no_mangle]
pub unsafe extern "C" fn clap_callback_do_gui_closed(host: *const clap_host, closed: bool) {
    // TODO
}

#[no_mangle]
pub unsafe extern "C" fn clap_callback_events_try_push(
    list: *const clap_output_events,
    event: *const clap_event_header,
) -> bool {
    let buffer = (*list).ctx as *mut EventBuffer;

    let event_len = (*event).size as usize;

    let mut new_event: ClapEvent = zeroed();

    std::ptr::copy_nonoverlapping(
        event as *const u8,
        &mut new_event as *mut _ as *mut u8,
        event_len,
    );

    (*buffer).push(new_event).is_ok()
}

#[no_mangle]
pub unsafe extern "C" fn clap_callback_events_size(list: *const clap_input_events) -> u32 {
    let buffer = (*list).ctx as *const EventBuffer;
    (*buffer).len() as u32
}

#[no_mangle]
pub unsafe extern "C" fn clap_callback_events_get(
    list: *const clap_input_events,
    index: u32,
) -> *const clap_event_header {
    let buffer = (*list).ctx as *const EventBuffer;
    (*buffer)
        .as_slice()
        .get(index as usize)
        .map_or(std::ptr::null(), |event| {
            event as *const _ as *const clap_event_header
        })
}

#[no_mangle]
pub unsafe extern "C" fn clap_callback_log(
    _host: *const clap_host,
    _level: i32,
    message: *const c_char,
) {
    let message = CStr::from_ptr(message).to_string_lossy().into_owned();
    eprintln!("[CLAP] Log: {}", message);
}

#[no_mangle]
pub unsafe extern "C" fn clap_callback_thread_pool_request_exec(
    host: *const clap_host,
    num_tasks: u32,
) -> bool {
    false
}

impl Drop for Clap {
    fn drop(&mut self) {
        unsafe {
            if !self.entry.is_null() {
                (*self.entry).deinit.unwrap()();
            }
        }
    }
}

impl PluginInner for Clap {
    fn process(
        &mut self,
        inputs: &[crate::audio_bus::AudioBus<f32>],
        outputs: &mut [crate::audio_bus::AudioBus<f32>],
        events: Vec<crate::event::HostIssuedEvent>,
        process_details: &crate::ProcessDetails,
    ) {
        unsafe {
            let plugin = *self.plugin;

            self.process.frames_count = process_details.block_size as u32;

            let mut input_pointers = HeaplessVec::<*mut f32, 16>::new();
            for input in inputs {
                for channel in input.data.iter() {
                    input_pointers.push(channel.as_ptr() as *mut f32);
                }
            }

            let inputs = clap_audio_buffer {
                data32: input_pointers.as_mut_ptr(),
                data64: std::ptr::null_mut(),
                channel_count: input_pointers.len() as u32,
                latency: 0,
                constant_mask: 0,
            };

            let mut output_pointers = HeaplessVec::<*mut f32, 16>::new();
            for input in outputs {
                for channel in input.data.iter() {
                    output_pointers.push(channel.as_ptr() as *mut f32);
                }
            }

            let mut outputs = clap_audio_buffer {
                data32: output_pointers.as_mut_ptr(),
                data64: std::ptr::null_mut(),
                channel_count: output_pointers.len() as u32,
                latency: 0,
                constant_mask: 0,
            };

            self.process.audio_inputs = &inputs as *const clap_audio_buffer;
            self.process.audio_outputs = &mut outputs as *mut clap_audio_buffer;

            self.in_events.clear();

            for event in events {
                let new_event = create_clap_event(event);

                let _ = self.in_events.push(new_event);
            }

            let in_events = clap_input_events {
                ctx: &self.in_events as *const EventBuffer as *mut c_void,
                size: Some(clap_callback_events_size),
                get: Some(clap_callback_events_get),
            };

            let out_events = clap_output_events {
                ctx: &mut self.out_events as *mut EventBuffer as *mut c_void,
                try_push: Some(clap_callback_events_try_push),
            };

            self.process.in_events = &in_events as *const clap_input_events;
            self.process.out_events = &out_events as *const clap_output_events;

            // self.process.in_events = &self.in_events as *const clap_input_events;

            plugin.process.unwrap()(self.plugin, &self.process);

            for out_event in self.out_events.iter() {
                println!("[CLAP] Event: {}", out_event.header.type_);

                match out_event.header.type_ {
                    CLAP_EVENT_PARAM_VALUE => {
                        let param_value = out_event.param_value;

                        self.host_data
                            .as_mut()
                            .unwrap()
                            .plugin_issued_events_producer
                            .try_push(PluginIssuedEvent::Parameter(
                                crate::parameter::ParameterUpdate {
                                    parameter_id: param_value.param_id as i32,
                                    parameter_index: param_value.param_id as i32,
                                    current_value: param_value.value as f32,
                                    initial_value: param_value.value as f32,
                                    end_edit: false,
                                },
                            ))
                            .ok();
                    }
                    _ => {}
                }
            }

            self.in_events.clear();
            self.out_events.clear();
        }
    }

    fn set_preset_data(&mut self, data: Vec<u8>) -> Result<(), String> {
        todo!()
    }

    fn get_preset_data(&mut self) -> Result<Vec<u8>, String> {
        todo!()
    }

    fn get_preset_name(&mut self, id: i32) -> Result<String, String> {
        todo!()
    }

    fn set_preset(&mut self, id: i32) -> Result<(), String> {
        todo!()
    }

    fn get_parameter(&self, index: i32) -> crate::parameter::Parameter {
        unsafe {
            let plugin = &*self.plugin;
            let params = plugin.get_extension.unwrap()(self.plugin, CLAP_EXT_PARAMS.as_ptr())
                as *const clap_plugin_params;

            if params.is_null() {
                // TODO: Make this function return a result.
            }

            let params = &*params;

            // let count = params.count.unwrap()(self.plugin);

            let mut info: clap_param_info = zeroed();
            if !params.get_info.unwrap()(self.plugin, index as u32, &mut info) {
                // TODO: error
            }

            let mut value: f64 = 0.;
            if !params.get_value.unwrap()(self.plugin, index as u32, &mut value) {
                // TODO: error
            }

            const VALUE_STRING_CAPACITY: usize = 256;
            let value_string = HeaplessString::<VALUE_STRING_CAPACITY>::new();

            if !params.value_to_text.unwrap()(
                self.plugin,
                index as u32,
                value,
                value_string.data_raw() as *mut c_char,
                VALUE_STRING_CAPACITY as u32,
            ) {
                // TODO: error
            }

            crate::parameter::Parameter {
                id: index,
                name: HeaplessString::from_str(
                    CStr::from_ptr(info.name.as_ptr()).to_str().unwrap(),
                )
                .unwrap(),
                index,
                value: value as f32,
                hidden: info.flags & CLAP_PARAM_IS_HIDDEN != 0,
                can_automate: info.flags & CLAP_PARAM_IS_AUTOMATABLE != 0,
                is_wrap_around: info.flags & CLAP_PARAM_IS_PERIODIC != 0,
                read_only: info.flags & CLAP_PARAM_IS_READONLY != 0,
                default_value: info.default_value as f32,
                formatted_value: value_string,
            }
        }
    }

    fn show_editor(
        &mut self,
        window_id: *mut std::ffi::c_void,
        window_id_type: WindowIDType,
    ) -> Result<(usize, usize), Error> {
        unsafe {
            crate::thread_check::ensure_main_thread("Clap::show_editor");

            let plugin = &*self.plugin;
            let gui = plugin.get_extension.unwrap()(self.plugin, CLAP_EXT_GUI.as_ptr())
                as *const clap_plugin_gui;

            if gui.is_null() {
                return Err(Error {
                    message: "CLAP plugin does not support GUI extension".to_string(),
                });
            }

            let gui = &*gui;

            let api = match window_id_type {
                WindowIDType::HWND => CLAP_WINDOW_API_WIN32,
                WindowIDType::XWNDX11 => CLAP_WINDOW_API_X11,
                WindowIDType::XWNDWayland => CLAP_WINDOW_API_WAYLAND,
                WindowIDType::NSView => CLAP_WINDOW_API_COCOA,
                _ => CLAP_WINDOW_API_X11,
            }
            .as_ptr();

            gui.create.unwrap()(self.plugin, api, false);

            let wind = clap_window {
                api,
                specific: std::mem::transmute(window_id),
            };

            gui.set_parent.unwrap()(self.plugin, &wind);

            gui.show.unwrap()(self.plugin);

            let mut w: u32 = 0;
            let mut h: u32 = 0;
            gui.get_size.unwrap()(self.plugin, &mut w, &mut h);

            Ok((w as usize, h as usize))
        }
    }

    fn hide_editor(&mut self) {}

    fn suspend(&mut self) {
        unsafe {
            let plugin = &*self.plugin;
            plugin.stop_processing.unwrap()(self.plugin);
        }
    }

    fn resume(&mut self) {
        unsafe {
            let plugin = &*self.plugin;
            plugin.start_processing.unwrap()(self.plugin);
        }
    }

    fn get_io_configuration(&mut self) -> crate::audio_bus::IOConfigutaion {
        crate::audio_bus::IOConfigutaion {
            audio_inputs: HeaplessVec::from(&vec![AudioBusDescriptor { channels: 2 }]).unwrap(),
            audio_outputs: HeaplessVec::from(&vec![AudioBusDescriptor { channels: 2 }]).unwrap(),
            event_inputs_count: 0,
        }
    }

    fn get_latency(&mut self) -> crate::Samples {
        unsafe {
            let plugin = &*self.plugin;
            let latency = plugin.get_extension.unwrap()(self.plugin, CLAP_EXT_LATENCY.as_ptr())
                as *const clap_plugin_latency;

            if latency.is_null() {
                return 0;
            }

            let latency = &*latency;
            latency.get.unwrap()(self.plugin) as crate::Samples
        }
    }

    fn get_parameter_count(&self) -> usize {
        unsafe {
            let plugin = &*self.plugin;
            let params = plugin.get_extension.unwrap()(self.plugin, CLAP_EXT_PARAMS.as_ptr())
                as *const clap_plugin_params;

            if params.is_null() {
                return 0;
            }

            let params = &*params;

            params.count.unwrap()(self.plugin) as usize
        }
    }

    fn change_sample_rate(&mut self, rate: crate::SampleRate) {
        self.sample_rate = rate;
        self.reactivate();
    }

    fn change_block_size(&mut self, size: crate::BlockSize) {
        self.block_size = size;
        self.reactivate();
    }
}

unsafe fn create_clap_event(event: HostIssuedEvent) -> ClapEvent {
    let mut new_event: ClapEvent = zeroed();
    new_event.header.time = event.block_time as u32;

    if event.is_live {
        new_event.header.flags |= CLAP_EVENT_IS_LIVE;
    }

    const NOTE_ON: u8 = 0x90;
    const NOTE_OFF: u8 = 0x80;

    match event.event_type {
        crate::event::HostIssuedEventType::Midi(midi_event) => {
            match midi_event.midi_data[0] {
                NOTE_ON => {
                    new_event.note.header.type_ = CLAP_EVENT_NOTE_ON;
                    new_event.note.header.size =
                        std::mem::size_of::<clap_event_note>() as u32;
                    new_event.note.port_index = event.bus_index as i16;
                    new_event.note.key = midi_event.midi_data[1] as i16;
                    new_event.note.velocity = midi_event.midi_data[2] as f64;
                }
                NOTE_OFF => {
                    new_event.note.header.type_ = CLAP_EVENT_NOTE_OFF;
                    new_event.note.header.size =
                        std::mem::size_of::<clap_event_note>() as u32;
                    new_event.note.port_index = event.bus_index as i16;
                    new_event.note.key = midi_event.midi_data[1] as i16;
                    new_event.note.velocity = midi_event.midi_data[2] as f64;
                }
                _ => {
                    new_event.midi.header.type_ = CLAP_EVENT_MIDI;
                    new_event.midi.header.size =
                        std::mem::size_of::<clap_event_midi>() as u32;
                    new_event.midi.port_index = event.bus_index as u16;
                    new_event.midi.data = midi_event.midi_data;
                }
            };
        }
        crate::event::HostIssuedEventType::Parameter(parameter_update) => {
            new_event.param_value.header.type_ = CLAP_EVENT_PARAM_VALUE;
            new_event.param_value.header.size =
                std::mem::size_of::<clap_event_param_value>() as u32;
            new_event.param_value.param_id = parameter_update.parameter_id as u32;
            new_event.param_value.value = parameter_update.current_value as f64;

            // There's a bunch of other stuff in `clap_event_param_value` that needs to
            // be looked at.
        }
    }
    new_event
}
