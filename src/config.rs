//! `config.toml`: optional settings, every key defaulting to an English
//! Kindle in Johannesburg. Unknown keys are errors, so a typo never silently
//! falls back to a default.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::kindle::parse::ParseConfig;
use crate::kindle::replay::ReplayConfig;

pub const CONFIG_FILE: &str = "config.toml";

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub kindle: KindleConfig,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KindleConfig {
    /// IANA zone the Kindle's clock is set to.
    pub timezone: Option<String>,
    /// `strptime` formats for the `Added on` date, tried in order.
    pub date_formats: Option<Vec<String>>,
    /// Local month and weekday names to English, e.g. `janvier = "January"`.
    pub date_words: Option<BTreeMap<String, String>>,
    pub keywords: Option<Keywords>,
    pub stub_markers: Option<Vec<String>>,
    pub min_shared_ratio: Option<f64>,
    pub min_shared_chars: Option<usize>,
    pub note_tolerance: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Keywords {
    pub separator: Option<String>,
    pub header_prefix: Option<String>,
    pub highlight: Option<String>,
    pub note: Option<String>,
    pub bookmark: Option<String>,
    pub location: Option<String>,
    pub page: Option<String>,
    pub added_on: Option<String>,
}

impl Config {
    /// Read `explicit` if given; otherwise `config.toml` in the archive if it
    /// exists; otherwise all defaults.
    pub fn load(explicit: Option<&Path>, archive: &Path) -> Result<Config, String> {
        let path = match explicit {
            Some(p) => p.to_path_buf(),
            None => {
                let p = archive.join(CONFIG_FILE);
                if !p.is_file() {
                    return Ok(Config::default());
                }
                p
            }
        };
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        toml::from_str(&text).map_err(|e| format!("{} is not a valid config: {e}", path.display()))
    }

    pub fn parse_config(&self) -> ParseConfig {
        let k = &self.kindle;
        let mut c = ParseConfig::default();
        set(&mut c.timezone, &k.timezone);
        set(&mut c.date_formats, &k.date_formats);
        set(&mut c.stub_markers, &k.stub_markers);
        if let Some(words) = &k.date_words {
            c.date_words = words.iter().map(|(a, b)| (a.clone(), b.clone())).collect();
        }
        if let Some(w) = &k.keywords {
            set(&mut c.separator, &w.separator);
            set(&mut c.header_prefix, &w.header_prefix);
            set(&mut c.highlight, &w.highlight);
            set(&mut c.note, &w.note);
            set(&mut c.bookmark, &w.bookmark);
            set(&mut c.location, &w.location);
            set(&mut c.page, &w.page);
            set(&mut c.added_on, &w.added_on);
        }
        c
    }

    pub fn replay_config(&self) -> ReplayConfig {
        let k = &self.kindle;
        let mut c = ReplayConfig::default();
        set(&mut c.min_shared_ratio, &k.min_shared_ratio);
        set(&mut c.min_shared_chars, &k.min_shared_chars);
        set(&mut c.note_tolerance, &k.note_tolerance);
        c
    }
}

fn set<T: Clone>(target: &mut T, value: &Option<T>) {
    if let Some(v) = value {
        *target = v.clone();
    }
}
