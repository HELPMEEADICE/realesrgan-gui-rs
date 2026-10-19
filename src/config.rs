//! A minimal INI reader/writer that stays byte-compatible with the
//! `configparser` output of the original Python implementation, so existing
//! `config.ini` files keep working.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

pub const SECTION: &str = "Config";

#[derive(Debug, Default, Clone)]
pub struct Config {
    entries: BTreeMap<String, String>,
}

impl Config {
    pub fn load(path: &Path) -> Self {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        Self::parse(&text)
    }

    pub fn parse(text: &str) -> Self {
        let mut config = Config::default();
        let mut in_section = false;
        let mut current_key: Option<String> = None;

        for raw in text.lines() {
            let line = raw.trim_end_matches(['\r', '\n']);
            if line.trim().is_empty() {
                current_key = None;
                continue;
            }
            let trimmed = line.trim_start();
            if trimmed.starts_with('#') || trimmed.starts_with(';') {
                continue;
            }
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                let name = trimmed[1..trimmed.len() - 1].trim().to_ascii_lowercase();
                in_section = name == SECTION.to_ascii_lowercase();
                current_key = None;
                continue;
            }
            // Indented lines continue the previous value (configparser rule).
            if line.starts_with([' ', '\t']) {
                if in_section {
                    if let Some(key) = &current_key {
                        if let Some(value) = config.entries.get_mut(key) {
                            value.push('\n');
                            value.push_str(trimmed);
                        }
                    }
                }
                continue;
            }
            if !in_section {
                current_key = None;
                continue;
            }
            let (key, value) = match line.split_once('=').or_else(|| line.split_once(':')) {
                Some(pair) => pair,
                None => continue,
            };
            let key = key.trim().to_string();
            config
                .entries
                .insert(key.clone(), value.trim().to_string());
            current_key = Some(key);
        }

        config
    }

    /// Raw value if present and non-empty.
    pub fn get(&self, key: &str) -> Option<String> {
        self.entries
            .get(key)
            .filter(|value| !value.is_empty())
            .cloned()
    }

    pub fn get_or(&self, key: &str, default: &str) -> String {
        self.entries
            .get(key)
            .filter(|value| !value.is_empty())
            .cloned()
            .unwrap_or_else(|| default.to_string())
    }

    pub fn get_int(&self, key: &str, default: i64) -> i64 {
        self.entries
            .get(key)
            .and_then(|value| value.trim().parse::<f64>().ok())
            .map(|value| value as i64)
            .unwrap_or(default)
    }

    pub fn get_bool(&self, key: &str, default: bool) -> bool {
        match self.entries.get(key).map(|value| value.trim().to_ascii_lowercase()) {
            Some(value) => match value.as_str() {
                "1" | "yes" | "true" | "on" => true,
                "0" | "no" | "false" | "off" => false,
                _ => default,
            },
            None => default,
        }
    }

    pub fn set(&mut self, key: &str, value: impl Into<String>) {
        self.entries.insert(key.to_string(), value.into());
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let mut file = std::fs::File::create(path)?;
        writeln!(file, "[{SECTION}]")?;
        // Keep the original key order so diffs against existing config files
        // stay readable.
        for key in KEY_ORDER {
            if let Some(value) = self.entries.get(key) {
                writeln!(file, "{key} = {value}")?;
            }
        }
        for (key, value) in &self.entries {
            if !KEY_ORDER.contains(&key.as_str()) {
                writeln!(file, "{key} = {value}")?;
            }
        }
        file.flush()
    }
}

const KEY_ORDER: [&str; 21] = [
    "Upscaler",
    "ModelDir",
    "ResizeMode",
    "ResizeRatio",
    "ResizeWidth",
    "ResizeHeight",
    "ResizeLongestSide",
    "ResizeShortestSide",
    "Model",
    "DownsampleIndex",
    "GPUID",
    "TileSizeIndex",
    "LossyQuality",
    "UseWebP",
    "UseTTA",
    "OptimizeGIF",
    "LossyMode",
    "IgnoreError",
    "Preupscale",
    "CustomCommand",
    "AppLanguage",
];
