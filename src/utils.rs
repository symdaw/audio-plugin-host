use std::path::{Path, PathBuf};

pub fn macos_exec_location(path: impl AsRef<Path>) -> Option<PathBuf> {
    let mut path = path.as_ref().to_path_buf();

    #[cfg(target_os = "macos")]
    {
        if path.is_file() {
            return Some(path);
        }

        path.push("Contents");
        path.push("MacOS");

        if !path.exists() {
            return None;
        }

        path = std::fs::read_dir(&path)
            .ok()?
            .into_iter()
            .next()?
            .ok()?
            .path();
    }

    Some(path)
}
