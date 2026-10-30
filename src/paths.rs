//! Filesystem layout of the application. This is the Rust counterpart of the
//! original `define.py`.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const APP_TITLE: &str = "Real-ESRGAN GUI";

/// Candidate upscaler executables, in the same priority order as `define.py`.
pub const UPSCALER_NAMES: [&str; 3] = [
    "upscayl-bin",
    "realesrgan-ncnn-vulkan",
    "realcugan-ncnn-vulkan",
];

pub const IMAGE_EXTENSIONS: [&str; 7] = [".jpg", ".jpeg", ".png", ".gif", ".webp", ".tif", ".tiff"];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UpscalerKind {
    Realesrgan,
    Upscayl,
    Realcugan,
}

fn executable_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// Directory the running executable lives in. The original used `APP_PATH`.
pub fn app_path() -> &'static Path {
    static CACHE: OnceLock<PathBuf> = OnceLock::new();
    CACHE.get_or_init(|| {
        std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf))
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."))
    })
}

/// Directory that holds the data files (`i18n.ini`, icons). When running from a
/// `cargo run` build the executable sits in `target/…`, so fall back to the
/// source checkout.
pub fn base_path() -> &'static Path {
    static CACHE: OnceLock<PathBuf> = OnceLock::new();
    CACHE.get_or_init(|| {
        let app = app_path();
        if app.join("i18n.ini").is_file() {
            return app.to_path_buf();
        }
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        if manifest.join("i18n.ini").is_file() {
            return manifest;
        }
        app.to_path_buf()
    })
}

pub fn config_path() -> PathBuf {
    app_path().join("config.ini")
}

pub fn log_path() -> PathBuf {
    app_path().join("output.log")
}

/// First existing upscaler next to the executable; when none exists the last
/// candidate is returned so that the caller can report a sensible path.
pub fn find_upscaler() -> PathBuf {
    for name in UPSCALER_NAMES {
        let candidate = app_path().join(executable_name(name));
        if candidate.exists() {
            return candidate;
        }
    }
    app_path().join(executable_name(UPSCALER_NAMES[UPSCALER_NAMES.len() - 1]))
}

pub fn upscaler_kind(path: &Path) -> UpscalerKind {
    match path.file_stem().and_then(|s| s.to_str()).unwrap_or("") {
        "upscayl-bin" => UpscalerKind::Upscayl,
        "realcugan-ncnn-vulkan" => UpscalerKind::Realcugan,
        _ => UpscalerKind::Realesrgan,
    }
}

/// Paths handed to the application on the command line.
pub fn initial_input_paths() -> Vec<PathBuf> {
    std::env::args_os().skip(1).map(PathBuf::from).collect()
}

pub fn is_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| IMAGE_EXTENSIONS.contains(&format!(".{}", e.to_ascii_lowercase()).as_str()))
        .unwrap_or(false)
}

pub fn extension_lower(path: &Path) -> String {
    match path.extension().and_then(|e| e.to_str()) {
        Some(e) => format!(".{}", e.to_ascii_lowercase()),
        None => String::new(),
    }
}
