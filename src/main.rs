// Hide the console window in release builds on Windows.
#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

mod app;
mod config;
mod dragdrop;
mod i18n;
mod imaging;
mod model;
mod paths;
mod platform;
mod task;

slint::include_modules!();

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use app::App;
use config::Config;
use i18n::I18n;
use paths::UpscalerKind;

fn main() {
    if let Err(error) = run() {
        // Release builds have no console attached, so surface a failure in a
        // dialog instead of writing to stderr.
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title(paths::APP_TITLE)
            .set_description(format!("{error:?}"))
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
        #[cfg(debug_assertions)]
        eprintln!("{error:?}");
    }
}

fn run() -> anyhow::Result<()> {
    // The original changed into the application directory so that relative
    // paths and the default model directory resolve the same way.
    let _ = std::env::set_current_dir(paths::app_path());

    let config = Config::load(&paths::config_path());

    let custom_upscaler = config.get("Upscaler");
    let custom_model_dir = config.get("ModelDir");

    let re_path = match &custom_upscaler {
        Some(value) => PathBuf::from(value),
        None => paths::find_upscaler(),
    };
    let re_kind = paths::upscaler_kind(&re_path);

    let model_dir = custom_model_dir
        .clone()
        .map(PathBuf::from)
        .unwrap_or_else(|| paths::app_path().join("models"));

    let mut i18n = I18n::load(paths::base_path());
    match config.get("AppLanguage") {
        Some(language) => i18n.set_language(&language),
        None => i18n.set_language(&i18n::system_language()),
    }

    let (models, model_factors) = discover_models(&model_dir, re_kind);

    platform::init();

    if !re_path.exists() || models.is_empty() {
        platform::open_url("https://github.com/xinntao/Real-ESRGAN/releases");
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Warning)
            .set_title(paths::APP_TITLE)
            .set_description(i18n.translate("WarningNotFoundRE"))
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
        return Ok(());
    }

    let application = Rc::new(App::new(
        config,
        i18n,
        models,
        model_factors,
        model_dir,
        re_path,
        re_kind,
    )?);

    if let Some(dir) = custom_model_dir {
        application.append_log(&format!("Using custom model dir: {dir}\n"));
    }
    if let Some(exe) = custom_upscaler {
        application.append_log(&format!(
            "Using custom upscaler executable: {exe}\nThe executable (and models) may be incompatible with Real-ESRGAN-ncnn-vulkan. Use at your own risk!\n"
        ));
    }

    {
        let weak = Rc::downgrade(&application);
        application
            .window()
            .window()
            .on_close_requested(move || {
                if let Some(application) = weak.upgrade() {
                    application.persist_config();
                }
                slint::CloseRequestResponse::HideWindow
            });
    }

    application.run()?;
    Ok(())
}

/// Enumerates the models in `model_dir`, reproducing
/// `init_config_and_model_paths` including the Real-CUGAN file name scheme.
fn discover_models(
    model_dir: &Path,
    kind: UpscalerKind,
) -> (Vec<String>, HashMap<String, u32>) {
    let mut models: Vec<String> = Vec::new();

    if kind == UpscalerKind::Realcugan {
        let mut directories: Vec<String> = std::fs::read_dir(model_dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| entry.path().is_dir())
            .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
            .collect();
        directories.sort();

        const NOISE_LEVELS: [&str; 5] = [
            "conservative",
            "no-denoise",
            "denoise1x",
            "denoise2x",
            "denoise3x",
        ];
        for name in directories {
            for scale in 2..=4 {
                for noise in NOISE_LEVELS {
                    let base = model_dir
                        .join(&name)
                        .join(format!("up{scale}x-{noise}"));
                    if base.with_extension("bin").exists() && base.with_extension("param").exists() {
                        models.push(format!("{name}#up{scale}x-{noise}"));
                    }
                }
            }
        }
    } else {
        let files: Vec<String> = std::fs::read_dir(model_dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| entry.path().is_file())
            .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
            .collect();
        let present: HashSet<&str> = files.iter().map(String::as_str).collect();
        let mut names: BTreeSet<String> = BTreeSet::new();
        for file in &files {
            let stem = Path::new(file)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("");
            if stem.is_empty() {
                continue;
            }
            if present.contains(format!("{stem}.bin").as_str())
                && present.contains(format!("{stem}.param").as_str())
            {
                names.insert(stem.to_string());
            }
        }
        models = names.into_iter().collect();
    }

    // Keep the bundled models at the top of the list, in this order.
    for name in ["realesrgan-x4plus-anime", "realesrgan-x4plus"] {
        if let Some(index) = models.iter().position(|model| model == name) {
            let model = models.remove(index);
            models.insert(0, model);
        }
    }

    let factors = models
        .iter()
        .map(|model| (model.clone(), model::model_factor_from_name(model)))
        .collect();

    (models, factors)
}
