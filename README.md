# Multi Uploader 3.0 (Rust)

A Rust rewrite of Multi Uploader 2.0 (C++/Win32). It uses the same native Win32
window, dark theme, 11 hosts, unique-copy logic and `MultiUploader.ini` file
(next to the exe), so your existing settings carry over.

## Build on Windows
1. Install Rust from https://rustup.rs (choose the default MSVC toolchain).
2. In this folder, run: `cargo build --release`
3. Output: `target\release\MultiUploader.exe`

## Cross-compile from Linux (how the shipped exe was built)
    rustup target add x86_64-pc-windows-gnu
    sudo apt install mingw-w64
    cargo build --release --target x86_64-pc-windows-gnu
The CRT is linked statically (`.cargo/config.toml`), so the exe has no runtime DLL dependencies.

## Tests
    cargo test                # unit tests (JSON, URLs, text helpers)
    MU_TEST_SERVER=http://127.0.0.1:8765 cargo test   # + upload tests against a local server

The `local-test` feature (`cargo build --features local-test`) sends every host's
traffic to http://127.0.0.1:8765 so you can test the whole app without uploading
anything. **Never use it for a release build.**

## Layout
- `src/main.rs`      window, painting, controls, dialogs, INI
- `src/engine.rs`    job queue, worker threads, retries, duplicate-link handling
- `src/uploader.rs`  WinHTTP multipart/raw uploads, progress, cancel
- `src/sites.rs`     the 11 hosts
- `src/json.rs`, `src/util.rs`, `src/win.rs` (Win32 constants and structs)
- `res/`             icon, manifest (PerMonitorV2 DPI), version info

## Differences from 2.0
- Version 3.0.0; User-Agent `MultiUploader/3.0 (native; Rust)`.
- Memory-safe Rust code; the behaviour and UI are otherwise the same.
