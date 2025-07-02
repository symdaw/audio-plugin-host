# Sandboxxed Plugin ID Getter
Simple application for indexing plugins in a separate sandboxxed process.
Takes plugin/module path as an argument then prints plugin details to stdout.
## Usage:
```rs
struct PluginDetails {
    id: String,
    name: String,
    version: String,
    vendor: String,
}

fn try_index_plugin(plugin_path: &str) -> Vec<PluginDetails> {
    let Ok(mut path) = std::env::current_exe() else { return vec![]; };
    path = path.parent().unwrap().to_path_buf();
    path.push("sandboxxed-id-getter");

    let Ok(output) = std::process::Command::new(path)
        .arg(plugin_path)
        .stdout(std::process::Stdio::piped())
        .output() 
    else {
        return vec![];
    };
        
    String::from_utf8_lossy(&output.stdout).lines()
        .filter_map(|l| {
            let parts: Vec<&str> = l.split(',').collect();
            if parts.len() == 4 {
                Some(PluginDetails {
                    id: parts[0].to_string(),
                    name: parts[1].to_string(),
                    version: parts[2].to_string(),
                    vendor: parts[3].to_string(),
                })
            } else {
                None
            }
        })
        .collect()
}
```
