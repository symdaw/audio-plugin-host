use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use audio_bus::AudioBus;
use audio_plugin_host::*;
use event::PluginIssuedEvent;
use plugin::PluginInstance;
use sdl2::audio::{AudioCallback, AudioDevice};
use sdl2::{Sdl, VideoSubsystem};

fn main() {
    let mut host = host::Host::new(
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_AUTHORS"),
    );

    host.thread_pool_hander = Some(thread_pool_run);

    thread_check::mark_current_as_main();

    let plugin_path = PathBuf::from(std::env::args().nth(1).expect("No plugin path provided"));

    let descriptors = discovery::get_descriptor_from_file(&plugin_path);
    println!("Plugins in file: {:?}", descriptors);

    let mut plugin = plugin::load(
        &plugin_path,
        &descriptors.first().expect("No plugins in file").id,
        &host,
    )
    .unwrap();

    println!("IO configuration: {:?}", plugin.get_io_configuration());

    let plugin = Arc::new(Mutex::new(plugin));

    let (sdl, video) = sdl();
    let audio_device = SDLAudioDevice::new(&sdl, plugin.clone());
    audio_device.device.resume();
    let mut window = video
        .window(&plugin.lock().unwrap().descriptor.name, 1024, 769)
        .build()
        .unwrap();
    let window_id = get_window_id(&window);

    let (width, height) = plugin.lock().unwrap().show_editor(window_id, WindowIDType::this_platform()).unwrap();
    window.set_size(width as u32, height as u32).unwrap();

    loop {
        let mut event_pump = sdl.event_pump().unwrap();
        for event in event_pump.poll_iter() {
            if let sdl2::event::Event::Quit { .. } = event {
                return;
            }
        }

        let events = plugin.lock().unwrap().get_events();

        if !events.is_empty() {
            println!("Received events: {:?}", events);
        }

        for event in events {
            match event {
                PluginIssuedEvent::ResizeWindow(width, height) => {
                    window.set_size(width as u32, height as u32).unwrap();
                }
                PluginIssuedEvent::Parameter(param) => {
                    let param = plugin.lock().unwrap().get_parameter(param.parameter_id);
                    println!("Parameter updated {:?}", param);
                }
                _ => {}
            }
        }
    }
}

fn thread_pool_run(func: Box<dyn Fn(usize)>, num_tasks: usize) {
    for i in 0..num_tasks {
        println!("[ThreadPool] Running task {}", i);
        func(i);
    }
}

impl AudioCallback for SDLAudioDeviceCallback {
    type Channel = f32;

    fn callback(&mut self, out: &mut [f32]) {
        let start_time = SystemTime::now();

        // This does not support all IO configurations and is not real-time safe. This is just
        // for demonstration purposes.
        let io = self.plugin.lock().unwrap().get_io_configuration();
        let mut input_buses = vec![];
        let mut output_buses = vec![];
        for bus in io.audio_inputs.iter() {
            input_buses.push(AudioBus::new_alloced(
                self.block_size as usize,
                bus.channels,
            ));
        }
        for bus in io.audio_outputs.iter() {
            output_buses.push(AudioBus::new_alloced(
                self.block_size as usize,
                bus.channels,
            ));
        }

        let process_details = ProcessDetails {
            block_size: self.block_size,
            sample_rate: self.sample_rate,
            nanos: start_time
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos() as f64,
            ..Default::default()
        };

        self.plugin.lock().unwrap().process(
            &input_buses,
            &mut output_buses,
            vec![],
            &process_details,
        );

        if let Some(output) = output_buses.get_mut(0) {
            for i in 0..self.block_size {
                for j in 0..CHANNELS as usize {
                    out[i * CHANNELS as usize + j] = output.data[j][i];
                }
            }
        }
    }
}

fn sdl() -> (Sdl, VideoSubsystem) {
    let sdl = sdl2::init().unwrap();
    let video = sdl.video().unwrap();
    let gl_attr = video.gl_attr();
    gl_attr.set_context_profile(sdl2::video::GLProfile::Core);
    gl_attr.set_context_version(3, 3);
    gl_attr.set_context_flags().forward_compatible().set();

    (sdl, video)
}

#[cfg(target_os = "windows")]
use winapi::shared::minwindef::HINSTANCE;
#[cfg(target_os = "windows")]
use winapi::shared::windef::{HDC, HWND};

#[cfg(target_os = "windows")]
#[repr(C)]
struct Win {
    hwnd: HWND,
    hdc: HDC,
    hinstance: HINSTANCE,
}

#[cfg(target_os = "windows")]
fn get_window_id(window: &sdl2::video::Window) -> *mut std::ffi::c_void {
    let mut wm_info = sdl2::sys::SDL_SysWMinfo {
        version: sdl2::sys::SDL_version {
            major: 2,
            minor: 0,
            patch: 10,
        },
        subsystem: sdl2::sys::SDL_SYSWM_TYPE::SDL_SYSWM_UNKNOWN,
        info: unsafe { std::mem::zeroed() },
    };

    unsafe { sdl2::sys::SDL_GetWindowWMInfo(window.raw(), &mut wm_info) };

    unsafe {
        let win = std::mem::transmute::<_, *mut Win>(&wm_info.info as *const _);
        (*win).hwnd as *mut std::ffi::c_void
    }
}

const CHANNELS: u8 = 2;
const BLOCK_SIZE: usize = 512;
const SAMPLE_RATE: u32 = 44100;

pub struct SDLAudioDeviceCallback {
    pub block_size: BlockSize,
    pub sample_rate: SampleRate,
    pub plugin: Arc<Mutex<PluginInstance>>,
}

pub struct SDLAudioDevice {
    pub device: Box<AudioDevice<SDLAudioDeviceCallback>>,
}

impl SDLAudioDevice {
    pub fn new(sdl_context: &Sdl, plugin: Arc<Mutex<PluginInstance>>) -> Self {
        let audio_subsystem = sdl_context.audio().unwrap();

        let desired_spec = sdl2::audio::AudioSpecDesired {
            freq: Some(SAMPLE_RATE as i32),
            channels: Some(CHANNELS),
            samples: Some(BLOCK_SIZE as u16),
        };

        let device = audio_subsystem
            .open_playback(None, &desired_spec, |spec| SDLAudioDeviceCallback {
                block_size: spec.samples as BlockSize,
                sample_rate: spec.freq as SampleRate,
                plugin: plugin.clone(),
            })
            .unwrap();

        device.resume();

        Self {
            device: Box::new(device),
        }
    }
}
