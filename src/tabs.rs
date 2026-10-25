use std::time::Instant;

use serde_json::{Value, json};
use webview2_com::Microsoft::Web::WebView2::Win32::*;

use crate::{motion::Layer, web::View};

pub struct Tab {
    pub id: u32,
    pub url: String,
    pub title: String,
    pub icon: String,
    pub view: Option<View>,
    pub layer: Layer,
    pub creating: bool,
    pub ready: bool,
    pub popup: Option<(ICoreWebView2NewWindowRequestedEventArgs, ICoreWebView2Deferral)>,
    pub loading: bool,
    pub asleep: bool,
    pub audible: bool,
    pub muted: bool,
    pub used: Instant,
}

impl Tab {
    pub fn new(id: u32, url: String, title: String, layer: Layer) -> Self {
        Self {
            id,
            url,
            title,
            icon: String::new(),
            view: None,
            layer,
            creating: false,
            ready: false,
            popup: None,
            loading: false,
            asleep: false,
            audible: false,
            muted: false,
            used: Instant::now(),
        }
    }

    pub fn label(&self) -> &str {
        if self.title.trim().is_empty() { &self.url } else { &self.title }
    }

    pub fn json(&self) -> Value {
        json!({
            "id": self.id,
            "title": self.label(),
            "url": self.url,
            "icon": self.icon,
            "loading": self.loading,
            "asleep": self.view.is_none() || self.asleep,
            "audible": self.audible,
            "muted": self.muted,
        })
    }
}
