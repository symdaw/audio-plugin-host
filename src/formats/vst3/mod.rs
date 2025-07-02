use std::collections::HashMap;
use std::ffi::c_void;
use std::path::Path;

use ringbuf::traits::{Consumer, Producer};
use ringbuf::{HeapProd, HeapRb};
use vst3_wrapper_sys::{descriptor, get_parameter, set_param_in_edit_controller};

use crate::audio_bus::AudioBus;
use crate::discovery::PluginDescriptor;
use crate::error::Error;
use crate::event::HostIssuedEventType;
use crate::event::{HostIssuedEvent, PluginIssuedEvent};
use crate::parameter::ParameterUpdate;
use crate::plugin::PluginInner;
use crate::{ProcessDetails, Samples};

use super::Common;

mod vst3_wrapper_sys;

struct Vst3 {
    app: *const c_void,
    _plugin_issued_events_producer: Box<HeapProd<PluginIssuedEvent>>,
    param_updates_for_edit_controller: HeapRb<ParameterUpdate>,
}

pub fn load(
    path: &Path,
    common: Common,
) -> Result<(Box<dyn PluginInner>, PluginDescriptor), Error> {
    let plugin_issued_events_producer = Box::new(common.plugin_issued_events_producer);

    let app = unsafe {
        let plugin_path = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
        vst3_wrapper_sys::load_plugin(
            plugin_path.as_ptr(),
            &*plugin_issued_events_producer as *const _ as *const c_void,
        )
    };

    let descriptor = unsafe { descriptor(app) }.to_plugin_descriptor(path);
    let processor = Vst3 {
        app,
        _plugin_issued_events_producer: plugin_issued_events_producer,
        param_updates_for_edit_controller: HeapRb::new(512),
    };

    Ok((Box::new(processor), descriptor))
}

impl PluginInner for Vst3 {
    fn process(
        &mut self,
        inputs: &[AudioBus<f32>],
        outputs: &mut [AudioBus<f32>],
        mut events: Vec<HostIssuedEvent>,
        process_details: &ProcessDetails,
    ) {
        for update in last_param_updates(&events) {
            let _ = self.param_updates_for_edit_controller.try_push(update);
        }

        // TODO: Make real-time safe
        let mut channel_buffers = vec![];

        let mut input_ptrs: Vec<*mut *mut f32> = Vec::with_capacity(inputs.len());
        let mut output_ptrs: Vec<*mut *mut f32> = Vec::with_capacity(outputs.len());

        for bus in inputs.iter() {
            let mut channels = vec![];
            for channel_idx in 0..bus.data.len() {
                channels.push(bus.data[channel_idx].as_ptr() as *mut f32);
            }
            channel_buffers.push(channels);
            input_ptrs.push(channel_buffers.last_mut().unwrap().as_mut_ptr());
        }

        for bus in outputs.iter_mut() {
            let mut channels = vec![];
            for channel_idx in 0..bus.data.len() {
                channels.push(bus.data[channel_idx].as_mut_ptr());
            }
            channel_buffers.push(channels);
            output_ptrs.push(channel_buffers.last_mut().unwrap().as_mut_ptr());
        }

        unsafe {
            vst3_wrapper_sys::process(
                self.app,
                process_details as *const ProcessDetails,
                input_ptrs.as_mut_ptr(),
                output_ptrs.as_mut_ptr(),
                events.as_mut_ptr(),
                events.len() as i32,
            );
        }
    }

    fn set_preset_data(&mut self, data: Vec<u8>) -> Result<(), String> {
        unsafe {
            vst3_wrapper_sys::set_data(self.app, data.as_ptr() as *const c_void, data.len() as i32);
            Ok(())
        }
    }

    fn get_preset_data(&mut self) -> Result<Vec<u8>, String> {
        unsafe {
            let mut len = 0;
            let mut stream = std::ptr::null();

            let data = vst3_wrapper_sys::get_data(
                self.app,
                &mut len as *mut i32,
                &mut stream as *mut *const c_void,
            );

            if data.is_null() {
                return Err("Failed to get preset data".to_string());
            }

            let data = std::slice::from_raw_parts(data as *const u8, len as usize)
                .to_vec()
                .clone();

            vst3_wrapper_sys::free_data_stream(stream);

            Ok(data)
        }
    }

    fn get_preset_name(&mut self, _id: i32) -> Result<String, String> {
        todo!()
    }

    fn set_preset(&mut self, _id: i32) -> Result<(), String> {
        todo!()
    }

    fn get_parameter(&self, id: i32) -> crate::parameter::Parameter {
        unsafe { get_parameter(self.app, id) }.to_parameter()
    }

    fn show_editor(&mut self, window_id: *mut std::ffi::c_void) -> Result<(usize, usize), Error> {
        let dims = unsafe { vst3_wrapper_sys::show_gui(self.app, window_id as *const c_void) };

        Ok((dims.width as usize, dims.height as usize))
    }

    fn hide_editor(&mut self) {
        unsafe { vst3_wrapper_sys::hide_gui(self.app) };
    }

    fn suspend(&mut self) {
        unsafe { vst3_wrapper_sys::set_processing(self.app, false) };
    }

    fn resume(&mut self) {
        unsafe { vst3_wrapper_sys::set_processing(self.app, true) };
    }

    fn get_io_configuration(&mut self) -> crate::audio_bus::IOConfigutaion {
        unsafe { vst3_wrapper_sys::io_config(self.app) }
    }

    fn get_latency(&mut self) -> crate::Samples {
        unsafe { vst3_wrapper_sys::get_latency(self.app) as crate::Samples } 
    }

    fn editor_updates(&mut self) {
        while let Some(update) = self.param_updates_for_edit_controller.try_pop() {
            if !update.current_value.is_nan() {
                unsafe {
                    set_param_in_edit_controller(
                        self.app,
                        update.parameter_id,
                        update.current_value,
                    )
                };
            }
        }
    }

    fn get_parameter_count(&self) -> usize {
        unsafe { vst3_wrapper_sys::parameter_count(self.app) }
    }
}

/// Gets param updates taking the final update at the latest sample for each parameter
fn last_param_updates(events: &[HostIssuedEvent]) -> Vec<ParameterUpdate> {
    struct ParamUpdate<'a> {
        param: &'a ParameterUpdate,
        block_time: Samples,
    }

    // FIXME: Make real-time safe
    let mut updates: HashMap<i32, ParamUpdate> = HashMap::new();
    for event in events {
        if let HostIssuedEventType::Parameter(ref param) = event.event_type {
            if let Some(existing) = updates.get_mut(&param.parameter_id) {
                if event.block_time < existing.block_time {
                    continue;
                }
            }

            updates.insert(
                param.parameter_id,
                ParamUpdate {
                    param,
                    block_time: event.block_time,
                },
            );
        }
    }

    updates.values().map(|v| v.param.clone()).collect()
}
