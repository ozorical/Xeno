use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    thread,
};

use serde_json::Value;
use webview2_com::{
    BrowserExtensionEnableCompletedHandler, BrowserExtensionRemoveCompletedHandler,
    Microsoft::Web::WebView2::Win32::*, ProfileAddBrowserExtensionCompletedHandler,
    ProfileGetBrowserExtensionsCompletedHandler,
};
use windows::core::*;

use crate::{
    app::{Event, Remote, emit, send},
    config::dir,
    net,
};

pub fn store_id(input: &str) -> Option<String> {
    input
        .split(|c: char| !c.is_ascii_lowercase())
        .find(|w| w.len() == 32 && w.bytes().all(|b| (b'a'..=b'p').contains(&b)))
        .map(str::to_string)
}

pub fn folder(id: &str) -> PathBuf {
    dir().join("Extensions").join(id)
}

pub fn fetch(id: String, version: String) {
    thread::spawn(move || {
        let url = format!(
            "https://clients2.google.com/service/update2/crx?response=redirect&prodversion={version}&acceptformat=crx2,crx3&x=id%3D{id}%26uc"
        );
        let result = match net::get(&url) {
            None => Err("Couldn't download that extension".to_string()),
            Some(crx) => unpack(&crx, &folder(&id))
                .map(|p| p.display().to_string())
                .ok_or_else(|| "That extension couldn't be unpacked".to_string()),
        };
        send(Remote::Unpacked(result));
    });
}

fn unpack(crx: &[u8], to: &Path) -> Option<PathBuf> {
    if crx.get(..4)? != b"Cr24" {
        return None;
    }
    let word = |i: usize| crx.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
    let start = match word(4)? {
        3 => 12 + word(8)?,
        2 => 16 + word(8)? + word(12)?,
        _ => return None,
    };
    let mut zip = zip::ZipArchive::new(Cursor::new(crx.get(start..)?)).ok()?;
    let _ = fs::remove_dir_all(to);
    zip.extract(to).ok()?;
    let _ = fs::remove_dir_all(to.join("_metadata"));
    Some(to.to_path_buf())
}

pub fn pages(path: &str) -> (String, String) {
    let v: Value = fs::read_to_string(Path::new(path).join("manifest.json"))
        .ok()
        .and_then(|s| serde_json::from_str(s.trim_start_matches('\u{feff}')).ok())
        .unwrap_or_default();
    let popup = ["action", "browser_action", "page_action"]
        .iter()
        .find_map(|k| v[k]["default_popup"].as_str())
        .unwrap_or_default();
    let options = v["options_ui"]["page"].as_str().or(v["options_page"].as_str()).unwrap_or_default();
    (popup.to_string(), options.to_string())
}

pub fn add(profile: &ICoreWebView2Profile7, path: String) -> Result<()> {
    let folder = HSTRING::from(&path);
    let handler = ProfileAddBrowserExtensionCompletedHandler::create(Box::new(move |hr, ext| {
        emit(Event::Added(path, hr.ok().and(ext)));
        Ok(())
    }));
    unsafe { profile.AddBrowserExtension(&folder, &handler) }
}

pub fn list(profile: &ICoreWebView2Profile7) -> Result<()> {
    let handler = ProfileGetBrowserExtensionsCompletedHandler::create(Box::new(|hr, list| {
        hr?;
        let mut items = Vec::new();
        if let Some(list) = list {
            let mut count = 0;
            unsafe { list.Count(&mut count)? };
            items.extend((0..count).filter_map(|i| unsafe { list.GetValueAtIndex(i) }.ok()));
        }
        emit(Event::Extensions(items));
        Ok(())
    }));
    unsafe { profile.GetBrowserExtensions(&handler) }
}

pub fn toggle(ext: &ICoreWebView2BrowserExtension, on: bool) -> Result<()> {
    let handler = BrowserExtensionEnableCompletedHandler::create(Box::new(|_| {
        emit(Event::Refresh);
        Ok(())
    }));
    unsafe { ext.Enable(on, &handler) }
}

pub fn remove(ext: &ICoreWebView2BrowserExtension) -> Result<()> {
    let handler = BrowserExtensionRemoveCompletedHandler::create(Box::new(|_| {
        emit(Event::Refresh);
        Ok(())
    }));
    unsafe { ext.Remove(&handler) }
}
