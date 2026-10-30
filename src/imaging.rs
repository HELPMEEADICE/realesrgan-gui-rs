//! Image helpers shared by the processing tasks. This replaces the Pillow usages
//! of the original implementation (`task.py`).

use std::borrow::Cow;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use image::codecs::gif::{GifDecoder, GifEncoder, Repeat};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::codecs::tiff::TiffEncoder;
use image::codecs::webp::WebPEncoder;
use image::{
    AnimationDecoder, Delay, DynamicImage, GrayImage, ImageEncoder, ImageReader, Luma, RgbImage,
    Rgba, RgbaImage,
};

use crate::model::Downsample;

/// Transparency curve copied from the original implementation
/// (Photoshop curve through (209, 182) and (237, 245)).
const ALPHA_LUT: [u8; 256] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
    0x01, 0x01, 0x01, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x03, 0x03, 0x03, 0x03,
    0x03, 0x03, 0x03, 0x04, 0x04, 0x04, 0x04, 0x04, 0x05, 0x05, 0x05, 0x05, 0x05, 0x06, 0x06, 0x06,
    0x06, 0x07, 0x07, 0x07, 0x07, 0x08, 0x08, 0x08, 0x09, 0x09, 0x09, 0x0A, 0x0A, 0x0A, 0x0B, 0x0B,
    0x0C, 0x0C, 0x0C, 0x0D, 0x0D, 0x0E, 0x0E, 0x0F, 0x0F, 0x10, 0x10, 0x10, 0x11, 0x12, 0x12, 0x13,
    0x13, 0x14, 0x14, 0x15, 0x15, 0x16, 0x17, 0x17, 0x18, 0x19, 0x19, 0x1A, 0x1B, 0x1B, 0x1C, 0x1D,
    0x1E, 0x1E, 0x1F, 0x20, 0x21, 0x22, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x29, 0x2A,
    0x2B, 0x2C, 0x2D, 0x2E, 0x2F, 0x30, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3B, 0x3C,
    0x3D, 0x3E, 0x40, 0x41, 0x42, 0x43, 0x45, 0x46, 0x47, 0x49, 0x4A, 0x4C, 0x4D, 0x4F, 0x50, 0x51,
    0x53, 0x55, 0x56, 0x58, 0x59, 0x5B, 0x5C, 0x5E, 0x60, 0x61, 0x63, 0x65, 0x67, 0x68, 0x6A, 0x6C,
    0x6E, 0x70, 0x71, 0x73, 0x75, 0x77, 0x79, 0x7B, 0x7D, 0x7F, 0x81, 0x83, 0x85, 0x87, 0x89, 0x8C,
    0x8E, 0x90, 0x92, 0x94, 0x97, 0x99, 0x9B, 0x9D, 0xA0, 0xA2, 0xA5, 0xA7, 0xA9, 0xAC, 0xAE, 0xB1,
    0xB3, 0xB6, 0xB9, 0xBB, 0xBE, 0xC0, 0xC3, 0xC6, 0xC8, 0xCB, 0xCE, 0xD0, 0xD3, 0xD5, 0xD8, 0xDA,
    0xDC, 0xDF, 0xE1, 0xE3, 0xE5, 0xE8, 0xEA, 0xEB, 0xED, 0xEF, 0xF1, 0xF2, 0xF4, 0xF5, 0xF6, 0xF7,
    0xF8, 0xF9, 0xFA, 0xFB, 0xFB, 0xFC, 0xFC, 0xFD, 0xFD, 0xFE, 0xFE, 0xFE, 0xFE, 0xFF, 0xFF, 0xFF,
];

/// Unique path in the system temporary directory, standing in for
/// `tempfile.mktemp`.
pub fn temp_path(extension: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "regui-{}-{}-{}{}",
        std::process::id(),
        nanos,
        counter,
        extension
    ))
}

pub fn open_image(path: &Path) -> Result<DynamicImage> {
    image::open(path).with_context(|| format!("failed to open {}", path.display()))
}

/// Reads only the image header. The upscale task needs the source size to plan
/// its passes; decoding the whole picture just for `width`/`height` used to
/// cost a full-resolution buffer for every file, which is significant for
/// multi-megapixel inputs.
pub fn image_dimensions(path: &Path) -> Result<(u32, u32)> {
    ImageReader::open(path)
        .with_context(|| format!("failed to open {}", path.display()))?
        .into_dimensions()
        .with_context(|| format!("failed to read the dimensions of {}", path.display()))
}

/// Reads the PNG IHDR colour type. Type 3 is palette based; ncnn based
/// upscalers drop the alpha channel of such files, so they are flattened to
/// RGBA first (the original did the same for PIL's `P` mode).
pub fn is_indexed_png(path: &Path) -> bool {
    if path
        .extension()
        .and_then(|it| it.to_str())
        .map(|it| !it.eq_ignore_ascii_case("png"))
        .unwrap_or(true)
    {
        return false;
    }
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let mut header = [0u8; 26];
    if file.read_exact(&mut header).is_err() {
        return false;
    }
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    header[..8] == SIGNATURE && header[25] == 3
}

pub fn resize_image(
    image: DynamicImage,
    width: u32,
    height: u32,
    downsample: Downsample,
) -> DynamicImage {
    let width = width.max(1);
    let height = height.max(1);
    if image.width() == width && image.height() == height {
        // Hand the decoded buffer back instead of cloning it: when no
        // downsample is needed this used to double the peak memory.
        return image;
    }
    if matches!(downsample, Downsample::Box) {
        // `thumbnail` performs the fast integer box average PIL's BOX filter
        // corresponds to.
        return image.thumbnail_exact(width, height);
    }
    image.resize_exact(width, height, downsample.image_filter())
}

/// Creates the output file with a buffered writer, the way every save path
/// below starts.
fn create_writer(path: &Path) -> Result<BufWriter<File>> {
    let file =
        File::create(path).with_context(|| format!("failed to create {}", path.display()))?;
    Ok(BufWriter::new(file))
}

/// Borrows the RGBA buffer when the image already is RGBA8 and converts only
/// otherwise, avoiding a full-size copy for the common case.
fn rgba_or_owned(image: &DynamicImage) -> Cow<'_, RgbaImage> {
    match image.as_rgba8() {
        Some(buffer) => Cow::Borrowed(buffer),
        None => Cow::Owned(image.to_rgba8()),
    }
}

fn rgb_or_owned(image: &DynamicImage) -> Cow<'_, RgbImage> {
    match image.as_rgb8() {
        Some(buffer) => Cow::Borrowed(buffer),
        None => Cow::Owned(image.to_rgb8()),
    }
}

/// Encodes the decoded buffer in its native colour type. Forcing RGBA first
/// allocated a second full-size buffer (and dropped the alpha-less RGB layout
/// that the upscaler usually returns).
fn write_png<W: Write>(image: &DynamicImage, writer: W) -> Result<()> {
    PngEncoder::new(writer)
        .write_image(
            image.as_bytes(),
            image.width(),
            image.height(),
            image.color().into(),
        )
        .context("failed to encode PNG")
}

/// Saves with the format implied by the file extension, the same way
/// `Image.save()` did.
pub fn save_image(image: &DynamicImage, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).ok();
        }
    }
    let extension = path
        .extension()
        .and_then(|it| it.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    match extension.as_str() {
        "jpg" | "jpeg" => {
            let mut writer = create_writer(path)?;
            let rgb = rgb_or_owned(image);
            JpegEncoder::new_with_quality(&mut writer, 75)
                .write_image(
                    rgb.as_raw(),
                    rgb.width(),
                    rgb.height(),
                    image::ExtendedColorType::Rgb8,
                )
                .context("failed to encode JPEG")?;
            writer.flush()?;
        }
        "webp" => {
            let mut writer = create_writer(path)?;
            let rgba = rgba_or_owned(image);
            WebPEncoder::new_lossless(&mut writer)
                .write_image(
                    rgba.as_raw(),
                    rgba.width(),
                    rgba.height(),
                    image::ExtendedColorType::Rgba8,
                )
                .context("failed to encode lossless WebP")?;
            writer.flush()?;
        }
        "gif" => {
            let mut writer = GifWriter::create(path)?;
            writer.write_frame(image.to_rgba8(), 0)?;
        }
        "tif" | "tiff" => {
            let mut writer = create_writer(path)?;
            let rgba = rgba_or_owned(image);
            TiffEncoder::new(&mut writer)
                .write_image(
                    rgba.as_raw(),
                    rgba.width(),
                    rgba.height(),
                    image::ExtendedColorType::Rgba8,
                )
                .context("failed to encode TIFF")?;
            writer.flush()?;
        }
        _ => {
            let mut writer = create_writer(path)?;
            write_png(image, &mut writer)?;
            writer.flush()?;
        }
    }
    Ok(())
}

/// Lossless WebP or PNG, decided by the path extension — the equivalent of
/// Pillow's `save(..., lossless=True)`.
pub fn save_lossless(image: &DynamicImage, path: &Path) -> Result<()> {
    let extension = path
        .extension()
        .and_then(|it| it.to_str())
        .map(|it| it.to_ascii_lowercase())
        .unwrap_or_default();
    let mut writer = create_writer(path)?;
    match extension.as_str() {
        "webp" => {
            let rgba = rgba_or_owned(image);
            WebPEncoder::new_lossless(&mut writer).write_image(
                rgba.as_raw(),
                rgba.width(),
                rgba.height(),
                image::ExtendedColorType::Rgba8,
            )?;
        }
        _ => write_png(image, &mut writer)?,
    }
    writer.flush()?;
    Ok(())
}

/// Lossy WebP (quality 0-100) through libwebp; Pillow's
/// `save(quality=..., method=6)` equivalent.
pub fn save_lossy_webp(image: &DynamicImage, path: &Path, quality: u8) -> Result<()> {
    // libwebp accepts RGB directly, which avoids the RGBA copy that the old
    // unconditional conversion made for the typical alpha-less upscaled image.
    let encoded = if let Some(rgb) = image.as_rgb8() {
        webp::Encoder::from_rgb(rgb.as_raw(), rgb.width(), rgb.height()).encode(quality as f32)
    } else if let Some(rgba) = image.as_rgba8() {
        webp::Encoder::from_rgba(rgba.as_raw(), rgba.width(), rgba.height()).encode(quality as f32)
    } else {
        let rgba = image.to_rgba8();
        webp::Encoder::from_rgba(rgba.as_raw(), rgba.width(), rgba.height()).encode(quality as f32)
    };
    std::fs::write(path, &*encoded)?;
    Ok(())
}

pub fn save_lossy_jpeg(image: &DynamicImage, path: &Path, quality: u8) -> Result<()> {
    let rgb = rgb_or_owned(image);
    let mut writer = create_writer(path)?;
    JpegEncoder::new_with_quality(&mut writer, quality).write_image(
        rgb.as_raw(),
        rgb.width(),
        rgb.height(),
        image::ExtendedColorType::Rgb8,
    )?;
    writer.flush()?;
    Ok(())
}

/// Decodes one GIF frame at a time and hands it to `handle`. The decoder only
/// keeps the previous frame (needed for disposal), so a long animation no
/// longer has to fit in memory as a whole, unlike the old
/// `collect_frames()` based implementation.
pub fn for_each_gif_frame<F>(path: &Path, mut handle: F) -> Result<()>
where
    F: FnMut(RgbaImage, u16) -> Result<()>,
{
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let decoder = GifDecoder::new(BufReader::new(file)).context("failed to decode GIF")?;

    for frame in decoder.into_frames() {
        let frame = frame.context("failed to decode GIF frame")?;
        let delay = frame.delay();
        let (numerator, denominator) = delay.numer_denom_ms();
        let milliseconds = numerator
            .checked_div(denominator)
            .unwrap_or(0)
            .min(u16::MAX as u32) as u16;
        handle(frame.into_buffer(), milliseconds)?;
    }
    Ok(())
}

/// Incremental GIF writer: each frame is encoded (and its buffer dropped) as
/// soon as it is available, instead of gathering every frame first and cloning
/// it again while encoding.
pub struct GifWriter {
    encoder: GifEncoder<BufWriter<File>>,
}

impl GifWriter {
    pub fn create(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).ok();
            }
        }
        let mut encoder = GifEncoder::new(create_writer(path)?);
        encoder.set_repeat(Repeat::Infinite)?;
        Ok(Self { encoder })
    }

    pub fn write_frame(&mut self, frame: RgbaImage, duration_ms: u16) -> Result<()> {
        let delay = Delay::from_numer_denom_ms(duration_ms as u32, 1);
        self.encoder
            .encode_frame(image::Frame::from_parts(frame, 0, 0, delay))
            .context("failed to encode GIF frame")
    }
}

/// Composites a frame onto white while keeping its own alpha channel, which is
/// what the "optimize GIF transparency" split step does.
pub fn composite_over_white(image: &RgbaImage) -> RgbaImage {
    let mut output = RgbaImage::new(image.width(), image.height());
    for (x, y, pixel) in image.enumerate_pixels() {
        let alpha = pixel[0] as u32;
        let _ = alpha;
        let a = pixel[3] as f32 / 255.0;
        let blend = |channel: u8| -> u8 {
            (channel as f32 * a + 255.0 * (1.0 - a))
                .round()
                .clamp(0.0, 255.0) as u8
        };
        output.put_pixel(
            x,
            y,
            Rgba([blend(pixel[0]), blend(pixel[1]), blend(pixel[2]), pixel[3]]),
        );
    }
    output
}

/// Blurs the alpha channel, applies the transparency curve and reduces it to
/// two levels with Floyd-Steinberg dithering — the merge step of the GIF
/// transparency optimisation.
pub fn optimize_gif_alpha(image: &RgbaImage) -> RgbaImage {
    let (width, height) = image.dimensions();
    let mut alpha = GrayImage::new(width, height);
    for (x, y, pixel) in image.enumerate_pixels() {
        alpha.put_pixel(x, y, Luma([pixel[3]]));
    }

    let blurred = gaussian_blur(&alpha, 3.0);
    let mut curved = GrayImage::new(width, height);
    for (x, y, pixel) in blurred.enumerate_pixels() {
        curved.put_pixel(x, y, Luma([ALPHA_LUT[pixel[0] as usize]]));
    }
    let bilevel = floyd_steinberg_bilevel(&curved);

    let mut output = image.clone();
    for (x, y, pixel) in bilevel.enumerate_pixels() {
        let mut rgba = *output.get_pixel(x, y);
        rgba[3] = pixel[0];
        output.put_pixel(x, y, rgba);
    }
    output
}

/// Separable Gaussian blur with clamped edges, matching Pillow's
/// `ImageFilter.GaussianBlur(radius)`.
pub fn gaussian_blur(image: &GrayImage, sigma: f32) -> GrayImage {
    let radius = (sigma * 3.0).ceil() as i32;
    if radius <= 0 {
        return image.clone();
    }
    let mut kernel: Vec<f32> = Vec::with_capacity((radius * 2 + 1) as usize);
    let mut sum = 0.0f32;
    for offset in -radius..=radius {
        let value = (-(offset as f32).powi(2) / (2.0 * sigma * sigma)).exp();
        kernel.push(value);
        sum += value;
    }
    for value in kernel.iter_mut() {
        *value /= sum;
    }

    let (width, height) = image.dimensions();
    let mut horizontal = GrayImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let mut accumulator = 0.0f32;
            for (index, weight) in kernel.iter().enumerate() {
                let sx = (x as i32 + index as i32 - radius).clamp(0, width as i32 - 1) as u32;
                accumulator += image.get_pixel(sx, y)[0] as f32 * weight;
            }
            horizontal.put_pixel(x, y, Luma([accumulator.round().clamp(0.0, 255.0) as u8]));
        }
    }

    let mut vertical = GrayImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let mut accumulator = 0.0f32;
            for (index, weight) in kernel.iter().enumerate() {
                let sy = (y as i32 + index as i32 - radius).clamp(0, height as i32 - 1) as u32;
                accumulator += horizontal.get_pixel(x, sy)[0] as f32 * weight;
            }
            vertical.put_pixel(x, y, Luma([accumulator.round().clamp(0.0, 255.0) as u8]));
        }
    }
    vertical
}

/// Pillow's `convert("1")` uses Floyd-Steinberg dithering by default, so the
/// same happens here.
pub fn floyd_steinberg_bilevel(image: &GrayImage) -> GrayImage {
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        return image.clone();
    }
    let mut buffer: Vec<f32> = image.pixels().map(|pixel| pixel[0] as f32).collect();
    let mut output = GrayImage::new(width, height);
    let stride = width as usize;

    for y in 0..height as usize {
        for x in 0..width as usize {
            let index = y * stride + x;
            let old = buffer[index];
            let new = if old < 128.0 { 0.0 } else { 255.0 };
            output.put_pixel(x as u32, y as u32, Luma([new as u8]));
            let error = old - new;

            if x + 1 < stride {
                buffer[index + 1] += error * 7.0 / 16.0;
            }
            if y + 1 < height as usize {
                let below = index + stride;
                if x > 0 {
                    buffer[below - 1] += error * 3.0 / 16.0;
                }
                buffer[below] += error * 5.0 / 16.0;
                if x + 1 < stride {
                    buffer[below + 1] += error * 1.0 / 16.0;
                }
            }
        }
    }
    output
}

/// Resizes an image with an explicit filter, used by the pre-upscale step.
pub fn resize_with_filter(
    image: &DynamicImage,
    width: u32,
    height: u32,
    filter: image::imageops::FilterType,
) -> DynamicImage {
    image.resize_exact(width.max(1), height.max(1), filter)
}
