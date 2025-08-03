use std::cell::RefCell;
use std::path::Path;
use std::sync::Mutex;

use crate::audio_bus::{AudioBus, AudioBusDescriptor, IOConfigutaion};
use crate::discovery::PluginDescriptor;
use crate::error::err;
use crate::event::{HostIssuedEvent, HostIssuedEventType, PluginIssuedEvent};
use crate::formats::Format;
use crate::heapless_vec::{HeaplessString, HeaplessVec};
use crate::host::{Host, KnobPreference, Language};
use crate::parameter::Parameter;
use crate::plugin::PluginInner;
use crate::thread_check::{ensure_main_thread, ensure_non_main_thread};
use crate::utils::macos_exec_location;
use crate::{error::Error, SampleRate};
use crate::{BlockSize, PlayingState, ProcessDetails, WindowIDType};

use std::sync::{atomic::AtomicBool, Arc};

use ringbuf::traits::Producer;
use ringbuf::HeapProd;
use vst::api::{HostLanguage, SpeakerArrangementType};
use vst::channels::StereoChannel;
use vst::host::Dispatch;
use vst::plugin::{Plugin, PluginParameters};
use vst::{
    api::{Event, TimeInfoFlags},
    editor::{Editor, KnobMode},
    host::{HostBuffer, PluginInstance},
};

use super::Common;

pub(crate) fn get_descriptor(path: &Path) -> Option<PluginDescriptor> {
    let host = NullHost {};

    let bin_path = macos_exec_location(path)?;

    let mut loader = vst::host::PluginLoader::load(&bin_path, Arc::new(Mutex::new(host))).ok()?;

    let instance = loader.instance().ok()?;

    let info = instance.get_info();

    let descriptor = PluginDescriptor {
        name: info.name,
        id: info.unique_id.to_string(),
        path: path.to_path_buf(),
        version: info.version.to_string(),
        vendor: info.vendor,
        format: Format::Vst2,
        initial_latency: info.initial_delay as usize,
    };

    Some(descriptor)
}

pub(super) fn load(
    path: &Path,
    common: Common,
) -> Result<(Box<dyn PluginInner>, PluginDescriptor), Error> {
    let Some(path) = macos_exec_location(path) else {
        return err("Invalid app bundle".to_string());
    };

    let details = Arc::new(std::sync::Mutex::new(ProcessDetails::default()));
    let size_change = Arc::new(std::sync::Mutex::new(None));
    let editor_param_state = Arc::new(std::sync::Mutex::new(EditorParamsState {
        just_started: vec![],
        currently_editing: vec![],
    }));

    let io_changed = Arc::new(AtomicBool::new(false));

    let host = Arc::new(Mutex::new(Vst2Host {
        host: common.host.clone(),
        plugin_issued_events_producer: RefCell::new(common.plugin_issued_events_producer),
        process_details: details.clone(),
        size_change: size_change.clone(),
        editor_params_state: editor_param_state.clone(),
        io_changed: io_changed.clone(),
    }));

    let mut loader =
        vst::host::PluginLoader::load(&path, Arc::clone(&host)).map_err(|e| Error {
            message: e.to_string(),
        })?;

    let mut instance = loader.instance().map_err(|e| Error {
        message: e.to_string(),
    })?;

    let info = instance.get_info();

    let descriptor = PluginDescriptor {
        name: info.name,
        id: info.unique_id.to_string(),
        path: path.to_path_buf(),
        version: info.version.to_string(),
        vendor: info.vendor,
        format: Format::Vst2,
        initial_latency: info.initial_delay as usize,
    };

    instance.init();

    let plugin = Vst2 {
        process_details: details,
        parameter_object: instance.get_parameter_object(),
        plugin_instance: instance,
        state: Vst2State::Suspended,
        host_buffer: HostBuffer::new(info.inputs as usize, info.outputs as usize),
        editor: None,
        host: common.host,
    };

    Ok((Box::new(plugin), descriptor))
}

pub(super) struct Vst2 {
    process_details: Arc<std::sync::Mutex<ProcessDetails>>,
    parameter_object: Arc<dyn PluginParameters>,
    plugin_instance: PluginInstance,
    state: Vst2State,
    host_buffer: HostBuffer<f32>,
    host: Host,
    editor: Option<Box<dyn Editor>>,
}

unsafe impl Send for Vst2 {}
unsafe impl Sync for Vst2 {}

struct EditorParamsState {
    just_started: Vec<i32>,
    currently_editing: Vec<ParamState>,
}

#[derive(Clone)]
struct ParamState {
    index: i32,
    current: f32,
    initial: f32,
}

#[derive(PartialEq, Eq)]
enum Vst2State {
    Suspended,
    Resumed,
}

impl PluginInner for Vst2 {
    fn change_sample_rate(&mut self, rate: SampleRate) {
        self.suspend();
        self.plugin_instance.set_sample_rate(rate as f32);
    }

    fn change_block_size(&mut self, size: BlockSize) {
        self.suspend();
        self.plugin_instance.set_block_size(size as i64);
    }

    fn suspend(&mut self) {
        if self.state == Vst2State::Suspended {
            return;
        }

        self.plugin_instance.stop_process();
        self.plugin_instance.suspend();
        self.state = Vst2State::Suspended;
    }

    fn resume(&mut self) {
        if self.state == Vst2State::Resumed {
            return;
        }

        self.plugin_instance.resume();
        self.plugin_instance.start_process();
        self.state = Vst2State::Resumed;
    }

    fn show_editor(
        &mut self,
        window_id: *mut std::ffi::c_void,
        _window_id_type: WindowIDType,
    ) -> Result<(usize, usize), Error> {
        ensure_main_thread("[VST2] show_editor");

        if self.editor.is_none() {
            let Some(editor) = self.plugin_instance.get_editor() else {
                return err("Plugin does not have an editor".to_string());
            };
            self.editor = Some(editor);
        }

        let editor = self.editor.as_mut().unwrap();

        let (w, h) = editor.size();

        editor.open(window_id);

        if let Some(knob_pref) = self.host.knob_preference {
            editor.set_knob_mode(match knob_pref {
                KnobPreference::Linear => KnobMode::Linear,
                KnobPreference::Circular => KnobMode::Circular,
            });
        }

        self.plugin_instance.resume();

        Ok((w as usize, h as usize))
    }

    fn hide_editor(&mut self) {
        if let Some(editor) = &mut self.editor {
            editor.close();
        }
    }

    // fn editor_events(&mut self, events: &EventsForPlugins) {
    //     if !self.currently_showing_window {
    //         return;
    //     }
    //
    //     if let Some(editor) = &mut self.editor {
    //         for key_press in events.key_presses.iter() {
    //             editor.key_down(to_vst2_key(key_press));
    //         }
    //
    //         for key_release in events.key_releases.iter() {
    //             editor.key_up(to_vst2_key(key_release));
    //         }
    //
    //         // if let Some(wheel) = events.wheel {
    //         // }
    //     }
    // }

    fn process(
        &mut self,
        inputs: &[AudioBus<f32>],
        outputs: &mut [AudioBus<f32>],
        events: Vec<HostIssuedEvent>,
        process_details: &ProcessDetails,
    ) {
        ensure_non_main_thread("[VST2] process");

        let _ = outputs;
        {
            let mut details_lock = self.process_details.lock().unwrap();
            *details_lock = process_details.clone();
        }

        // Events are sorted by time.
        // Since sample-accurate parameters are not supported we want to only take the last value
        // for each parameter. Loop from last to first skipping anything we've already seen.
        let mut seen = HeaplessVec::<i32, 300>::new();
        for i in 0..events.len() {
            if let HostIssuedEventType::Parameter(ref param) =
                unsafe { events.get_unchecked(events.len() - 1 - i) }.event_type
            {
                if seen.contains(param.parameter_id) {
                    continue;
                }

                self.plugin_instance
                    .get_parameter_object()
                    .set_parameter(param.parameter_id, param.current_value);

                let _ = seen.push(param.parameter_id);
            }
        }

        process_vst2_midi_events_list(&self.plugin_instance, events);

        let mut input = vec![];
        let mut output = vec![];

        for input_bus in inputs.iter() {
            for channel in input_bus.data.iter() {
                input.push(channel);
            }
        }
        for output_bus in outputs.iter_mut() {
            for channel in output_bus.data.iter_mut() {
                output.push(channel);
            }
        }

        let mut audio_buffer = self.host_buffer.bind(&input, &mut output);

        self.plugin_instance.process(&mut audio_buffer);
    }

    fn set_preset_data(&mut self, data: Vec<u8>) -> Result<(), String> {
        if !self.plugin_instance.get_info().preset_chunks {
            return Err("Plugin does not support preset data".to_string());
        }

        self.plugin_instance
            .get_parameter_object()
            .load_preset_data(data.as_slice());
        Ok(())
    }

    fn get_preset_data(&mut self) -> Result<Vec<u8>, String> {
        if !self.plugin_instance.get_info().preset_chunks {
            return Err("Plugin does not support preset data".to_string());
        }

        Ok(self
            .plugin_instance
            .get_parameter_object()
            .get_preset_data())
    }

    fn get_preset_name(&mut self, id: i32) -> Result<String, String> {
        Ok(self
            .plugin_instance
            .get_parameter_object()
            .get_preset_name(id))
    }

    fn set_preset(&mut self, id: i32) -> Result<(), String> {
        self.plugin_instance
            .get_parameter_object()
            .change_preset(id);
        Ok(())
    }

    fn get_parameter(&self, id: i32) -> crate::parameter::Parameter {
        let value = self.parameter_object.get_parameter(id);
        let name = self.parameter_object.get_parameter_name(id);

        let text = self.parameter_object.get_parameter_text(id);
        let label = self.parameter_object.get_parameter_label(id);
        let formatted_value = format!("{} {}", text.trim(), label.trim())
            .trim()
            .to_string();
        let can_automate = self.parameter_object.can_be_automated(id);

        Parameter {
            id,
            name: HeaplessString::from_str(&name).unwrap(),
            index: id,
            value,
            formatted_value: HeaplessString::from_str(&formatted_value).unwrap(),
            hidden: false,
            can_automate,
            is_wrap_around: false,
            read_only: false,
            default_value: f32::NAN,
        }
    }

    fn get_io_configuration(&mut self) -> IOConfigutaion {
        let info = self.plugin_instance.get_info();

        let event_inputs_count = info.midi_inputs.min(1); // TODO: Look into supporting more channels

        let mut inputs = HeaplessVec::new();
        let mut outputs = HeaplessVec::new();

        // TODO: Check these. Lots of plugins don't specify this correctly.
        // for i in 0..info.inputs {
        //     let input_info = self.plugin_instance.get_input_info(i);
        //     append_bus_info_from_arrangment(&mut inputs, input_info.arrangement_type);
        // }
        // for i in 0..info.inputs {
        //     let output_info = self.plugin_instance.get_output_info(i);
        //     append_bus_info_from_arrangment(&mut outputs, output_info.arrangement_type);
        // }

        if total_channels(&inputs) != info.inputs as usize
            || total_channels(&outputs) != info.outputs as usize
        {
            // Plugin didn't specify speaker arrangement properly. Just try make a guess. This
            // works for most plugins.

            outputs.clear();
            inputs.clear();

            match info.inputs {
                0 => {}
                1 | 2 => {
                    // Mono or stereo
                    let _ = inputs.push(AudioBusDescriptor {
                        channels: info.inputs as usize,
                    });
                }
                _ => {
                    let _ = inputs.push(AudioBusDescriptor { channels: 2 });
                    let _ = inputs.push(AudioBusDescriptor {
                        channels: info.inputs as usize - 2,
                    });
                }
            }
            match info.outputs {
                0 => {}
                1 | 2 => {
                    // Mono or stereo
                    let _ = outputs.push(AudioBusDescriptor {
                        channels: info.outputs as usize,
                    });
                }
                _ => {
                    // Stereo with sidechain
                    let _ = outputs.push(AudioBusDescriptor { channels: 2 });
                    let _ = outputs.push(AudioBusDescriptor {
                        channels: info.outputs as usize - 2,
                    });
                }
            }
        }

        self.plugin_instance.info = info;

        IOConfigutaion {
            audio_inputs: inputs,
            audio_outputs: outputs,
            event_inputs_count,
        }
    }

    fn get_latency(&mut self) -> usize {
        self.plugin_instance.initial_delay() as usize
    }

    fn get_parameter_count(&self) -> usize {
        self.plugin_instance.info.parameters as usize
    }
}

fn total_channels<const N: usize>(buses: &HeaplessVec<AudioBusDescriptor, N>) -> usize {
    buses.iter().map(|b| b.channels).sum()
}

fn append_bus_info_from_arrangment(
    buses: &mut HeaplessVec<AudioBusDescriptor, 16>,
    arrangement: vst::channels::SpeakerArrangementType,
) {
    match arrangement {
        vst::channels::SpeakerArrangementType::Empty => {}
        vst::channels::SpeakerArrangementType::Mono => {
            let _ = buses.push(AudioBusDescriptor { channels: 1 });
        }
        vst::channels::SpeakerArrangementType::Stereo(_, channel) => {
            // Assume left will also be present and ignore it
            if channel == StereoChannel::Right {
                let _ = buses.push(AudioBusDescriptor { channels: 2 });
            }
        }
        vst::channels::SpeakerArrangementType::Surround(config) => {
            // TODO
        }
        vst::channels::SpeakerArrangementType::Custom => {
            // TODO
        }
    }
}

#[allow(dead_code)]
struct Vst2Host {
    process_details: Arc<std::sync::Mutex<ProcessDetails>>,
    host: Host,
    plugin_issued_events_producer: RefCell<HeapProd<PluginIssuedEvent>>,
    size_change: Arc<std::sync::Mutex<Option<(i32, i32)>>>,
    editor_params_state: Arc<std::sync::Mutex<EditorParamsState>>,
    io_changed: Arc<AtomicBool>,
}

impl vst::host::Host for Vst2Host {
    fn automate(&self, index: i32, value: f32) {
        let mut initial_value = None;

        {
            let mut editor_params_state = self.editor_params_state.lock().unwrap();

            let editing_index = editor_params_state
                .currently_editing
                .iter()
                .position(|p| p.index == index);

            if editor_params_state.just_started.contains(&index) || editing_index.is_none() {
                editor_params_state.currently_editing.push(ParamState {
                    index,
                    current: value,
                    initial: value,
                });
            } else {
                let editing_index = editing_index.unwrap();
                editor_params_state.currently_editing[editing_index].current = value;
                initial_value = Some(editor_params_state.currently_editing[editing_index].initial);
            }

            editor_params_state.just_started.retain(|&i| i != index);
        }

        let _ =
            self.plugin_issued_events_producer
                .borrow_mut()
                .try_push(PluginIssuedEvent::Parameter(
                    crate::parameter::ParameterUpdate {
                        parameter_id: index,
                        parameter_index: index,
                        current_value: value,
                        initial_value: initial_value.unwrap_or(f32::NAN),
                        end_edit: false,
                    },
                ));
    }

    fn get_time_info(&self, _mask: i32) -> Option<vst::api::TimeInfo> {
        let mut info = vst::api::TimeInfo::default();

        let details = (self.process_details).lock().unwrap();

        let mut flags = TimeInfoFlags::TEMPO_VALID
            | TimeInfoFlags::TIME_SIG_VALID
            | TimeInfoFlags::NANOSECONDS_VALID
            | TimeInfoFlags::PPQ_POS_VALID
            | TimeInfoFlags::BARS_VALID;

        if details.cycle_enabled {
            flags |= TimeInfoFlags::TRANSPORT_CYCLE_ACTIVE;
            flags |= TimeInfoFlags::CYCLE_POS_VALID;

            info.cycle_start_pos = details.cycle_start;
            info.cycle_end_pos = details.cycle_end;
        }

        if details.playing_state == PlayingState::Recording {
            flags |= TimeInfoFlags::TRANSPORT_RECORDING;
        } else if details.playing_state.is_playing() {
            flags |= TimeInfoFlags::TRANSPORT_PLAYING;
        }

        info.nanoseconds = details.nanos;

        info.time_sig_numerator = details.time_signature_numerator as i32;
        info.time_sig_denominator = details.time_signature_denominator as i32;
        info.tempo = details.tempo;
        info.sample_rate = details.sample_rate as f64;
        info.ppq_pos = details.player_time;
        info.sample_pos = details.player_time / (details.tempo / 60.) * details.sample_rate as f64;
        info.bar_start_pos = details.bar_start_pos;

        info.flags = flags.bits();

        Some(info)
    }

    fn get_info(&self) -> (isize, String, String) {
        (1, self.host.name.to_string(), self.host.vendor.to_string())
    }

    fn get_block_size(&self) -> isize {
        self.process_details.lock().unwrap().block_size as isize
    }

    fn begin_edit(&self, index: i32) {
        self.editor_params_state
            .lock()
            .unwrap()
            .just_started
            .push(index);
    }

    fn end_edit(&self, index: i32) {
        let mut editor_params_state = self.editor_params_state.lock().unwrap();

        if let Some(currently_editing_index) = editor_params_state
            .currently_editing
            .iter()
            .position(|p| p.index == index)
        {
            let param = editor_params_state
                .currently_editing
                .remove(currently_editing_index);

            let _ = self.plugin_issued_events_producer.borrow_mut().try_push(
                PluginIssuedEvent::Parameter(crate::parameter::ParameterUpdate {
                    parameter_id: index as i32,
                    parameter_index: index,
                    current_value: param.current,
                    initial_value: param.initial,
                    end_edit: true,
                }),
            );
        }
    }

    fn update_display(&self) {
        let _ = self
            .plugin_issued_events_producer
            .borrow_mut()
            .try_push(PluginIssuedEvent::UpdateDisplay);
    }

    fn can_do(&self, can_do: &str) -> isize {
        println!("can_do: {}", can_do);

        match can_do {
            "sendVstTimeInfo" => 1,
            "sendVstEvents" => 1,
            "sendVstMidiEvent" => 1,
            "sizeWindow" => 1,
            _ => 0,
        }
    }

    fn get_language(&self) -> i32 {
        #[allow(clippy::match_like_matches_macro)]
        #[allow(unreachable_patterns)]
        match self.host.language.unwrap_or_default() {
            Language::English => HostLanguage::English as i32,
            Language::Spanish => HostLanguage::Spanish as i32,
            Language::French => HostLanguage::French as i32,
            Language::German => HostLanguage::German as i32,
            Language::Italian => HostLanguage::Italian as i32,
            _ => HostLanguage::English as i32,
        }
    }

    fn get_process_level(&self) -> i32 {
        match self.process_details.lock().unwrap().playing_state {
            PlayingState::OfflineRendering => vst::api::ProcessLevel::Offline as i32,
            _ => vst::api::ProcessLevel::Realtime as i32,
        }
    }

    fn set_size(&self, width: i32, height: i32) {
        let _ = self.plugin_issued_events_producer.borrow_mut().try_push(
            PluginIssuedEvent::ResizeWindow(width as usize, height as usize),
        );
    }

    fn io_changed(&mut self) {
        let _ = self
            .plugin_issued_events_producer
            .borrow_mut()
            .try_push(PluginIssuedEvent::IOChanged);
    }
}

fn process_vst2_midi_events_list(
    plugin_instance: &PluginInstance,
    mut midi_events: Vec<HostIssuedEvent>,
) {
    midi_events.sort_by(|a, b| a.block_time.cmp(&b.block_time));

    let events: Vec<*mut Event> = midi_events
        .into_iter()
        .filter_map(|event| midi_event_to_vst2_event(&event))
        .collect();

    let num_events = events.len();

    const MAX_EVENTS: usize = 100;

    let mut events_object = Vst2Events::<MAX_EVENTS>::new(num_events);

    if num_events > MAX_EVENTS {
        // TODO: Handle this case
        return;
    }

    for (i, event) in events.iter().enumerate() {
        events_object.events[i] = *event;
    }

    plugin_instance.dispatch(
        vst::plugin::OpCode::ProcessEvents,
        0,
        0,
        &events_object as *const _ as *mut _,
        0.0,
    );

    for event in events {
        drop(unsafe { Box::from_raw(event) });
    }
}

fn midi_event_to_vst2_event(midi_event: &HostIssuedEvent) -> Option<*mut Event> {
    let frame = midi_event.block_time;

    let HostIssuedEventType::Midi(ref event) = midi_event.event_type else {
        return None;
    };

    let event = vst::api::MidiEvent {
        event_type: vst::api::EventType::Midi,
        byte_size: 0,
        delta_frames: frame as i32,
        flags: 0,
        note_length: event.note_length as i32,
        note_offset: 0,
        midi_data: event.midi_data,
        detune: event.detune as i8,
        note_off_velocity: 0,
        _reserved1: 0,
        _reserved2: 0,
        _midi_reserved: 0,
    };

    let event: Event = unsafe { std::mem::transmute(event) };
    let event = Box::into_raw(Box::new(event));

    Some(event)
}

#[repr(C)]
pub struct Vst2Events<const L: usize> {
    pub num_events: i32,
    pub _reserved: isize,
    pub events: [*mut Event; L],
}

impl<const L: usize> Vst2Events<L> {
    pub fn new(len: usize) -> Self {
        Self {
            num_events: len as i32,
            _reserved: 0,
            events: [std::ptr::null_mut(); L],
        }
    }
}

struct NullHost {}

impl vst::host::Host for NullHost {
    fn automate(&self, _index: i32, _value: f32) {}

    fn begin_edit(&self, _index: i32) {}

    fn end_edit(&self, _index: i32) {}

    fn get_info(&self) -> (isize, String, String) {
        (0, String::new(), String::new())
    }

    fn get_time_info(&self, _mask: i32) -> Option<vst::api::TimeInfo> {
        None
    }

    fn get_block_size(&self) -> isize {
        0
    }

    fn update_display(&self) {}

    fn can_do(&self, _can_do: &str) -> isize {
        1
    }

    fn get_language(&self) -> i32 {
        0
    }

    fn get_process_level(&self) -> i32 {
        0
    }

    fn set_size(&self, _width: i32, _height: i32) {}
}
