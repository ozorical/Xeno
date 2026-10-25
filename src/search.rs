use std::thread;

use serde_json::Value;

use crate::{
    app::{Remote, send},
    config::Config,
    net,
};

pub struct Engine {
    pub id: &'static str,
    pub name: &'static str,
    search: &'static str,
    suggest: &'static str,
}

pub const ENGINES: [Engine; 7] = [
    Engine {
        id: "google",
        name: "Google",
        search: "https://www.google.com/search?q=",
        suggest: "https://suggestqueries.google.com/complete/search?client=firefox&ie=utf-8&oe=utf-8&q=",
    },
    Engine {
        id: "duckduckgo",
        name: "DuckDuckGo",
        search: "https://duckduckgo.com/?q=",
        suggest: "https://duckduckgo.com/ac/?type=list&q=",
    },
    Engine {
        id: "bing",
        name: "Bing",
        search: "https://www.bing.com/search?q=",
        suggest: "https://api.bing.com/osjson.aspx?query=",
    },
    Engine {
        id: "brave",
        name: "Brave",
        search: "https://search.brave.com/search?q=",
        suggest: "https://search.brave.com/api/suggest?q=",
    },
    Engine {
        id: "startpage",
        name: "Startpage",
        search: "https://www.startpage.com/sp/search?query=",
        suggest: "https://www.startpage.com/osuggestions?q=",
    },
    Engine {
        id: "ecosia",
        name: "Ecosia",
        search: "https://www.ecosia.org/search?q=",
        suggest: "https://ac.ecosia.org/autocomplete?type=list&q=",
    },
    Engine {
        id: "kagi",
        name: "Kagi",
        search: "https://kagi.com/search?q=",
        suggest: "https://kagi.com/api/autosuggest?q=",
    },
];

const SCHEMES: [&str; 12] = [
    "http:", "https:", "file:", "about:", "data:", "view-source:", "chrome-extension:", "edge:",
    "mailto:", "ftp:", "blob:", "chrome:",
];

fn engine(config: &Config) -> &'static Engine {
    ENGINES.iter().find(|e| e.id == config.engine).unwrap_or(&ENGINES[0])
}

pub fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn query_url(q: &str, config: &Config) -> String {
    if config.engine == "custom" && config.custom.contains("%s") {
        return config.custom.replace("%s", &encode(q));
    }
    format!("{}{}", engine(config).search, encode(q))
}

pub fn resolve(input: &str, config: &Config) -> String {
    let s = input.trim();
    let lower = s.to_ascii_lowercase();
    if SCHEMES.iter().any(|p| lower.starts_with(p)) {
        return s.to_string();
    }
    if s.contains(char::is_whitespace) {
        return query_url(s, config);
    }
    let host = s.split(['/', '?', '#']).next().unwrap_or_default();
    let name = host.rsplit_once(':').map_or(host, |(h, port)| {
        if port.chars().all(|c| c.is_ascii_digit()) { h } else { host }
    });
    let local = name.eq_ignore_ascii_case("localhost")
        || (name.split('.').count() == 4 && name.split('.').all(|p| p.parse::<u8>().is_ok()));
    if local {
        return format!("http://{s}");
    }
    let labels: Vec<&str> = name.split('.').collect();
    let tld = labels.last().copied().unwrap_or_default();
    let valid = labels.len() > 1
        && labels.iter().all(|l| !l.is_empty() && l.chars().all(|c| c.is_alphanumeric() || c == '-'))
        && tld.len() >= 2
        && tld.chars().all(char::is_alphabetic);
    if valid {
        format!("https://{s}")
    } else {
        query_url(s, config)
    }
}

pub fn suggest(q: String, config: &Config) {
    if !config.suggest || config.engine == "custom" || q.trim().is_empty() {
        return;
    }
    let url = format!("{}{}", engine(config).suggest, encode(&q));
    thread::spawn(move || {
        let list = net::get(&url)
            .and_then(|body| serde_json::from_slice::<Value>(&body).ok())
            .and_then(|v| v.get(1).and_then(Value::as_array).cloned())
            .map(|items| {
                items.iter().filter_map(Value::as_str).take(6).map(str::to_string).collect()
            })
            .unwrap_or_default();
        send(Remote::Suggest(q, list));
    });
}
