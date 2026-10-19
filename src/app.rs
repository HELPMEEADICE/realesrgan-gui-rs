//! Wires the Slint UI to the configuration, translations and the processing
//! pipeline. This is the Rust counterpart of the `REGUIApp` class.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use slint::{ComponentHandle, Timer, TimerMode};

use crate::config::Config;
use crate::i18n::{self, FormatArg, I18n};
use crate::imaging;
use crate::model::{Downsample, ReConfigParams, ResizeMode, TILE_SIZES};
use crate::paths::{self, UpscalerKind};
use crate::platform::{self, TaskbarProgress, TaskbarState};
use crate::task::{
    self, CustomCompressTask, LossyCompressTask, OutputFn, PauseFlag, Progress, ReSpawnTask,
    RunnerCallbacks, SplitGifTask, Task, TaskContext,
};
use crate::{AppWindow, Strings};

/// Keeps the log view pinned to its last line.
const LOG_SCROLL_BOTTOM: f32 = -999_999.0;

enum UiMessage {
    Log(String),
    Completed {
        with_error: bool,
        output: String,
        elapsed: f64,
    },
    Failed(String),
    Finished,
}

struct State {
    config: Config,
    i18n: I18n,
    models: Vec<String>,
    model_factors: HashMap<String, u32>,
    model_dir: PathBuf,
    re_path: PathBuf,
    re_kind: UpscalerKind,
    progress: Arc<Progress>,
    pause: Arc<PauseFlag>,
    receiver: Option<Receiver<UiMessage>>,
    log_text: String,
    log_file: Option<Arc<Mutex<std::fs::File>>>,
    processing: bool,
    processing_paused: bool,
    progress_display: f32,
    /// Set while the bar is forced to a value, e.g. 100% when a run completes.
    progress_override: Option<f32>,
    taskbar: Option<TaskbarProgress>,
    started_at: Option<Instant>,
    toast_output: String,
}

pub struct App {
    ui: AppWindow,
    state: Rc<RefCell<State>>,
    _timer: Timer,
}

impl App {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: Config,
        i18n: I18n,
        models: Vec<String>,
        model_factors: HashMap<String, u32>,
        model_dir: PathBuf,
        re_path: PathBuf,
        re_kind: UpscalerKind,
    ) -> anyhow::Result<Self> {
        let ui = AppWindow::new()?;

        let state = Rc::new(RefCell::new(State {
            config,
            i18n,
            models,
            model_factors,
            model_dir,
            re_path,
            re_kind,
            progress: Progress::new(),
            pause: PauseFlag::new(true),
            receiver: None,
            log_text: String::new(),
            log_file: None,
            processing: false,
            processing_paused: false,
            progress_display: 0.0,
            progress_override: None,
            taskbar: None,
            started_at: None,
            toast_output: String::new(),
        }));

        {
            let guard = state.borrow();
            ui.set_models(string_list(&guard.models));
            ui.set_languages(string_list(
                &i18n::LOCALES_MAP
                    .iter()
                    .map(|(name, _)| name.to_string())
                    .collect::<Vec<_>>(),
            ));
            ui.set_downsample_methods(string_list(
                &Downsample::NAMES
                    .iter()
                    .map(|name| name.to_string())
                    .collect::<Vec<_>>(),
            ));
            ui.set_tile_sizes(string_list(&tile_size_labels(&guard.i18n)));
            ui.set_ui_font_family(ui_font_family().into());
            ui.set_about_subtitle(about_subtitle().into());
            ui.set_language_index(guard.i18n.current_locale_display_index());
            apply_config_to_ui(&ui, &guard.config, &guard.models);
            apply_translations(&ui, &guard.i18n);
        }

        // The taskbar progress needs an existing native window.
        let taskbar = TaskbarProgress::new(ui.window());
        state.borrow_mut().taskbar = Some(taskbar);

        install_callbacks(&ui, &state);

        let timer = Timer::default();
        {
            let state = state.clone();
            let weak = ui.as_weak();
            timer.start(TimerMode::Repeated, Duration::from_millis(30), move || {
                if let Some(ui) = weak.upgrade() {
                    tick(&ui, &state);
                }
            });
        }

        let initial_inputs = paths::initial_input_paths();
        if !initial_inputs.is_empty() {
            let models = state.borrow().models.clone();
            set_input_path(&ui, &initial_inputs, &models);
        }

        Ok(App {
            ui,
            state,
            _timer: timer,
        })
    }

    pub fn window(&self) -> &AppWindow {
        &self.ui
    }

    pub fn run(&self) -> Result<(), slint::PlatformError> {
        self.ui.run()
    }

    /// Appends a line to the output view without going through the worker
    /// thread; used for the start-up notices.
    pub fn append_log(&self, text: &str) {
        let mut state = self.state.borrow_mut();
        state.log_text.push_str(text);
        self.ui.set_log_text(state.log_text.clone().into());
        self.ui.set_log_scroll(LOG_SCROLL_BOTTOM);
    }

    /// Reads every setting back out of the UI and writes `config.ini`, exactly
    /// like `REGUIApp.close`.
    pub fn persist_config(&self) {
        let ui = &self.ui;
        let mut state = self.state.borrow_mut();
        let mut config = Config::default();
        config.set("Upscaler", state.config.get_or("Upscaler", ""));
        config.set("ModelDir", state.config.get_or("ModelDir", ""));
        config.set("ResizeMode", ui.get_resize_mode().to_string());
        config.set("ResizeRatio", ui.get_resize_ratio().to_string());
        config.set("ResizeWidth", ui.get_resize_width().to_string());
        config.set("ResizeHeight", ui.get_resize_height().to_string());
        config.set("ResizeLongestSide", ui.get_resize_longest_side().to_string());
        config.set("ResizeShortestSide", ui.get_resize_shortest_side().to_string());
        config.set(
            "Model",
            state
                .models
                .get(ui.get_model_index().max(0) as usize)
                .cloned()
                .unwrap_or_default(),
        );
        config.set("DownsampleIndex", ui.get_downsample_index().to_string());
        config.set("GPUID", ui.get_gpu_id().to_string());
        config.set("TileSizeIndex", ui.get_tile_size_index().to_string());
        config.set("LossyQuality", ui.get_lossy_quality().to_string());
        config.set("UseWebP", bool_text(ui.get_use_webp()));
        config.set("UseTTA", bool_text(ui.get_use_tta()));
        config.set("OptimizeGIF", bool_text(ui.get_optimize_gif()));
        config.set("LossyMode", bool_text(ui.get_lossy_mode()));
        config.set("IgnoreError", bool_text(ui.get_ignore_error()));
        config.set("Preupscale", bool_text(ui.get_preupscale()));
        config.set("CustomCommand", ui.get_custom_command().to_string());
        config.set("AppLanguage", state.i18n.current_language.clone());

        if let Err(error) = config.save(&paths::config_path()) {
            // Release builds have no console, so only report this in debug.
            #[cfg(debug_assertions)]
            eprintln!(
                "failed to write {}: {error}",
                paths::config_path().display()
            );
            let _ = error;
        }
        state.config = config;
    }
}

fn install_callbacks(ui: &AppWindow, state: &Rc<RefCell<State>>) {
    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_browse_input(move || {
            let Some(ui) = weak.upgrade() else { return };
            let picked = rfd::FileDialog::new()
                .add_filter(
                    "Image files",
                    &["jpg", "jpeg", "png", "gif", "webp", "tif", "tiff"],
                )
                .pick_files();
            if let Some(files) = picked {
                if !files.is_empty() {
                    let models = state.borrow().models.clone();
                    set_input_path(&ui, &files, &models);
                }
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_browse_output(move || {
            let Some(ui) = weak.upgrade() else { return };            let picked = rfd::FileDialog::new()
                .add_filter("Image files", &["png", "gif", "webp"])
                .pick_file();
            if let Some(file) = picked {
                let path = file.to_string_lossy().to_string();
                ui.set_output_path(path.into());
                // An explicit choice stops the automatic derivation, matching
                // the `outputPathChanged` flag of the original.
                ui.set_output_auto(false);
            }
        });
    }
    {
        ui.on_open_url(move |url| {
            platform::open_url(url.as_str());
        });
    }
    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_settings_changed(move || {
            let Some(ui) = weak.upgrade() else { return };
            if !ui.get_output_auto() {
                return;
            }
            let models = state.borrow().models.clone();
            let derived = derive_output_path(&ui, &models);
            // Writing a property from Rust never triggers the `edited` handler
            // of the line edit, so `output-auto` is preserved automatically.
            ui.set_output_path(derived.into());
        });
    }
    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_process_clicked(move || {
            let Some(ui) = weak.upgrade() else { return };
            let mut guard = state.borrow_mut();

            if guard.processing {
                if guard.processing_paused {
                    guard.processing_paused = false;
                    guard.pause.set(true);
                } else {
                    guard.processing_paused = true;
                    guard.pause.set(false);
                    push_log(&mut guard, "Will pause after current task is completed.\n");
                }
                let paused = guard.processing_paused;
                drop(guard);
                ui.set_processing(true);
                ui.set_processing_paused(paused);
                return;
            }
            drop(guard);
            start_processing(&ui, &state);
        });
    }
    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_language_changed(move |index| {
            let Some(ui) = weak.upgrade() else { return };
            let i18n = {
                let mut guard = state.borrow_mut();
                let code = guard.i18n.language_code_at(index);
                guard.i18n.set_language(&code);
                guard.i18n.clone()
            };
            apply_translations(&ui, &i18n);
        });
    }
}

// ---------------------------------------------------------------------------
// Background log / progress plumbing
// ---------------------------------------------------------------------------

fn push_log(state: &mut State, text: &str) {
    state.log_text.push_str(text);
    if let Some(file) = &state.log_file {
        if let Ok(mut file) = file.lock() {
            let _ = file.write_all(text.as_bytes());
        }
    }
}

/// Polls the worker thread and updates the UI. Runs on the UI thread.
fn tick(ui: &AppWindow, state: &Rc<RefCell<State>>) {
    let mut guard = state.borrow_mut();
    let mut log_changed = false;
    let mut messages = Vec::new();
    if let Some(receiver) = &guard.receiver {
        while let Ok(message) = receiver.try_recv() {
            messages.push(message);
        }
    }

    for message in messages {
        match message {
            UiMessage::Log(text) => {
                guard.log_text.push_str(&text);
                log_changed = true;
            }
            UiMessage::Completed {
                with_error,
                output,
                elapsed,
            } => {
                guard.progress_override = Some(1.0);
                guard.progress_display = 1.0;
                let i18n = guard.i18n.clone();
                let (title, body) = if with_error {
                    (
                        i18n.translate("ToastCompletedTitle"),
                        i18n::format_message(
                            &i18n.translate("ToastCompletedMessageWithError"),
                            &[FormatArg::Text(
                                paths::log_path().to_string_lossy().to_string(),
                            )],
                        ),
                    )
                } else {
                    (
                        i18n.translate("ToastCompletedTitle"),
                        i18n::format_message(
                            &i18n.translate("ToastCompletedMessage"),
                            &[FormatArg::Text(output), FormatArg::Float(elapsed)],
                        ),
                    )
                };
                platform::notify(&title, &body);
            }
            UiMessage::Failed(message) => {
                let i18n = guard.i18n.clone();
                platform::notify(&i18n.translate("ToastFailedTitle"), &message);
            }
            UiMessage::Finished => {
                guard.processing = false;
                guard.processing_paused = false;
                guard.pause.set(true);
                guard.log_file = None;
                guard.receiver = None;
                ui.set_processing(false);
                ui.set_processing_paused(false);
                if let Some(taskbar) = &guard.taskbar {
                    taskbar.set_state(TaskbarState::NoProgress);
                }
            }
        }
    }

    if guard.processing {
        let target = guard
            .progress_override
            .unwrap_or_else(|| guard.progress.fraction() as f32);
        if (target - guard.progress_display).abs() > 0.0005 {
            guard.progress_display += (target - guard.progress_display) * 0.3;
            let value = guard.progress_display.clamp(0.0, 1.0);
            if let Some(taskbar) = &guard.taskbar {
                taskbar.set_value(round_progress(value), 100);
            }
            ui.set_progress(value);
        }
    }

    if log_changed {
        ui.set_log_text(guard.log_text.clone().into());
        ui.set_log_scroll(LOG_SCROLL_BOTTOM);
    }
}

fn round_progress(value: f32) -> u64 {
    (value.clamp(0.0, 1.0) as f64 * 100.0).round() as u64
}

// ---------------------------------------------------------------------------
// Starting a batch
// ---------------------------------------------------------------------------

fn start_processing(ui: &AppWindow, state: &Rc<RefCell<State>>) {
    let mut guard = state.borrow_mut();
    let i18n = guard.i18n.clone();

    let inputs = split_paths(&ui.get_input_path());
    let outputs = split_paths(&ui.get_output_path());
    if inputs.is_empty() || outputs.is_empty() || inputs.len() != outputs.len() {
        drop(guard);
        warn(&i18n.translate("WarningInvalidPath"));
        return;
    }

    let params = collect_config_params(ui, &guard);
    if params.resize_mode == ResizeMode::Ratio && params.resize_mode_value == 1 {
        drop(guard);
        warn(&i18n.translate("WarningResizeRatio"));
        return;
    }

    let use_webp = ui.get_use_webp();
    let lossy_mode = ui.get_lossy_mode();
    let optimize_gif = ui.get_optimize_gif();
    let lossy_quality = ui.get_lossy_quality().clamp(0, 100) as u8;
    let ignore_error = ui.get_ignore_error();

    guard.progress.reset();
    let mut queue: VecDeque<Box<dyn Task>> = VecDeque::new();
    let mut warning: Option<String> = None;

    for (input, output) in inputs.iter().zip(outputs.iter()) {
        if !input.exists() {
            warning = Some(i18n.translate("WarningNotFoundPath"));
            break;
        }

        if input.is_dir() {
            let mut files = Vec::new();
            collect_files(input, &mut files);
            files.sort();
            for file in files {
                if !paths::is_image_path(&file) {
                    continue;
                }
                let relative = file.strip_prefix(input).unwrap_or(&file);
                let target = output.join(relative);
                for task in make_tasks(
                    &file,
                    &target,
                    &params,
                    use_webp,
                    lossy_mode,
                    optimize_gif,
                    lossy_quality,
                    true,
                ) {
                    queue.push_back(task);
                }
                guard.progress.add_total(1);
            }
            if queue.is_empty() {
                warning = Some(i18n.translate("WarningEmptyFolder"));
                break;
            }
        } else if paths::is_image_path(input) {
            guard.progress.add_total(1);
            for task in make_tasks(
                input,
                output,
                &params,
                use_webp,
                lossy_mode,
                optimize_gif,
                lossy_quality,
                false,
            ) {
                queue.push_back(task);
            }
        } else {
            warning = Some(i18n.translate("WarningInvalidFormat"));
            break;
        }
    }

    if let Some(message) = warning {
        guard.processing = false;
        drop(guard);
        warn(&message);
        return;
    }

    // Reset the UI for the new run.
    guard.log_text.clear();
    guard.progress_display = 0.0;
    guard.progress_override = None;
    guard.processing = true;
    guard.processing_paused = false;
    guard.pause.set(true);
    guard.started_at = Some(Instant::now());
    guard.toast_output = outputs
        .first()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_default();
    let log_file = std::fs::File::create(paths::log_path())
        .ok()
        .map(|file| Arc::new(Mutex::new(file)));
    guard.log_file = log_file.clone();

    let (sender, receiver) = channel::<UiMessage>();
    guard.receiver = Some(receiver);

    let progress = guard.progress.clone();
    let pause = guard.pause.clone();
    let re_path = guard.re_path.clone();
    let re_kind = guard.re_kind;
    let output_for_toast = guard.toast_output.clone();
    let started_at = guard.started_at.unwrap_or_else(Instant::now);
    let taskbar = guard.taskbar.as_ref().map(|_| ());
    drop(guard);

    ui.set_log_text(String::new().into());
    ui.set_progress(0.0);
    ui.set_processing(true);
    ui.set_processing_paused(false);
    if taskbar.is_some() {
        if let Some(taskbar) = &state.borrow().taskbar {
            taskbar.set_state(TaskbarState::Normal);
            // A value of "almost zero" is what makes the bar appear; an exact 0
            // keeps it invisible.
            taskbar.set_value(1, u32::MAX as u64);
        }
    }

    let output: OutputFn = {
        let sender: Sender<UiMessage> = sender.clone();
        let log_file = log_file.clone();
        Arc::new(move |text: &str| {
            if let Some(file) = &log_file {
                if let Ok(mut file) = file.lock() {
                    let _ = file.write_all(text.as_bytes());
                }
            }
            let _ = sender.send(UiMessage::Log(text.to_string()));
        })
    };

    let context = TaskContext {
        out: output,
        progress,
        re_path,
        re_kind,
    };

    std::thread::Builder::new()
        .name("regui-worker".to_string())
        .spawn(move || {
            let mut on_complete = |with_error: bool| {
                let _ = sender.send(UiMessage::Completed {
                    with_error,
                    output: output_for_toast.clone(),
                    elapsed: started_at.elapsed().as_secs_f64(),
                });
            };
            let mut on_error = |error: &anyhow::Error| {
                let _ = sender.send(UiMessage::Failed(format!("{error:#}")));
            };
            let mut on_finished = || {
                let _ = sender.send(UiMessage::Finished);
            };
            let mut callbacks = RunnerCallbacks {
                on_complete: &mut on_complete,
                on_error: &mut on_error,
                on_finished: &mut on_finished,
            };
            task::run_queue(queue, pause, &context, ignore_error, &mut callbacks);
        })
        .expect("failed to spawn the worker thread");
}

/// Builds the task (or task pair) for a single input file, mirroring the
/// branches of `buttonProcess_click`.
///
/// `from_directory` must be true when the file comes from a folder scan: the
/// original chose the output container from the branch it was in, not from the
/// shape of the path.
#[allow(clippy::too_many_arguments)]
fn make_tasks(
    input: &Path,
    output: &Path,
    params: &ReConfigParams,
    use_webp: bool,
    lossy_mode: bool,
    optimize_gif: bool,
    lossy_quality: u8,
    from_directory: bool,
) -> Vec<Box<dyn Task>> {
    let extension = paths::extension_lower(input);
    let output_extension = paths::extension_lower(output);

    if extension == ".gif" {
        return vec![Box::new(SplitGifTask::new(
            input.to_path_buf(),
            output.to_path_buf(),
            params.clone(),
            optimize_gif,
        ))];
    }

    if !params.custom_command.is_empty() {
        let intermediate = imaging::temp_path(".png");
        let target = if from_directory {
            task::with_extension(output, if use_webp { ".webp" } else { ".png" })
        } else {
            output.to_path_buf()
        };
        return vec![
            Box::new(ReSpawnTask::new(
                input.to_path_buf(),
                intermediate.clone(),
                params.clone(),
                false,
            )),
            Box::new(CustomCompressTask::new(
                intermediate,
                target,
                params.custom_command.clone(),
                true,
            )),
        ];
    }

    // A single file only goes through lossy compression when its output is a
    // container that supports it; a folder run always derives one.
    let lossy_applies = if from_directory {
        true
    } else {
        matches!(output_extension.as_str(), ".jpg" | ".jpeg" | ".webp")
    };
    if lossy_mode && lossy_applies {
        let intermediate = imaging::temp_path(".webp");
        let target = if from_directory {
            task::with_extension(output, if use_webp { ".webp" } else { ".jpg" })
        } else {
            output.to_path_buf()
        };
        return vec![
            Box::new(ReSpawnTask::new(
                input.to_path_buf(),
                intermediate.clone(),
                params.clone(),
                false,
            )),
            Box::new(LossyCompressTask::new(intermediate, target, lossy_quality, true)),
        ];
    }

    let target = if from_directory {
        task::with_extension(output, if use_webp { ".webp" } else { ".png" })
    } else {
        output.to_path_buf()
    };
    vec![Box::new(ReSpawnTask::new(
        input.to_path_buf(),
        target,
        params.clone(),
        false,
    ))]
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn collect_files(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn split_paths(value: &slint::SharedString) -> Vec<PathBuf> {
    value
        .split('|')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(PathBuf::from)
        .collect()
}

fn string_list(values: &[String]) -> slint::ModelRc<slint::SharedString> {
    slint::ModelRc::new(slint::VecModel::from(
        values
            .iter()
            .map(|value| slint::SharedString::from(value.as_str()))
            .collect::<Vec<_>>(),
    ))
}

fn tile_size_labels(i18n: &I18n) -> Vec<String> {
    std::iter::once(i18n.translate("TileSizeAuto"))
        .chain(TILE_SIZES[1..].iter().map(|size| size.to_string()))
        .collect()
}

fn bool_text(value: bool) -> &'static str {
    if value {
        "True"
    } else {
        "False"
    }
}

fn warn(message: &str) {
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Warning)
        .set_title(paths::APP_TITLE)
        .set_description(message)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

fn ui_font_family() -> &'static str {
    if cfg!(windows) {
        "Microsoft YaHei UI"
    } else if cfg!(target_os = "macos") {
        "PingFang SC"
    } else {
        "Noto Sans CJK SC"
    }
}

fn about_subtitle() -> String {
    match option_env!("REGUI_BUILD_TIME") {
        Some(value) if !value.trim().is_empty() => {
            format!("By TransparentLC\nBuilt at {}", value.trim())
        }
        _ => "By TransparentLC".to_string(),
    }
}

fn apply_config_to_ui(ui: &AppWindow, config: &Config, models: &[String]) {
    ui.set_resize_mode(config.get_int("ResizeMode", 1) as i32);
    ui.set_resize_ratio(config.get_int("ResizeRatio", 4) as i32);
    ui.set_resize_width(config.get_int("ResizeWidth", 1024) as i32);
    ui.set_resize_height(config.get_int("ResizeHeight", 1024) as i32);
    ui.set_resize_longest_side(config.get_int("ResizeLongestSide", 1024) as i32);
    ui.set_resize_shortest_side(config.get_int("ResizeShortestSide", 1024) as i32);
    ui.set_downsample_index(
        config
            .get_int("DownsampleIndex", 0)
            .clamp(0, Downsample::ALL.len() as i64 - 1) as i32,
    );
    ui.set_tile_size_index(
        config
            .get_int("TileSizeIndex", 0)
            .clamp(0, TILE_SIZES.len() as i64 - 1) as i32,
    );
    ui.set_gpu_id(config.get_int("GPUID", -1).clamp(-1, 7) as i32);
    ui.set_lossy_quality(config.get_int("LossyQuality", 80).clamp(0, 100) as i32);
    ui.set_custom_command(config.get_or("CustomCommand", "").into());
    ui.set_use_webp(config.get_bool("UseWebP", false));
    ui.set_use_tta(config.get_bool("UseTTA", false));
    ui.set_optimize_gif(config.get_bool("OptimizeGIF", false));
    ui.set_lossy_mode(config.get_bool("LossyMode", false));
    ui.set_ignore_error(config.get_bool("IgnoreError", false));
    ui.set_preupscale(config.get_bool("Preupscale", false));

    let model = config.get_or("Model", "");
    let index = models
        .iter()
        .position(|candidate| *candidate == model)
        .unwrap_or(0);
    ui.set_model_index(index as i32);
}

fn apply_translations(ui: &AppWindow, i18n: &I18n) {
    let strings = ui.global::<Strings>();
    {
        let set = |key: &str, setter: &dyn Fn(slint::SharedString)| {
            setter(i18n.translate(key).into());
        };
        set("Input", &|value| strings.set_input_path_label(value));
        set("Output", &|value| strings.set_output_path_label(value));
        set("OpenFileDialog", &|value| strings.set_browse(value));
        set("UsedModel", &|value| strings.set_used_model(value));
        set("ResizeMode", &|value| strings.set_resize_mode(value));
        set("ResizeModeRatio", &|value| strings.set_resize_mode_ratio(value));
        set("ResizeModeWidth", &|value| strings.set_resize_mode_width(value));
        set("ResizeModeHeight", &|value| strings.set_resize_mode_height(value));
        set("ResizeModeLongestSide", &|value| {
            strings.set_resize_mode_longest_side(value)
        });
        set("ResizeModeShortestSide", &|value| {
            strings.set_resize_mode_shortest_side(value)
        });
        set("DownsampleMode", &|value| strings.set_downsample_mode(value));
        set("TileSize", &|value| strings.set_tile_size(value));
        set("UsedGPUID", &|value| strings.set_used_gpu_id(value));
        set("LossyModeQuality", &|value| {
            strings.set_lossy_mode_quality(value)
        });
        set("CustomCommand", &|value| strings.set_custom_command(value));
        set("PreferWebP", &|value| strings.set_prefer_webp(value));
        set("EnableTTA", &|value| strings.set_enable_tta(value));
        set("GIFOptimizeTransparency", &|value| {
            strings.set_gif_optimize_transparency(value)
        });
        set("EnableLossyMode", &|value| strings.set_enable_lossy_mode(value));
        set("EnableIgnoreError", &|value| {
            strings.set_enable_ignore_error(value)
        });
        set("EnablePreupscale", &|value| strings.set_enable_preupscale(value));
        set("ViewREGUISource", &|value| strings.set_view_source(value));
        set("ViewRESource", &|value| strings.set_view_re_source(value));
        set("ViewAdditionalModel", &|value| {
            strings.set_view_additional_model(value)
        });
        set("ViewDonatePage", &|value| strings.set_view_donate_page(value));
        set("FrameBasicConfig", &|value| strings.set_tab_basic(value));
        set("FrameAdvancedConfig", &|value| strings.set_tab_advanced(value));
        set("FrameAbout", &|value| strings.set_tab_about(value));
        set("StartProcessing", &|value| strings.set_start_processing(value));
        set("PauseProcessing", &|value| strings.set_pause_processing(value));
        set("ContinueProcessing", &|value| {
            strings.set_continue_processing(value)
        });
    }

    // The "auto" entry of the tile size combo is translated as well.
    ui.set_tile_sizes(string_list(&tile_size_labels(i18n)));
    ui.set_language_index(i18n.current_locale_display_index());
}

fn set_input_path(ui: &AppWindow, input_paths: &[PathBuf], models: &[String]) {
    let joined = input_paths
        .iter()
        .map(|path| path.to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join(" | ");
    ui.set_input_path(joined.into());
    let derived = derive_output_path(ui, models);
    ui.set_output_path(derived.into());
    ui.set_output_auto(true);
}

/// Reimplementation of `getOutputPath`.
fn derive_output_path(ui: &AppWindow, models: &[String]) -> String {
    let input = ui.get_input_path();
    let use_webp = ui.get_use_webp();
    let custom_command = ui.get_custom_command().trim().to_string();
    let model = models
        .get(ui.get_model_index().max(0) as usize)
        .cloned()
        .unwrap_or_default();

    let suffix = match ResizeMode::from_int(ui.get_resize_mode()) {
        ResizeMode::Ratio => format!("x{}", ui.get_resize_ratio()),
        ResizeMode::Width => format!("w{}", ui.get_resize_width()),
        ResizeMode::Height => format!("h{}", ui.get_resize_height()),
        ResizeMode::LongestSide => format!("l{}", ui.get_resize_longest_side()),
        ResizeMode::ShortestSide => format!("s{}", ui.get_resize_shortest_side()),
    };

    let mut results = Vec::new();
    for part in input
        .split('|')
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
        let path = PathBuf::from(part);
        let (base, mut extension) = if path.is_dir() {
            (path.to_string_lossy().to_string(), String::new())
        } else {
            // Split like `os.path.splitext`, keeping the original separators.
            let text = path.to_string_lossy();
            let (stem, extension) = task::split_extension_text(&text);
            (stem.to_string(), extension.to_ascii_lowercase())
        };

        if matches!(extension.as_str(), ".jpg" | ".tif" | ".tiff") || !custom_command.is_empty() {
            extension = ".png".to_string();
        }
        if extension == ".png" && use_webp {
            extension = ".webp".to_string();
        }

        results.push(format!("{base} ({model} {suffix}){extension}"));
    }
    results.join(" | ")
}

fn collect_config_params(ui: &AppWindow, state: &State) -> ReConfigParams {
    let resize_mode = ResizeMode::from_int(ui.get_resize_mode());
    let resize_mode_value = match resize_mode {
        ResizeMode::Ratio => ui.get_resize_ratio() as i64,
        ResizeMode::Width => ui.get_resize_width() as i64,
        ResizeMode::Height => ui.get_resize_height() as i64,
        ResizeMode::LongestSide => ui.get_resize_longest_side() as i64,
        ResizeMode::ShortestSide => ui.get_resize_shortest_side() as i64,
    };
    let model = state
        .models
        .get(ui.get_model_index().max(0) as usize)
        .cloned()
        .unwrap_or_default();
    let model_factor = state.model_factors.get(&model).copied().unwrap_or(4);

    ReConfigParams {
        model,
        model_factor,
        model_dir: state.model_dir.clone(),
        resize_mode,
        resize_mode_value,
        downsample: Downsample::from_index(ui.get_downsample_index()),
        tile_size: TILE_SIZES
            .get(ui.get_tile_size_index().max(0) as usize)
            .copied()
            .unwrap_or(0),
        gpu_id: ui.get_gpu_id(),
        use_tta: ui.get_use_tta(),
        preupscale: ui.get_preupscale(),
        custom_command: ui.get_custom_command().trim().to_string(),
    }
}
