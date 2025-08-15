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
use crate::formats::vst3::vst3_wrapper_sys::FFIPluginDescriptor;
use crate::heapless_vec::HeaplessVec;
use crate::parameter::ParameterUpdate;
use crate::plugin::PluginInner;
use crate::{ProcessDetails, Samples, WindowIDType};

use super::Common;

mod vst3_wrapper_sys;

struct Vst3 {
    app: *const c_void,
    _plugin_issued_events_producer: Box<HeapProd<PluginIssuedEvent>>,
    param_updates_for_edit_controller: HeapRb<ParameterUpdate>,
    param_updates_for_audio_processor: HeapRb<ParameterUpdate>,
}

pub fn load(
    path: &Path,
    id: &str,
    common: Common,
) -> Result<(Box<dyn PluginInner>, PluginDescriptor), Error> {
    let plugin_issued_events_producer = Box::new(common.plugin_issued_events_producer);

    let instance = Vst3 {
        app: std::ptr::null(),
        _plugin_issued_events_producer: plugin_issued_events_producer,
        param_updates_for_edit_controller: HeapRb::new(512),
        param_updates_for_audio_processor: HeapRb::new(512),
    };

    let mut instance = Box::new(instance);

    let app = unsafe {
        let plugin_path = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
        let plugin_id = std::ffi::CString::new(id).unwrap();
        vst3_wrapper_sys::load_plugin(
            plugin_path.as_ptr(),
            plugin_id.as_ptr(),
            &*instance as *const _ as *const c_void,
        )
    };

    instance.app = app;

    let descriptor = unsafe { descriptor(app) }.to_plugin_descriptor(path);
    Ok((instance, descriptor))
}

impl PluginInner for Vst3 {
    fn process(
        &mut self,
        inputs: &[AudioBus<f32>],
        outputs: &mut [AudioBus<f32>],
        mut events: Vec<HostIssuedEvent>,
        process_details: &ProcessDetails,
    ) {
        // Queue parameters to be sent to the IEditController because they need to be sent to both
        // the IEditController and IAudioProcessor separately.
        for update in last_param_updates(&events) {
            let _ = self.param_updates_for_edit_controller.try_push(update);
        }

        // " When the controller transmits a parameter change to the host, the host synchronizes
        //   the processor by passing the new values as Steinberg::Vst::IParameterChanges to the
        //   process call. "
        while let Some(param_update) = self.param_updates_for_audio_processor.try_pop() {
            events.push(HostIssuedEvent {
                ppq_time: process_details.player_time,
                event_type: HostIssuedEventType::Parameter(param_update),
                ..HostIssuedEvent::default()
            });
        }

        let mut channel_buffers = HeaplessVec::<HeaplessVec<*mut f32, 16>, 32>::new();

        let mut input_ptrs = HeaplessVec::<*mut *mut f32, 16>::new();
        let mut output_ptrs = HeaplessVec::<*mut *mut f32, 16>::new();

        assert!(inputs.len() <= 16);
        for bus in inputs.iter() {
            let mut channels = HeaplessVec::<*mut f32, 16>::new();
            assert!(bus.data.len() <= 16);
            for channel_idx in 0..bus.data.len() {
                channels
                    .push(bus.data[channel_idx].as_ptr() as *mut f32)
                    .unwrap();
            }
            channel_buffers.push(channels).unwrap();
            input_ptrs
                .push(channel_buffers.last_mut().unwrap().as_mut_ptr())
                .unwrap();
        }

        assert!(outputs.len() <= 16);
        for bus in outputs.iter_mut() {
            let mut channels = HeaplessVec::<*mut f32, 16>::new();
            assert!(bus.data.len() <= 16);
            for channel_idx in 0..bus.data.len() {
                channels.push(bus.data[channel_idx].as_mut_ptr()).unwrap();
            }
            channel_buffers.push(channels).unwrap();
            output_ptrs
                .push(channel_buffers.last_mut().unwrap().as_mut_ptr())
                .unwrap();
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
            let mut proc_data_len = 0;

            if data.len() < 4 {
                return Err("Invalid data".to_string());
            }

            proc_data_len |= data[0] as usize;
            proc_data_len |= (data[1] as usize) << 8;
            proc_data_len |= (data[2] as usize) << 8 * 2;
            proc_data_len |= (data[3] as usize) << 8 * 3;

            println!("loading with proc data len {}", proc_data_len);

            if data.len() < proc_data_len {
                return Err("Invalid data".to_string());
            }

            let processor_data = &data[4..(proc_data_len + 4)];
            let controller_data = &data[(proc_data_len + 4)..];

            println!("proc {}", processor_data.len());
            println!("cont {}", controller_data.len());

            vst3_wrapper_sys::set_data(
                self.app,
                processor_data.as_ptr() as *const c_void,
                processor_data.len() as i32,
            );
            vst3_wrapper_sys::set_controller_data(
                self.app,
                controller_data.as_ptr() as *const c_void,
                controller_data.len() as i32,
            );

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
            let processor_data = if !data.is_null() {
                let data = std::slice::from_raw_parts(data as *const u8, len as usize)
                    .to_vec()
                    .clone();
                vst3_wrapper_sys::free_data_stream(stream);
                data
            } else {
                vec![]
            };

            let mut len = 0;
            let mut stream = std::ptr::null();
            let data = vst3_wrapper_sys::get_controller_data(
                self.app,
                &mut len as *mut i32,
                &mut stream as *mut *const c_void,
            );
            let controller_data = if !data.is_null() {
                let data = std::slice::from_raw_parts(data as *const u8, len as usize)
                    .to_vec()
                    .clone();
                vst3_wrapper_sys::free_data_stream(stream);
                data
            } else {
                vec![]
            };

            let mut data = vec![];

            let proc_data_len = processor_data.len();

            data.push(((proc_data_len) & 0xFF) as u8);
            data.push(((proc_data_len >> (8)) & 0xFF) as u8);
            data.push(((proc_data_len >> (8 * 2)) & 0xFF) as u8);
            data.push(((proc_data_len >> (8 * 3)) & 0xFF) as u8);

            println!("saving with proc data len {}", proc_data_len);

            println!("proc {}", processor_data.len());
            println!("cont {}", controller_data.len());

            data.extend(processor_data);
            data.extend(controller_data);

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
        unsafe { get_parameter(self.app, id) }
    }

    fn show_editor(
        &mut self,
        window_id: *mut std::ffi::c_void,
        window_id_type: WindowIDType,
    ) -> Result<(usize, usize), Error> {
        let dims = unsafe { vst3_wrapper_sys::show_gui(self.app, window_id as *const c_void, window_id_type) };

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

    fn change_sample_rate(&mut self, _rate: crate::SampleRate) {
        unsafe { vst3_wrapper_sys::vst3_set_sample_rate(self.app, _rate as i32) };
    }

    fn set_track_details(&mut self, details: &crate::track::Track) {
        unsafe { vst3_wrapper_sys::set_track_details(self.app, details) };
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

pub fn get_descriptor(path: &Path) -> Vec<PluginDescriptor> {
    let mut descs = HeaplessVec::<FFIPluginDescriptor, 10>::new();

    let c_path = path.to_string_lossy().to_string() + "\0";
    unsafe {
        vst3_wrapper_sys::get_descriptors(c_path.as_ptr() as *const std::ffi::c_char, &mut descs);
    }

    descs
        .as_slice()
        .iter()
        .map(|d| d.to_plugin_descriptor(path))
        .collect::<Vec<_>>()
}

impl Drop for Vst3 {
    fn drop(&mut self) {
        unsafe {
            vst3_wrapper_sys::unload(self.app);
        }
    }
}
