use std::path::PathBuf;

use audio_plugin_host::*;

fn main() {
    let plugin_path = PathBuf::from(std::env::args().nth(1).expect("No plugin path provided"));
    let descriptors = discovery::get_descriptor_from_file(&plugin_path);
    println!(
        "{}",
        descriptors
            .into_iter()
            .map(|d| format!("{},{},{},{}", d.id, d.name, d.vendor, d.version))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
