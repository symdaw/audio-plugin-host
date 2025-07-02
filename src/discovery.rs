use std::path::{Path, PathBuf};

use crate::{error::Error, host::Host, load, plugin::PluginInstance, Samples};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
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

#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Format {
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
        todo!()
    } else {
        unimplemented!()
    }
}

pub fn is_vst2(path: &Path, check_contents: bool) -> bool {
    if !path.exists() {
        return false;
    }

    let path = path.to_string_lossy().to_lowercase();
    if !(path.ends_with(".dll") || path.ends_with(".so") || path.ends_with(".vst")) {
        return false;
    }

    if !check_contents {
        return true;
    }

    let data = std::fs::read(path).expect("Failed to read file");
    match goblin::Object::parse(&data) {
        Ok(goblin::Object::Elf(elf)) => elf.syms.iter().any(|s| {
            elf.strtab
                .get(s.st_name)
                .map(|s| s.map(|s| s == "VSTPluginMain").unwrap_or(false))
                .unwrap_or(false)
        }),
        Ok(goblin::Object::PE(pe)) => pe.exports.iter().any(|e| e.name == Some("VSTPluginMain")),
        _ => false,
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
