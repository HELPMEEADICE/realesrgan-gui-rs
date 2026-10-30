//! Configuration value types, the Rust counterpart of `param.py`.

use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResizeMode {
    Ratio = 1,
    Width = 2,
    Height = 3,
    LongestSide = 4,
    ShortestSide = 5,
}

impl ResizeMode {
    pub fn from_int(value: i32) -> Self {
        match value {
            2 => ResizeMode::Width,
            3 => ResizeMode::Height,
            4 => ResizeMode::LongestSide,
            5 => ResizeMode::ShortestSide,
            _ => ResizeMode::Ratio,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Downsample {
    Lanczos,
    Bicubic,
    Hamming,
    Bilinear,
    Box,
    Nearest,
}

impl Downsample {
    pub const ALL: [Downsample; 6] = [
        Downsample::Lanczos,
        Downsample::Bicubic,
        Downsample::Hamming,
        Downsample::Bilinear,
        Downsample::Box,
        Downsample::Nearest,
    ];

    /// Names shown in the combo box, matching PIL's `Image.Resampling` names.
    pub const NAMES: [&'static str; 6] = [
        "Lanczos", "Bicubic", "Hamming", "Bilinear", "Box", "Nearest",
    ];

    pub fn from_index(index: i32) -> Self {
        Downsample::ALL
            .get(index.max(0) as usize)
            .copied()
            .unwrap_or(Downsample::Lanczos)
    }

    pub fn image_filter(self) -> image::imageops::FilterType {
        use image::imageops::FilterType;
        match self {
            // PIL's Hamming and Bilinear both reduce to a triangle kernel in
            // the `image` crate, which has no Hamming filter of its own.
            Downsample::Lanczos => FilterType::Lanczos3,
            Downsample::Bicubic => FilterType::CatmullRom,
            Downsample::Hamming | Downsample::Bilinear | Downsample::Box => FilterType::Triangle,
            Downsample::Nearest => FilterType::Nearest,
        }
    }
}

/// Tile sizes offered in the advanced tab; index 0 means "auto".
pub const TILE_SIZES: [u32; 9] = [0, 32, 64, 128, 256, 512, 1024, 2048, 4096];

#[derive(Clone, Debug)]
pub struct ReConfigParams {
    pub model: String,
    pub model_factor: u32,
    pub model_dir: PathBuf,
    pub resize_mode: ResizeMode,
    pub resize_mode_value: i64,
    pub downsample: Downsample,
    pub tile_size: u32,
    pub gpu_id: i32,
    pub use_tta: bool,
    pub preupscale: bool,
    pub custom_command: String,
}

/// Determines the scale factor encoded in a model file name, defaulting to 4
/// exactly like the original regex `(\d+)x|x(\d+)`.
pub fn model_factor_from_name(name: &str) -> u32 {
    let bytes: Vec<char> = name.chars().collect();
    for (index, ch) in bytes.iter().enumerate() {
        if *ch != 'x' && *ch != 'X' {
            continue;
        }
        // `(\d+)x` — digits immediately before the x.
        let mut start = index;
        while start > 0 && bytes[start - 1].is_ascii_digit() {
            start -= 1;
        }
        if start < index {
            let digits: String = bytes[start..index].iter().collect();
            if let Ok(value) = digits.parse::<u32>() {
                return value;
            }
        }
        // `x(\d+)` — digits immediately after the x.
        let mut end = index + 1;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
        if end > index + 1 {
            let digits: String = bytes[index + 1..end].iter().collect();
            if let Ok(value) = digits.parse::<u32>() {
                return value;
            }
        }
    }
    4
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_model_factor() {
        assert_eq!(model_factor_from_name("realesrgan-x4plus"), 4);
        assert_eq!(model_factor_from_name("realesr-animevideov3"), 4);
        assert_eq!(model_factor_from_name("realesrgan-x2plus"), 2);
        assert_eq!(model_factor_from_name("4x-UltraSharp"), 4);
        assert_eq!(model_factor_from_name("2x-Anime"), 2);
        assert_eq!(model_factor_from_name("remacri"), 4);
    }
}
