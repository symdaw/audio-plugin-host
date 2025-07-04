use crate::{error::{err, Error}, heapless_vec::HeaplessVec};

pub struct AudioBus<'a, T> {
    pub data: &'a mut Vec<Vec<T>>,
    /// For `new_alloced`. If non-null this is freed on drop.
    owned_data: *mut Vec<Vec<T>>,
}

unsafe impl<'a, T> Sync for AudioBus<'a, T> {}
unsafe impl<'a, T> Send for AudioBus<'a, T> {}

impl<'a, T> AudioBus<'a, T> {
    pub fn new(data: &'a mut Vec<Vec<T>>) -> Self {
        AudioBus {
            data,
            owned_data: std::ptr::null_mut(),
        }
    }

    pub fn channels(&self) -> usize {
        self.data.len()
    }
}

#[derive(Clone, Debug)]
#[repr(C)]
/// Input and output configuration for the plugin.
pub struct IOConfigutaion {
    pub audio_inputs: HeaplessVec<AudioBusDescriptor, 16>,
    pub audio_outputs: HeaplessVec<AudioBusDescriptor, 16>,
    pub event_inputs_count: i32,
}

impl IOConfigutaion {
    pub fn matches<'a, T>(
        &self,
        inputs: &[AudioBus<'a, T>],
        outputs: &[AudioBus<'a, T>],
    ) -> Result<(), Error> {
        if self.audio_inputs.len() != inputs.len() || self.audio_outputs.len() != outputs.len() {
            return err(&format!(
                "Input/output count mismatch: expected {} inputs and {} outputs, got {} inputs and {} outputs",
                self.audio_inputs.len(),
                self.audio_outputs.len(),
                inputs.len(),
                outputs.len()
            ));
        }

        #[allow(clippy::needless_range_loop)]
        for i in 0..self.audio_inputs.len() {
            let channels = inputs[i].channels();
            let needed_channels = self.audio_inputs[i].channels;
            if channels != needed_channels {
                return err(&format!(
                    "Input channel count mismatch: expected {}, got {}",
                    needed_channels,
                    channels
                ));
            }
        }

        #[allow(clippy::needless_range_loop)]
        for i in 0..self.audio_outputs.len() {
            let channels = outputs[i].channels();
            let needed_channels = self.audio_outputs[i].channels;
            if channels != needed_channels {
                return err(&format!(
                    "Output channel count mismatch: expected {}, got {}",
                    needed_channels,
                    channels
                ));
            }
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Copy)]
#[repr(C)]
pub struct AudioBusDescriptor {
    pub channels: usize,
}

impl<T> AudioBus<'_, T>
where
    T: Default + Clone,
{
    pub fn new_alloced(block_size: usize, channels: usize) -> Self {
        let buffer = vec![vec![T::default(); block_size]; channels];
        let ptr = Box::into_raw(Box::new(buffer));
        AudioBus {
            data: unsafe { &mut *ptr },
            owned_data: ptr,
        }
    }
}

impl<T> Drop for AudioBus<'_, T> {
    fn drop(&mut self) {
        if !self.owned_data.is_null() {
            unsafe {
                drop(Box::from_raw(self.owned_data));
            }
        }
    }
}
