//! Translation handling. Parses the very same `i18n.ini` as the original
//! Python implementation (configparser syntax, comma separated language list in
//! the section header, indented continuation lines).

use std::collections::BTreeMap;
use std::path::Path;

const EMBEDDED_I18N: &str = include_str!("../i18n.ini");

/// Display name -> locale code, copied verbatim from `i18n.py`.
pub const LOCALES_MAP: [(&str, &str); 14] = [
    ("中文 (简体, 中国)", "zh_CN"),
    ("中文 (简体, 新加坡)", "zh_SG"),
    ("中文 (繁體字, 中國香港特別行政區)", "zh_HK"),
    ("中文 (繁體字, 中國澳門特別行政區)", "zh_MO"),
    ("中文 (繁體, 台灣)", "zh_TW"),
    ("English (United States)", "en_US"),
    ("English (United Kingdom)", "en_GB"),
    ("українська (Україна)", "uk_UA"),
    ("Türkçe (Türkiye)", "tr_TR"),
    ("español (España)", "es_ES"),
    ("español (México)", "es_MX"),
    ("español (Colombia)", "es_CO"),
    ("español (Argentina)", "es_AR"),
    ("français (France)", "fr_FR"),
];

const FALLBACK_LANGUAGE: &str = "en_US";

#[derive(Debug, Clone)]
struct Section {
    languages: Vec<String>,
    values: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default)]
pub struct Translations {
    sections: Vec<Section>,
}

impl Translations {
    pub fn parse(text: &str) -> Self {
        let mut sections: Vec<Section> = Vec::new();
        let mut current: Option<Section> = None;
        let mut current_key: Option<String> = None;

        for raw in text.lines() {
            let line = raw.trim_end_matches(['\r', '\n']);
            let trimmed = line.trim();
            if trimmed.is_empty() {
                current_key = None;
                continue;
            }
            if trimmed.starts_with('#') || trimmed.starts_with(';') {
                continue;
            }
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                if let Some(section) = current.take() {
                    sections.push(section);
                }
                let languages = trimmed[1..trimmed.len() - 1]
                    .split(',')
                    .map(|language| language.trim().to_string())
                    .filter(|language| !language.is_empty())
                    .collect();
                current = Some(Section {
                    languages,
                    values: BTreeMap::new(),
                });
                current_key = None;
                continue;
            }
            let Some(section) = current.as_mut() else {
                continue;
            };
            // Indented lines continue the value of the previous key.
            if line.starts_with([' ', '\t']) {
                if let Some(key) = &current_key {
                    if let Some(value) = section.values.get_mut(key) {
                        value.push('\n');
                        value.push_str(trimmed);
                    }
                }
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim().to_string();
            section.values.insert(key.clone(), value.trim().to_string());
            current_key = Some(key);
        }

        if let Some(section) = current.take() {
            sections.push(section);
        }

        Translations { sections }
    }

    fn section(&self, language: &str) -> Option<&Section> {
        self.sections
            .iter()
            .find(|section| section.languages.iter().any(|it| it == language))
    }

    /// Look up a key with the same fallback chain as `i18n.py`: the requested
    /// language, then the English defaults.
    fn lookup(&self, language: &str, key: &str) -> Option<String> {
        self.section(language)
            .and_then(|section| section.values.get(key))
            .or_else(|| {
                self.section(FALLBACK_LANGUAGE)
                    .and_then(|section| section.values.get(key))
            })
            .cloned()
    }
}

#[derive(Debug, Clone)]
pub struct I18n {
    translations: Translations,
    pub all_locales: Vec<String>,
    pub current_language: String,
}

impl I18n {
    pub fn load(base_path: &Path) -> Self {
        let text = std::fs::read_to_string(base_path.join("i18n.ini"))
            .unwrap_or_else(|_| EMBEDDED_I18N.to_string());
        let translations = Translations::parse(&text);

        // Locale codes in the order the file lists them, without duplicates.
        let mut all_locales: Vec<String> = Vec::new();
        for section in &translations.sections {
            for language in &section.languages {
                if !all_locales.contains(language) {
                    all_locales.push(language.clone());
                }
            }
        }

        let mut i18n = I18n {
            translations,
            all_locales,
            current_language: FALLBACK_LANGUAGE.to_string(),
        };
        i18n.set_language(&system_language());
        i18n
    }

    /// Mirrors `i18n.set_current_language`.
    pub fn set_language(&mut self, requested: &str) {
        if self.all_locales.iter().any(|it| it == requested) {
            self.current_language = requested.to_string();
            return;
        }
        let system = system_language();
        if self.all_locales.iter().any(|it| it == &system) {
            self.current_language = system;
        } else {
            self.current_language = FALLBACK_LANGUAGE.to_string();
        }
    }

    pub fn translate(&self, key: &str) -> String {
        self.translations
            .lookup(&self.current_language, key)
            .unwrap_or_else(|| format!("{key}${}", self.current_language))
    }

    /// Index of the current language inside [`LOCALES_MAP`], which is also the
    /// index used by the language combo box.
    pub fn current_locale_display_index(&self) -> i32 {
        LOCALES_MAP
            .iter()
            .position(|(_, code)| *code == self.current_language)
            .map(|index| index as i32)
            .unwrap_or(0)
    }

    pub fn language_code_at(&self, index: i32) -> String {
        LOCALES_MAP
            .get(index.max(0) as usize)
            .map(|(_, code)| code.to_string())
            .unwrap_or_else(|| FALLBACK_LANGUAGE.to_string())
    }
}

/// Locale of the operating system in `xx_YY` form, matching what Python's
/// `locale.getdefaultlocale()` produced.
pub fn system_language() -> String {
    let raw = sys_locale::get_locale().unwrap_or_default();
    let normalized = raw.replace('-', "_");
    if normalized.is_empty() {
        FALLBACK_LANGUAGE.to_string()
    } else {
        normalized
    }
}

/// Substitutes `{0}`, `{1}`, `{2}` and `{1:.03f}` style placeholders, covering
/// the subset of `str.format` used by `i18n.ini`.
pub fn format_message(template: &str, args: &[FormatArg]) -> String {
    let mut out = String::with_capacity(template.len());
    let chars: Vec<char> = template.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '{' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let Some(close) = chars[i + 1..].iter().position(|c| *c == '}') else {
            out.push(chars[i]);
            i += 1;
            continue;
        };
        let close = i + 1 + close;
        let spec: String = chars[i + 1..close].iter().collect();
        // `{{` is not used by i18n.ini, so a plain substitution is enough.
        match render_spec(&spec, args) {
            Some(rendered) => out.push_str(&rendered),
            None => out.push_str(&chars[i..=close].iter().collect::<String>()),
        }
        i = close + 1;
    }
    out
}

#[derive(Debug, Clone)]
pub enum FormatArg {
    Text(String),
    Float(f64),
}

fn render_spec(spec: &str, args: &[FormatArg]) -> Option<String> {
    let (index_part, format_part) = match spec.split_once(':') {
        Some((index, format)) => (index, Some(format)),
        None => (spec, None),
    };
    let index: usize = index_part.trim().parse().ok()?;
    let arg = args.get(index)?;
    match (arg, format_part) {
        (FormatArg::Text(text), _) => Some(text.clone()),
        (FormatArg::Float(value), Some(format)) => Some(format_float(*value, format)),
        (FormatArg::Float(value), None) => Some(format!("{value}")),
    }
}

/// Handles the `:.03f` style specifiers that appear in the toast messages.
fn format_float(value: f64, format: &str) -> String {
    if let Some(precision) = format.strip_prefix('.') {
        if let Some(digits) = precision.strip_suffix('f') {
            if let Ok(digits) = digits.parse::<usize>() {
                return format!("{value:.digits$}");
            }
        }
    }
    format!("{value}")
}
