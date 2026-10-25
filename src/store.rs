use std::{
    cmp::Reverse,
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::config::dir;

#[derive(Serialize, Deserialize, Clone)]
pub struct Visit {
    pub url: String,
    pub title: String,
    pub visits: u32,
    pub last: u64,
}

#[derive(Default)]
pub struct History {
    items: Vec<Visit>,
    dirty: bool,
}

#[derive(Serialize, Deserialize, Default)]
pub struct Session {
    pub tabs: Vec<Saved>,
    pub active: usize,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Saved {
    pub url: String,
    pub title: String,
}

fn read<T: DeserializeOwned>(name: &str) -> Option<T> {
    serde_json::from_str(&fs::read_to_string(dir().join(name)).ok()?).ok()
}

fn write<T: Serialize>(name: &str, value: &T) {
    let _ = fs::create_dir_all(dir());
    if let Ok(s) = serde_json::to_string(value) {
        let _ = fs::write(dir().join(name), s);
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn bare(url: &str) -> &str {
    let s = url.split_once("://").map_or(url, |(_, rest)| rest);
    s.strip_prefix("www.").unwrap_or(s)
}

impl History {
    pub fn load() -> Self {
        Self { items: read("history.json").unwrap_or_default(), dirty: false }
    }

    pub fn save(&mut self) {
        if self.dirty {
            write("history.json", &self.items);
            self.dirty = false;
        }
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.dirty = true;
        self.save();
    }

    pub fn visit(&mut self, url: &str, title: &str) {
        if !url.starts_with("http") {
            return;
        }
        match self.items.iter_mut().find(|v| v.url == url) {
            Some(v) => {
                v.visits += 1;
                v.last = now();
                if !title.is_empty() {
                    v.title = title.into();
                }
            }
            None => self.items.push(Visit { url: url.into(), title: title.into(), visits: 1, last: now() }),
        }
        if self.items.len() > 5000 {
            self.items.sort_by_key(|v| Reverse(v.last));
            self.items.truncate(4000);
        }
        self.dirty = true;
    }

    pub fn title(&mut self, url: &str, title: &str) {
        if let Some(v) = self.items.iter_mut().find(|v| v.url == url) {
            if v.title != title && !title.is_empty() {
                v.title = title.into();
                self.dirty = true;
            }
        }
    }

    pub fn search(&self, q: &str, n: usize) -> Vec<&Visit> {
        let q = q.trim().to_lowercase();
        let words: Vec<&str> = q.split_whitespace().collect();
        if words.is_empty() {
            return Vec::new();
        }
        let t = now();
        let mut hits: Vec<(f64, &Visit)> = self
            .items
            .iter()
            .filter_map(|v| {
                let hay = format!("{} {}", bare(&v.url), v.title).to_lowercase();
                words.iter().all(|w| hay.contains(w)).then(|| {
                    let days = t.saturating_sub(v.last) as f64 / 86400.0 + 1.0;
                    let lead = if bare(&v.url).to_lowercase().starts_with(&q) { 6.0 } else { 0.0 };
                    (lead + (v.visits as f64).ln_1p() * 2.0 - days.ln(), v)
                })
            })
            .collect();
        hits.sort_by(|a, b| b.0.total_cmp(&a.0));
        hits.into_iter().take(n).map(|(_, v)| v).collect()
    }

    pub fn complete(&self, q: &str) -> Option<String> {
        let q = q.trim().to_lowercase();
        if q.is_empty() || q.contains(' ') {
            return None;
        }
        let url = self
            .items
            .iter()
            .filter(|v| bare(&v.url).to_lowercase().starts_with(&q))
            .max_by_key(|v| (v.visits, v.last))?;
        let s = bare(&url.url).to_lowercase();
        let end = s.get(q.len()..)?.find('/').map_or(s.len(), |i| q.len() + i + 1);
        s.get(..end).map(str::to_string)
    }
}

impl Session {
    pub fn load() -> Self {
        read("session.json").unwrap_or_default()
    }

    pub fn save(&self) {
        write("session.json", self);
    }
}
