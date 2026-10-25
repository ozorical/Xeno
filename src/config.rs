use std::{collections::BTreeMap, fs, path::PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Config {
    pub engine: String,
    pub custom: String,
    pub suggest: bool,
    pub theme: String,
    pub accent: String,
    pub motion: f64,
    pub depth: bool,
    pub scale: f64,
    pub font: String,
    pub corners: u32,
    pub sites: String,
    pub fill: String,
    pub restore: bool,
    pub start: String,
    pub zoom: f64,
    pub line: bool,
    pub scrollbars: bool,
    pub sleep: u32,
    pub discard: u32,
    pub trim: bool,
    pub lite: bool,
    pub gpu: bool,
    pub processes: u32,
    pub tracking: String,
    pub popups: bool,
    pub history: bool,
    pub autofill: bool,
    pub keys: BTreeMap<String, String>,
    pub extensions: BTreeMap<String, String>,
}

pub const KEYS: [(&str, &str); 10] = [
    ("bar", "Alt+A, Ctrl+L"),
    ("switch", "Alt+T"),
    ("switch_back", "Alt+Shift+T"),
    ("new", "Alt+N, Ctrl+T"),
    ("close", "Alt+W, Ctrl+W"),
    ("reopen", "Ctrl+Shift+T"),
    ("settings", "Alt+S"),
    ("next", "Ctrl+Tab"),
    ("prev", "Ctrl+Shift+Tab"),
    ("copy", "Alt+C"),
];

impl Default for Config {
    fn default() -> Self {
        Self {
            engine: "google".into(),
            custom: String::new(),
            suggest: true,
            theme: "dark".into(),
            accent: "#7c9cff".into(),
            motion: 1.0,
            depth: true,
            scale: 1.0,
            font: String::new(),
            corners: 14,
            sites: "system".into(),
            fill: "screen".into(),
            restore: true,
            start: String::new(),
            zoom: 1.0,
            line: true,
            scrollbars: true,
            sleep: 15,
            discard: 120,
            trim: true,
            lite: false,
            gpu: true,
            processes: 0,
            tracking: "balanced".into(),
            popups: true,
            history: true,
            autofill: true,
            keys: KEYS.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            extensions: BTreeMap::new(),
        }
    }
}

pub fn dir() -> PathBuf {
    PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into())).join("Xeno")
}

impl Config {
    pub fn load() -> Self {
        let mut config: Config = fs::read_to_string(dir().join("config.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        for (k, v) in KEYS {
            config.keys.entry(k.into()).or_insert_with(|| v.into());
        }
        config
    }

    pub fn save(&self) {
        let _ = fs::create_dir_all(dir());
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = fs::write(dir().join("config.json"), s);
        }
    }

    pub fn args(&self) -> String {
        let mut args = vec!["--disable-features=SpareRendererForSitePerProcess".to_string()];
        if self.lite {
            args.push("--js-flags=--lite-mode".into());
        }
        if !self.gpu {
            args.push("--disable-gpu".into());
        }
        if self.processes > 0 {
            args.push(format!("--renderer-process-limit={}", self.processes));
        }
        args.join(" ")
    }
}
