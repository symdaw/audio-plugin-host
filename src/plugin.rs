use std::{any::Any, path::Path, sync::atomic::AtomicUsize};

use ringbuf::{traits::*, HeapCons, HeapRb};

use crate::{
    audio_bus::{AudioBus, IOConfigutaion},
    discovery::PluginDescriptor,
    error::{err, Error},
    event::{HostIssuedEvent, PluginIssuedEvent},
    host::Host,
    parameter::Parameter,
    BlockSize, ProcessDetails, SampleRate, Samples,
};

/// Loads a plugin of any of the supported formats from the given path and returns a
/// `PluginInstance`.
pub fn load<P: AsRef<Path>>(path: P, host: &Host) -> Result<PluginInstance, Error> {
    let plugin_issued_events: HeapRb<PluginIssuedEvent> = HeapRb::new(512);
    let (plugin_issued_events_producer, plugin_issued_events_consumer) =
        plugin_issued_events.split();

    let common = crate::formats::Common {
        host: host.clone(),
        plugin_issued_events_producer,
    };

    let (mut inner, descriptor) = crate::formats::load_any(path.as_ref(), common)?;

    let io_configuration = inner.get_io_configuration();

    Ok(PluginInstance {
        latency: AtomicUsize::new(descriptor.initial_latency),
        window: Box::new(()),
        descriptor,
        inner,
        plugin_issued_events: plugin_issued_events_consumer,
        sample_rate: 0,
        block_size: 0,
        showing_editor: false,
        io_configuration,
        resumed: false,
    })
}

pub struct PluginInstance {
    pub descriptor: PluginDescriptor,
    /// `Box` to store a window object for convenience. This isn't used by this
    /// crate at all you can use this however you want. Whatever you put in here
    /// will be dropped when the editor is closed.
    pub window: Box<dyn Any>,
    pub(crate) inner: Box<dyn PluginInner>,
    plugin_issued_events: HeapCons<PluginIssuedEvent>,
    sample_rate: SampleRate,
    block_size: BlockSize,
    showing_editor: bool,
    latency: AtomicUsize,
    io_configuration: IOConfigutaion,
    resumed: bool,
}

unsafe impl Send for PluginInstance {}
unsafe impl Sync for PluginInstance {}

impl PluginInstance {
    /// {Audio thread}
    pub fn process(
        &mut self,
        inputs: &Vec<AudioBus<f32>>,
        outputs: &mut Vec<AudioBus<f32>>,
        mut events: Vec<HostIssuedEvent>,
        process_details: &ProcessDetails,
    ) {
        if let Err(e) = self.io_configuration.matches(inputs, outputs) {
            panic!("Inputs and outputs do not match the plugin's IO configuration:\n{}", e);
        }

        events.sort_by_key(|e| e.block_time);

        self.resume();

        self.fix_configuration(process_details);

        self.inner.process(inputs, outputs, events, process_details);
    }

    /// {UI Thread} Must be called routinely by the UI thread. Consume `PluginIssuedEvent`s
    /// queued by the plugin. Informs the host of parameter changes in the editor, latency
    /// changes, etc.
    pub fn get_events(&mut self) -> Vec<PluginIssuedEvent> {
        self.inner.editor_updates();

        let mut events = Vec::new();
        while let Some(event) = self.plugin_issued_events.try_pop() {
            events.extend(self.create_subsequent_events(&event));

            events.push(event);
        }

        events
    }

    /// {Any thread}
    pub fn get_latency(&self) -> usize {
        self.latency.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// {UI thread}
    pub fn get_io_configuration(&mut self) -> IOConfigutaion {
        let io = self.inner.get_io_configuration();
        self.io_configuration = io.clone();
        io
    }

    pub fn resume(&mut self) {
        if self.resumed {
            return;
        }

        self.inner.resume();
        self.resumed = true;
    }

    pub fn suspend(&mut self) {
        if !self.resumed {
            return;
        }
        self.inner.suspend();

        self.resumed = false;
    }

    pub fn get_descriptor(&self) -> PluginDescriptor {
        self.descriptor.clone()
    }

    pub fn get_preset_data(&mut self) -> Result<Vec<u8>, String> {
        self.inner.get_preset_data()
    }

    pub fn set_preset_data(&mut self, data: Vec<u8>) -> Result<(), String> {
        self.inner.set_preset_data(data)
    }

    pub fn get_preset_name(&mut self, id: i32) -> Result<String, String> {
        self.inner.get_preset_name(id)
    }

    pub fn set_preset(&mut self, id: i32) -> Result<(), String> {
        self.inner.set_preset(id)
    }

    pub fn get_parameter(&self, index: i32) -> Parameter {
        self.inner.get_parameter(index)
    }

    pub fn get_all_parameters(&self) -> Vec<Parameter> {
        (0..self.inner.get_parameter_count())
            .map(|i| self.inner.get_parameter(i as i32))
            .filter(|p| !p.hidden)
            .collect()
    }

    pub fn get_parameter_count(&self) -> usize {
        self.inner.get_parameter_count()
    }

    pub fn show_editor(
        &mut self,
        window_id: *mut std::ffi::c_void,
    ) -> Result<(usize, usize), Error> {
        if self.showing_editor {
            return err("Editor is already open");
        }

        let size = self.inner.show_editor(window_id)?;

        self.showing_editor = true;

        Ok(size)
    }

    pub fn hide_editor(&mut self) {
        if !self.showing_editor {
            return;
        }

        self.inner.hide_editor();
        self.window = Box::new(());

        self.showing_editor = false;
    }

    pub fn is_showing_editor(&self) -> bool {
        self.showing_editor
    }

    fn fix_configuration(&mut self, process_details: &ProcessDetails) {
        if self.sample_rate != process_details.sample_rate {
            self.sample_rate = process_details.sample_rate;
            self.inner.change_sample_rate(process_details.sample_rate);
        }

        if self.block_size != process_details.block_size {
            self.block_size = process_details.block_size;
            self.inner.change_block_size(process_details.block_size);
        }
    }

    /// Returns any new events
    fn create_subsequent_events(&mut self, event: &PluginIssuedEvent) -> Vec<PluginIssuedEvent> {
        match event {
            PluginIssuedEvent::IOChanged => {
                self.io_configuration = self.inner.get_io_configuration();

                let latency = self.inner.get_latency();

                self.latency
                    .store(latency, std::sync::atomic::Ordering::Relaxed);

                vec![PluginIssuedEvent::ChangeLatency(latency)]
            }
            _ => {
                vec![]
            }
        }
    }
}

pub(crate) trait PluginInner {
    fn process(
        &mut self,
        inputs: &[AudioBus<f32>],
        outputs: &mut [AudioBus<f32>],
        events: Vec<HostIssuedEvent>,
        process_details: &ProcessDetails,
    );

    fn set_preset_data(&mut self, data: Vec<u8>) -> Result<(), String>;
    fn get_preset_data(&mut self) -> Result<Vec<u8>, String>;
    fn get_preset_name(&mut self, id: i32) -> Result<String, String>;
    fn set_preset(&mut self, id: i32) -> Result<(), String>;

    fn get_parameter(&self, index: i32) -> Parameter;

    fn show_editor(&mut self, window_id: *mut std::ffi::c_void) -> Result<(usize, usize), Error>;
    fn hide_editor(&mut self);

    fn change_sample_rate(&mut self, _rate: SampleRate) {}
    fn change_block_size(&mut self, _size: BlockSize) {}
    fn suspend(&mut self);
    fn resume(&mut self);

    fn get_io_configuration(&mut self) -> IOConfigutaion;

    fn get_latency(&mut self) -> Samples;

    fn editor_updates(&mut self) {}

    fn get_parameter_count(&self) -> usize;
}
