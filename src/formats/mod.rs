/// cbindgen:ignore
pub(crate) mod clap;

pub(crate) mod vst2;

pub(crate) mod vst3;

use std::path::Path;

use ringbuf::HeapProd;

use crate::discovery::*;
use crate::error::{err, Error};
use crate::event::PluginIssuedEvent;
use crate::host::Host;
use crate::plugin::PluginInner;

pub fn load_any(
    path: &Path,
    id: &str,
    common: Common,
) -> Result<(Box<dyn PluginInner>, PluginDescriptor), Error> {
    if is_vst2(path, true) {
        return vst2::load(path, common);
    }

    if is_vst3(path) {
        return vst3::load(path, id, common);
    }

    if is_clap(path) {
        return clap::load(path, id, common);
    }

    err("The requested path was not a supported plugin format.")
}

/// Common data shared between all plugin formats.
pub struct Common {
    pub host: Host,
    pub plugin_issued_events_producer: HeapProd<PluginIssuedEvent>,
}
