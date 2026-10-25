# Xeno

A minimal, full screen browser for Windows. No tab strip and no toolbar, just the page.

Xeno is a small native shell written in Rust around the WebView2 engine that already ships with Windows. Pages sit directly on the Windows compositor, so every transition runs on the GPU and the shell itself stays at a few megabytes.

## Keys

| Keys | |
| --- | --- |
| Alt+A | Action bar: search, open sites, run commands |
| Alt+T | Switch tabs. Hold Alt and tap T to move through them |
| Alt+N | New tab |
| Alt+W | Close tab |
| Ctrl+Shift+T | Reopen the last closed tab |
| Alt+S | Settings |
| Alt+1 to Alt+9 | Jump to a tab |
| Alt+C | Copy the page link |

Every shortcut can be changed in Settings, under Keys.

## Building

You need Rust and the WebView2 Runtime, which comes with Windows 11.

```
cargo build --release
```

On the GNU toolchain, MinGW-w64 binutils need to be on your PATH.
