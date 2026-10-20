//! The processing pipeline: the Rust counterpart of `task.py`.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

use anyhow::{anyhow, Context, Result};
use image::imageops::FilterType;

use crate::imaging;
use crate::model::{ReConfigParams, ResizeMode};
use crate::paths::{self, UpscalerKind};

/// `[current file progress, completed files, total files]` of the original.
#[derive(Debug, Default)]
pub struct Progress {
    current: AtomicU64,
    done: AtomicUsize,
    total: AtomicUsize,
}

impl Progress {
    pub fn new() -> Arc<Self> {
        Arc::new(Progress::default())
    }

    pub fn set_current(&self, value: f64) {
        self.current.store(value.to_bits(), Ordering::Relaxed);
    }

    pub fn current(&self) -> f64 {
        f64::from_bits(self.current.load(Ordering::Relaxed))
    }

    pub fn done(&self) -> usize {
        self.done.load(Ordering::Relaxed)
    }

    pub fn total(&self) -> usize {
        self.total.load(Ordering::Relaxed)
    }

    pub fn add_done(&self, amount: usize) {
        self.done.fetch_add(amount, Ordering::Relaxed);
    }

    pub fn add_total(&self, amount: usize) {
        self.total.fetch_add(amount, Ordering::Relaxed);
    }

    pub fn sub_total(&self, amount: usize) {
        // Equivalent to `fetch_update`/`try_update` with a saturating subtraction,
        // but written as an explicit CAS loop: the method was renamed in newer
        // rustc versions (deprecating the old name), and this form compiles
        // unchanged on the older toolchains the project still supports.
        let mut current = self.total.load(Ordering::Relaxed);
        loop {
            match self.total.compare_exchange_weak(
                current,
                current.saturating_sub(amount),
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
    }

    pub fn reset(&self) {
        self.set_current(0.0);
        self.done.store(0, Ordering::Relaxed);
        self.total.store(0, Ordering::Relaxed);
    }

    /// `(current + done) / total`, clamped to `0.0..=1.0`.
    pub fn fraction(&self) -> f64 {
        let total = self.total();
        if total == 0 {
            return 0.0;
        }
        ((self.current() + self.done() as f64) / total as f64).clamp(0.0, 1.0)
    }
}

/// Stand-in for `threading.Event`: the worker blocks while it is cleared.
#[derive(Debug)]
pub struct PauseFlag {
    running: Mutex<bool>,
    signal: Condvar,
}

impl PauseFlag {
    pub fn new(running: bool) -> Arc<Self> {
        Arc::new(PauseFlag {
            running: Mutex::new(running),
            signal: Condvar::new(),
        })
    }

    pub fn set(&self, running: bool) {
        let mut guard = self.running.lock().unwrap_or_else(|error| error.into_inner());
        *guard = running;
        drop(guard);
        self.signal.notify_all();
    }

    pub fn wait(&self) {
        let mut guard = self.running.lock().unwrap_or_else(|error| error.into_inner());
        while !*guard {
            guard = self
                .signal
                .wait(guard)
                .unwrap_or_else(|error| error.into_inner());
        }
    }

}

pub type OutputFn = Arc<dyn Fn(&str) + Send + Sync>;

pub struct TaskContext {
    pub out: OutputFn,
    pub progress: Arc<Progress>,
    pub re_path: PathBuf,
    pub re_kind: UpscalerKind,
}

/// A unit of work. The returned tasks are pushed to the front of the queue,
/// which is how the GIF split step injects one upscale task per frame plus the
/// merge step.
pub trait Task: Send {
    fn run(&mut self, ctx: &TaskContext) -> Result<Vec<Box<dyn Task>>>;
}

pub struct RunnerCallbacks<'a> {
    pub on_complete: &'a mut dyn FnMut(bool),
    pub on_error: &'a mut dyn FnMut(&anyhow::Error),
    pub on_finished: &'a mut dyn FnMut(),
}

/// The Rust counterpart of `taskRunner`.
pub fn run_queue(
    mut queue: VecDeque<Box<dyn Task>>,
    pause: Arc<PauseFlag>,
    ctx: &TaskContext,
    ignore_error: bool,
    callbacks: &mut RunnerCallbacks<'_>,
) {
    let mut counter = 0usize;
    let mut with_error = false;

    while !queue.is_empty() {
        pause.wait();
        let started = Instant::now();
        let mut task = queue.pop_front().expect("queue is not empty");
        match task.run(ctx) {
            Ok(extra) => {
                for (index, extra_task) in extra.into_iter().enumerate() {
                    queue.insert(index, extra_task);
                }
                (ctx.out)(&format!(
                    "Task #{counter} completed in {}ms.\n",
                    started.elapsed().as_millis()
                ));
                counter += 1;
            }
            Err(error) => {
                with_error = true;
                (ctx.out)(&format!("{error:?}\n"));
                (callbacks.on_error)(&error);
                if !ignore_error {
                    (callbacks.on_finished)();
                    return;
                }
            }
        }
    }

    (callbacks.on_complete)(with_error);
    (callbacks.on_finished)();
}

// ---------------------------------------------------------------------------
// Upscaling
// ---------------------------------------------------------------------------

pub struct ReSpawnTask {
    input: PathBuf,
    output: PathBuf,
    config: ReConfigParams,
    remove_input: bool,
}

impl ReSpawnTask {
    pub fn new(
        input: PathBuf,
        output: PathBuf,
        config: ReConfigParams,
        remove_input: bool,
    ) -> Self {
        ReSpawnTask {
            input,
            output,
            config,
            remove_input,
        }
    }

    fn build_command(&self, ctx: &TaskContext, input: &Path, output: &Path) -> Result<Vec<String>> {
        let mut command = vec![
            ctx.re_path.to_string_lossy().to_string(),
            "-v".to_string(),
            "-i".to_string(),
            input.to_string_lossy().to_string(),
            "-o".to_string(),
            output.to_string_lossy().to_string(),
            "-s".to_string(),
            self.config.model_factor.to_string(),
        ];

        match ctx.re_kind {
            UpscalerKind::Realcugan => {
                // Model names use the "<dir>#up<scale>x-<noise>" form, mirroring
                // the file names Real-CUGAN expects.
                let (model, model_file) = self.config.model.split_once('#').ok_or_else(|| {
                    anyhow!(
                        "Real-CUGAN model name must look like <dir>#up<scale>x-<noise>, got {}",
                        self.config.model
                    )
                })?;
                let noise = model_file
                    .split_once('-')
                    .map(|(_, rest)| rest)
                    .unwrap_or(model_file);
                let denoise_level = match noise {
                    "conservative" => -1,
                    "no-denoise" => 0,
                    _ => {
                        let level = noise
                            .strip_prefix("denoise")
                            .and_then(|rest| rest.strip_suffix('x'))
                            .and_then(|digits| digits.parse::<i32>().ok());
                        level.ok_or_else(|| anyhow!("unsupported Real-CUGAN noise level: {noise}"))?
                    }
                };
                command.extend([
                    "-t".to_string(),
                    self.config.tile_size.to_string(),
                    "-m".to_string(),
                    self.config
                        .model_dir
                        .join(model)
                        .to_string_lossy()
                        .to_string(),
                    "-n".to_string(),
                    denoise_level.to_string(),
                    "-g".to_string(),
                    gpu_argument(self.config.gpu_id),
                    "-c".to_string(),
                    "1".to_string(),
                ]);
            }
            UpscalerKind::Upscayl | UpscalerKind::Realesrgan => {
                if ctx.re_kind == UpscalerKind::Upscayl {
                    command.extend(["-z".to_string(), self.config.model_factor.to_string()]);
                }
                command.extend([
                    "-t".to_string(),
                    self.config.tile_size.to_string(),
                    "-n".to_string(),
                    self.config.model.clone(),
                    "-g".to_string(),
                    gpu_argument(self.config.gpu_id),
                ]);
            }
        }

        if self.config.use_tta {
            command.push("-x".to_string());
        }
        Ok(command)
    }

    /// Runs one upscaler pass and returns the path the upscaler reported when it
    /// renamed its output because of an alpha channel.
    fn run_upscaler(
        &self,
        ctx: &TaskContext,
        input: &Path,
        output: &Path,
        index: usize,
        steps: usize,
    ) -> Result<Option<PathBuf>> {
        let command = self.build_command(ctx, input, output)?;
        let mut process = Command::new(&command[0]);
        process
            .args(&command[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            process.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = process
            .spawn()
            .with_context(|| format!("failed to start {}", command[0]))?;
        let stderr = child.stderr.take().expect("stderr was piped");
        let mut reader = BufReader::new(stderr);
        let mut buffer = Vec::new();
        let mut alpha_override: Option<PathBuf> = None;

        loop {
            buffer.clear();
            let read = reader
                .read_until(b'\n', &mut buffer)
                .context("failed to read upscaler output")?;
            if read == 0 {
                break;
            }
            let line = String::from_utf8_lossy(&buffer)
                .trim_end_matches(['\r', '\n'])
                .to_string();

            // Order matters: the original used `if/elif`.
            if let Some(path) = parse_alpha_override(&line) {
                alpha_override = Some(PathBuf::from(path));
            } else if let Some(percent) = parse_percentage(&line) {
                ctx.progress
                    .set_current((index as f64 + percent / 100.0) / steps as f64);
            } else if line.contains(" -> ") && line.ends_with(" done") {
                ctx.progress
                    .set_current((index as f64 + 1.0) / steps as f64);
            }

            (ctx.out)(&format!("{line}\n"));
        }

        let status = child.wait().context("failed to wait for the upscaler")?;
        if !status.success() {
            return Err(anyhow!(
                "command returned {}: {}",
                status,
                command.join(" ")
            ));
        }
        Ok(alpha_override)
    }
}

impl Task for ReSpawnTask {
    fn run(&mut self, ctx: &TaskContext) -> Result<Vec<Box<dyn Task>>> {
        (ctx.out)(&format!(
            "Using executable: {}\n",
            ctx.re_path.display()
        ));
        ctx.progress.set_current(0.0);

        let mut input = self.input.clone();
        let mut remove_input = self.remove_input;
        // Only the header is read here. Decoding the whole image just to learn
        // its size used to cost a full-resolution buffer before any work began.
        let (mut source_width, mut source_height) = imaging::image_dimensions(&input)?;

        // Palette based PNG files lose their alpha channel inside the upscaler,
        // so they are flattened to RGBA first.
        if imaging::is_indexed_png(&input) {
            let image = imaging::open_image(&input)?;
            let flattened = imaging::temp_path(".png");
            imaging::save_lossless(&image, &flattened)?;
            input = flattened;
            remove_input = true;
        }

        let mut resize_mode = self.config.resize_mode;
        if (resize_mode == ResizeMode::LongestSide && source_width >= source_height)
            || (resize_mode == ResizeMode::ShortestSide && source_width <= source_height)
        {
            resize_mode = ResizeMode::Width;
        } else if (resize_mode == ResizeMode::LongestSide && source_height >= source_width)
            || (resize_mode == ResizeMode::ShortestSide && source_height <= source_width)
        {
            resize_mode = ResizeMode::Height;
        }

        let value = self.config.resize_mode_value.max(0) as f64;
        let source_ratio = source_width as f64 / source_height as f64;
        let (destination_width, destination_height) = match resize_mode {
            ResizeMode::Ratio => (
                (source_width as f64 * value) as u32,
                (source_height as f64 * value) as u32,
            ),
            ResizeMode::Width => {
                let width = value as u32;
                (width, (width as f64 / source_ratio).round() as u32)
            }
            ResizeMode::Height => {
                let height = value as u32;
                ((height as f64 * source_ratio).round() as u32, height)
            }
            ResizeMode::LongestSide | ResizeMode::ShortestSide => (
                (source_width as f64 * value) as u32,
                (source_height as f64 * value) as u32,
            ),
        };

        let model_factor = self.config.model_factor.max(1) as f64;
        let mut preupscaled: Option<PathBuf> = None;
        if self.config.preupscale {
            let scale_ratio = match resize_mode {
                ResizeMode::Ratio => value,
                ResizeMode::Width => value / source_width as f64,
                ResizeMode::Height => value / source_height as f64,
                ResizeMode::LongestSide | ResizeMode::ShortestSide => value,
            };
            let logarithm = scale_ratio.log(model_factor);
            let integral = logarithm.trunc();
            let fraction = logarithm.fract();
            let pre_width = (destination_width as f64 / model_factor.powf(integral)).ceil() as u32;
            let pre_height =
                (destination_height as f64 / model_factor.powf(integral)).ceil() as u32;

            if fraction < 0.5 && (source_width != pre_width || source_height != pre_height) {
                (ctx.out)(&format!(
                    "Pre-upscale from {source_width}x{source_height} to {pre_width}x{pre_height}.\n"
                ));
                let extension = if paths::extension_lower(&input) == ".webp" {
                    ".webp"
                } else {
                    ".png"
                };
                let path = imaging::temp_path(extension);
                let image = imaging::open_image(&input)?;
                let resized = imaging::resize_with_filter(
                    &image,
                    pre_width,
                    pre_height,
                    FilterType::Lanczos3,
                );
                imaging::save_lossless(&resized, &path)?;
                preupscaled = Some(path);
                source_width = pre_width;
                source_height = pre_height;
            }
        }

        let mut scale_pass = 0usize;
        while source_width < destination_width && source_height < destination_height {
            scale_pass += 1;
            source_width *= self.config.model_factor.max(1);
            source_height *= self.config.model_factor.max(1);
        }

        // input -> output, or input -> temp0 -> ... -> output
        let output_extension = paths::extension_lower(&self.output);
        let mut files: Vec<PathBuf> = Vec::with_capacity(scale_pass + 1);
        files.push(
            preupscaled
                .clone()
                .unwrap_or_else(|| input.clone()),
        );
        for _ in 0..scale_pass {
            files.push(imaging::temp_path(&output_extension));
        }

        let steps = files.len().saturating_sub(1);
        for index in 0..steps {
            let step_input = files[index].clone();
            let step_output = files[index + 1].clone();
            let alpha_override = self.run_upscaler(ctx, &step_input, &step_output, index, steps)?;

            if index > 0 || Some(&files[index]) == preupscaled.as_ref() || remove_input {
                let _ = std::fs::remove_file(&files[index]);
            }
            if let Some(alpha_path) = alpha_override {
                let _ = move_file(&alpha_path, &step_output);
                (ctx.out)(&format!(
                    "Rename {} to {}\n",
                    alpha_path.display(),
                    step_output.display()
                ));
            }
        }

        if let Some(parent) = self.output.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).ok();
            }
        }

        let last = files.last().cloned().unwrap_or_else(|| input.clone());
        if source_width == destination_width && source_height == destination_height {
            if self.output.exists() {
                let _ = std::fs::remove_file(&self.output);
            }
            if files.len() == 1 {
                // Nothing was upscaled, do not consume the source file.
                std::fs::copy(&last, &self.output).with_context(|| {
                    format!("failed to copy {} to {}", last.display(), self.output.display())
                })?;
            } else {
                move_file(&last, &self.output).with_context(|| {
                    format!("failed to move {} to {}", last.display(), self.output.display())
                })?;
            }
        } else {
            // The upscaler output is the largest buffer the application ever
            // holds; encode it straight from the decoded colour type and let
            // `resize_image` reuse the buffer when no resize is needed.
            let resized_image = imaging::open_image(&last)?;
            (ctx.out)(&format!(
                "Downsample from {}x{} to {destination_width}x{destination_height}.\n",
                resized_image.width(),
                resized_image.height()
            ));
            let resized = imaging::resize_image(
                resized_image,
                destination_width,
                destination_height,
                self.config.downsample,
            );
            imaging::save_image(&resized, &self.output)?;
            if scale_pass > 0 {
                let _ = std::fs::remove_file(&last);
            }
        }

        ctx.progress.set_current(0.0);
        ctx.progress.add_done(1);
        Ok(Vec::new())
    }
}

// ---------------------------------------------------------------------------
// GIF handling
// ---------------------------------------------------------------------------

pub struct MergeGifTask {
    output: PathBuf,
    frames: Vec<PathBuf>,
    durations: Vec<u16>,
    optimize_transparency: bool,
}

impl MergeGifTask {
    pub fn new(
        output: PathBuf,
        frames: Vec<PathBuf>,
        durations: Vec<u16>,
        optimize_transparency: bool,
    ) -> Self {
        MergeGifTask {
            output,
            frames,
            durations,
            optimize_transparency,
        }
    }
}

impl Task for MergeGifTask {
    fn run(&mut self, ctx: &TaskContext) -> Result<Vec<Box<dyn Task>>> {
        (ctx.out)(&format!(
            "Merging {} frames to {}\n",
            self.frames.len(),
            self.output.display()
        ));
        if self.frames.is_empty() {
            return Err(anyhow!("no frames to merge"));
        }

        // Encode while decoding: keeping every upscaled frame in a Vec used to
        // need `frame_count * width * height * 4` bytes, twice over while the
        // encoder cloned each buffer.
        let mut writer = imaging::GifWriter::create(&self.output)?;
        for (index, frame) in self.frames.iter().enumerate() {
            let mut image = imaging::open_image(frame)?.into_rgba8();
            if self.optimize_transparency {
                image = imaging::optimize_gif_alpha(&image);
            }
            let _ = std::fs::remove_file(frame);
            let duration = self.durations.get(index).copied().unwrap_or(0);
            writer.write_frame(image, duration)?;
        }
        Ok(Vec::new())
    }
}

pub struct SplitGifTask {
    input: PathBuf,
    output: PathBuf,
    config: ReConfigParams,
    optimize_transparency: bool,
}

impl SplitGifTask {
    pub fn new(
        input: PathBuf,
        output: PathBuf,
        config: ReConfigParams,
        optimize_transparency: bool,
    ) -> Self {
        SplitGifTask {
            input,
            output,
            config,
            optimize_transparency,
        }
    }
}

impl Task for SplitGifTask {
    fn run(&mut self, ctx: &TaskContext) -> Result<Vec<Box<dyn Task>>> {
        let extension = if self.optimize_transparency {
            ".png"
        } else {
            ".webp"
        };

        let mut tasks: Vec<Box<dyn Task>> = Vec::new();
        let mut merged_frames: Vec<PathBuf> = Vec::new();
        let mut durations: Vec<u16> = Vec::new();
        let mut index = 0usize;

        // Frames are written out as they are decoded. Previously the complete
        // animation was decoded into a Vec first, so peak memory was
        // proportional to the frame count.
        imaging::for_each_gif_frame(&self.input, |frame, duration| {
            let frame_source = imaging::temp_path(extension);
            let frame_destination = imaging::temp_path(extension);

            if self.optimize_transparency {
                let composited = imaging::composite_over_white(&frame);
                imaging::save_lossless(&image::DynamicImage::ImageRgba8(composited), &frame_source)?;
            } else {
                imaging::save_lossless(&image::DynamicImage::ImageRgba8(frame), &frame_source)?;
            }

            (ctx.out)(&format!(
                "Frame #{index}: {} -> {} Duration: {duration}\n",
                frame_source.display(),
                frame_destination.display()
            ));

            merged_frames.push(frame_destination.clone());
            durations.push(duration);
            tasks.push(Box::new(ReSpawnTask::new(
                frame_source,
                frame_destination,
                self.config.clone(),
                true,
            )));
            ctx.progress.add_total(1);
            index += 1;
            Ok(())
        })?;

        // The caller already counted the GIF itself as one task.
        ctx.progress.sub_total(1);

        if self.config.custom_command.is_empty() {
            tasks.push(Box::new(MergeGifTask::new(
                self.output.clone(),
                merged_frames,
                durations,
                self.optimize_transparency,
            )));
        } else {
            let intermediate = imaging::temp_path(".gif");
            tasks.push(Box::new(MergeGifTask::new(
                intermediate.clone(),
                merged_frames,
                durations,
                self.optimize_transparency,
            )));
            tasks.push(Box::new(CustomCompressTask::new(
                intermediate,
                self.output.clone(),
                self.config.custom_command.clone(),
                true,
            )));
        }

        Ok(tasks)
    }
}

// ---------------------------------------------------------------------------
// Post processing
// ---------------------------------------------------------------------------

pub struct LossyCompressTask {
    input: PathBuf,
    output: PathBuf,
    quality: u8,
    remove_input: bool,
}

impl LossyCompressTask {
    pub fn new(input: PathBuf, output: PathBuf, quality: u8, remove_input: bool) -> Self {
        LossyCompressTask {
            input,
            output,
            quality,
            remove_input,
        }
    }
}

impl Task for LossyCompressTask {
    fn run(&mut self, ctx: &TaskContext) -> Result<Vec<Box<dyn Task>>> {
        (ctx.out)(&format!(
            "Compressing {} to {} with quality {}\n",
            self.input.display(),
            self.output.display(),
            self.quality
        ));
        if let Some(parent) = self.output.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).ok();
            }
        }

        let mut image = imaging::open_image(&self.input)?;
        match paths::extension_lower(&self.output).as_str() {
            ".webp" => imaging::save_lossy_webp(&image, &self.output, self.quality)?,
            ".jpg" | ".jpeg" => {
                if image.color().has_alpha() {
                    (ctx.out)("Discarding alpha channel to compress the RGBA image to JPEG\n");
                    image = image::DynamicImage::ImageRgb8(image.to_rgb8());
                }
                imaging::save_lossy_jpeg(&image, &self.output, self.quality)?;
            }
            extension => {
                return Err(anyhow!("unsupported lossy compression target {extension}"));
            }
        }

        if self.remove_input {
            let _ = std::fs::remove_file(&self.input);
        }
        Ok(Vec::new())
    }
}

pub struct CustomCompressTask {
    input: PathBuf,
    output: PathBuf,
    command_template: String,
    remove_input: bool,
}

impl CustomCompressTask {
    pub fn new(
        input: PathBuf,
        output: PathBuf,
        command_template: String,
        remove_input: bool,
    ) -> Self {
        CustomCompressTask {
            input,
            output,
            command_template,
            remove_input,
        }
    }
}

impl Task for CustomCompressTask {
    fn run(&mut self, ctx: &TaskContext) -> Result<Vec<Box<dyn Task>>> {
        let command = expand_command(&self.command_template, &self.input, &self.output);
        if command.is_empty() {
            return Err(anyhow!("custom command is empty"));
        }
        (ctx.out)(&format!(
            "Compressing {} with command: {}\n",
            self.input.display(),
            command.join(" ")
        ));
        if let Some(parent) = self.output.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).ok();
            }
        }

        let mut process = Command::new(&command[0]);
        process
            .args(&command[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            process.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = process
            .spawn()
            .with_context(|| format!("failed to start {}", command[0]))?;
        let stderr = child.stderr.take().expect("stderr was piped");
        let mut reader = BufReader::new(stderr);
        let mut buffer = Vec::new();
        loop {
            buffer.clear();
            let read = reader.read_until(b'\n', &mut buffer)?;
            if read == 0 {
                break;
            }
            let line = String::from_utf8_lossy(&buffer);
            (ctx.out)(&line);
        }

        let status = child.wait()?;
        if !status.success() {
            return Err(anyhow!(
                "command returned {}: {}",
                status,
                command.join(" ")
            ));
        }

        if self.remove_input {
            let _ = std::fs::remove_file(&self.input);
        }
        Ok(Vec::new())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn gpu_argument(gpu_id: i32) -> String {
    if gpu_id < 0 {
        "auto".to_string()
    } else {
        gpu_id.to_string()
    }
}

fn move_file(from: &Path, to: &Path) -> Result<()> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    std::fs::copy(from, to)
        .with_context(|| format!("failed to copy {} to {}", from.display(), to.display()))?;
    let _ = std::fs::remove_file(from);
    Ok(())
}

/// `image <path> has alpha channel ! <path> will output <path>`
fn parse_alpha_override(line: &str) -> Option<String> {
    if !line.starts_with("image ") || !line.contains(" has alpha channel ! ") {
        return None;
    }
    let marker = " will output ";
    let index = line.find(marker)?;
    Some(line[index + marker.len()..].to_string())
}

/// `(\d+[.,]\d+)%`
fn parse_percentage(line: &str) -> Option<f64> {
    let percent = line.find('%')?;
    let bytes = line.as_bytes();
    let mut start = percent;
    while start > 0 {
        let character = bytes[start - 1];
        if character.is_ascii_digit() || character == b'.' || character == b',' {
            start -= 1;
        } else {
            break;
        }
    }
    if start == percent {
        return None;
    }
    let number = line[start..percent].replace(',', ".");
    // A bare integer has no decimal separator and must not match, matching the
    // original `\d+[.,]\d+` pattern.
    if !number.contains('.') {
        return None;
    }
    number.parse::<f64>().ok()
}

/// Splits a path into (stem, extension) the way Python's `os.path.splitext`
/// does: the extension includes the dot, and a leading dot inside the file name
/// is not treated as the start of an extension.
pub(crate) fn split_extension_text(text: &str) -> (&str, &str) {
    let after_separator = text.rfind(['/', '\\']).map(|index| index + 1).unwrap_or(0);
    match text.rfind('.') {
        Some(dot) if dot > after_separator => (&text[..dot], &text[dot..]),
        _ => (text, ""),
    }
}

/// Expands `{input}`, `{output}` and `{output:ext}` in a custom command.
fn expand_command(template: &str, input: &Path, output: &Path) -> Vec<String> {
    let parts = shlex::split(template).unwrap_or_default();
    parts
        .into_iter()
        .map(|part| {
            if part == "{input}" {
                return input.to_string_lossy().to_string();
            }
            if part == "{output}" {
                return output.to_string_lossy().to_string();
            }
            if let Some(extension) = part
                .strip_prefix("{output:")
                .and_then(|rest| rest.strip_suffix('}'))
            {
                let text = output.to_string_lossy();
                let (stem, _) = split_extension_text(&text);
                return format!("{stem}.{extension}");
            }
            part
        })
        .collect()
}

/// Replaces the extension of a path, matching
/// `os.path.splitext(path)[0] + extension` from the original implementation.
pub fn with_extension(path: &Path, extension: &str) -> PathBuf {
    let text = path.to_string_lossy();
    let (stem, _) = split_extension_text(&text);
    PathBuf::from(format!("{stem}{extension}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_percentage_lines() {
        assert_eq!(parse_percentage("12.34%"), Some(12.34));
        assert_eq!(parse_percentage("12,34%"), Some(12.34));
        assert_eq!(parse_percentage("12%"), None);
        assert_eq!(parse_percentage("no progress here"), None);
    }

    #[test]
    fn parses_alpha_override() {
        assert_eq!(
            parse_alpha_override("image foo.png has alpha channel ! foo.png will output foo.webp"),
            Some("foo.webp".to_string())
        );
        assert_eq!(parse_alpha_override("image foo.png done"), None);
    }

    #[test]
    fn expands_custom_command() {
        let expanded = expand_command(
            "cwebp -q 80 {input} -o {output}",
            Path::new("/tmp/a.png"),
            Path::new("/tmp/b.webp"),
        );
        assert_eq!(expanded[3], "/tmp/a.png");
        assert_eq!(expanded[5], "/tmp/b.webp");

        let with_extension = expand_command(
            "ffmpeg -i {input} {output:mp4}",
            Path::new("/tmp/a.gif"),
            Path::new("/tmp/b.gif"),
        );
        assert_eq!(with_extension[3], "/tmp/b.mp4");
    }

    #[test]
    fn splits_extensions_like_splitext() {
        assert_eq!(split_extension_text("a/b.png"), ("a/b", ".png"));
        assert_eq!(split_extension_text("a.tar.gz"), ("a.tar", ".gz"));
        assert_eq!(split_extension_text(".bashrc"), (".bashrc", ""));
        assert_eq!(split_extension_text(r"C:\dir\file"), (r"C:\dir\file", ""));
        assert_eq!(split_extension_text(r"C:\dir\file.JPG"), (r"C:\dir\file", ".JPG"));

        assert_eq!(
            with_extension(Path::new("dir/name.jpg"), ".png"),
            PathBuf::from("dir/name.png")
        );
        assert_eq!(
            with_extension(Path::new("no-extension"), ".webp"),
            PathBuf::from("no-extension.webp")
        );
    }
}
