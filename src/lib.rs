#![doc = include_str!("../README.md")]

pub mod audio_bus;
pub mod discovery;
pub mod error;
pub mod event;
pub mod host;
pub mod parameter;
pub mod plugin;

pub use plugin::load;

mod formats;
pub mod heapless_vec;

///////////////////// Unsorted

pub type SampleRate = usize;
pub type BlockSize = usize;
pub type Tempo = f64;
pub type PpqTime = f64;
pub type Samples = usize;

#[repr(C)]
#[derive(Clone)]
pub struct ProcessDetails {
    pub sample_rate: SampleRate,
    pub block_size: BlockSize,
    pub tempo: Tempo,
    pub player_time: PpqTime,
    pub time_signature_numerator: usize,
    pub time_signature_denominator: usize,
    pub cycle_enabled: bool,
    pub cycle_start: PpqTime,
    pub cycle_end: PpqTime,
    pub playing_state: PlayingState,
    pub bar_start_pos: PpqTime,
    pub nanos: f64,
}

impl Default for ProcessDetails {
    fn default() -> Self {
        ProcessDetails {
            sample_rate: 44100,
            block_size: 512,
            tempo: 120.0,
            player_time: 0.0,
            time_signature_numerator: 4,
            time_signature_denominator: 4,
            cycle_enabled: false,
            cycle_start: 0.0,
            cycle_end: 0.0,
            playing_state: PlayingState::Stopped,
            bar_start_pos: 0.0,
            nanos: 0.0,
        }
    }
}

#[derive(Default, PartialEq, Eq, Clone, Copy, Debug)]
#[repr(u8)]
pub enum PlayingState {
    #[default]
    Stopped,
    Playing,
    Recording,
    OfflineRendering,
}

impl PlayingState {
    pub fn is_playing(&self) -> bool {
        match self {
            PlayingState::Stopped => false,
            PlayingState::Playing => true,
            PlayingState::Recording => true,
            PlayingState::OfflineRendering => true,
        }
    }
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
#[repr(u8)]
pub enum WindowIDType {
    HWND, 
    XWNDX11,
    XWNDWayland,
    NSView,
    Other,
}

impl WindowIDType {
    pub fn this_platform() -> Self {
        #[cfg(target_os = "windows")]
        {
            WindowIDType::HWND
        }
        #[cfg(target_os = "linux")]
        {
            // TODO: Check Wayland
            WindowIDType::XWNDX11
        }
        #[cfg(target_os = "macos")]
        {
            WindowIDType::NSView
        }
    }
}

///////////////////////
