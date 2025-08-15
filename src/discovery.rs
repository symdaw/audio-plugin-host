use std::path::{Path, PathBuf};

use crate::{error::Error, host::Host, load, plugin::PluginInstance, Samples};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct PluginDescriptor {
    pub name: String,
    pub id: String,
    pub path: PathBuf,
    pub version: String,
    pub vendor: String,
    pub format: Format,
    pub initial_latency: Samples,
}

impl PluginDescriptor {
    pub fn load(&self, host: &Host) -> Result<PluginInstance, Error> {
        load(&self.path, &self.id, host)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Format {
    #[default]
    Vst2,
    Vst3,
    Clap,
}

/// Returns any plugin descriptors of plugins in a file. Note that formats such as VST3 allow
/// multiple plugins to be defined in the same file.
pub fn get_descriptor_from_file(path: impl AsRef<Path>) -> Vec<PluginDescriptor> {
    if is_vst2(path.as_ref(), false) {
        let Some(desc) = crate::formats::vst2::get_descriptor(path.as_ref()) else {
            return vec![];
        };

        vec![desc]
    } else if is_vst3(path.as_ref()) {
        crate::formats::vst3::get_descriptor(path.as_ref())
    } else if is_clap(path.as_ref()) {
        crate::formats::clap::get_descriptor(path.as_ref())
    } else {
        vec![]
    }
}

pub fn is_vst2(path: &Path, check_contents: bool) -> bool {
    if !path.exists() {
        return false;
    }

    let ext = path.extension().unwrap_or_default();
    if ext != "dll" && ext != "so" && ext != "vst" {
        return false;
    }

    // if !check_contents {
        return true;
    // }

    #[cfg(target_os = "macos")]
    if path.is_dir() {
        return true;
    }

    let data = std::fs::read(path).expect("Failed to read file");
    match goblin::Object::parse(&data) {
        Ok(goblin::Object::Elf(elf)) => elf.dynstrtab.to_vec().map(|t| t.contains(&"VSTPluginMain")).unwrap_or(true),
        Ok(goblin::Object::PE(pe)) => pe.exports.iter().any(|e| e.name == Some("VSTPluginMain")),
        _ => true, // Can't check so just return true
    }
}

pub fn is_vst3(path: &Path) -> bool {
    let path = path.to_string_lossy().to_lowercase();
    path.ends_with(".vst3")
}

pub fn is_clap(path: &Path) -> bool {
    let path = path.to_string_lossy().to_lowercase();
    path.ends_with(".clap")
}
