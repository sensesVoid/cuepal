# CuePal M0 Platform Spike Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prove the four riskiest CuePal assumptions with a running spike: capture exclusion, no-activate click-through window, Vosk grammar recognition on a live mic, and the memory/latency budgets — with numbers recorded.

**Architecture:** One eframe overlay window (420×180, transparent, no-activate, click-through, capture-excluded) fed by two worker threads: a cpal capture callback that only copies audio, and a processing thread that downmixes → resamples to 16 kHz mono → feeds a Vosk recognizer (grammar = 5 hardcoded cues + `[unk]`) → emits matches to the UI over channels. The UI never touches audio; the audio thread never waits on the UI or a lock. A tray icon (tray-icon crate) drives show/hide, click-through toggle, and quit.

**Tech Stack:** Rust, eframe 0.37 (glow default, wgpu tested), cpal 0.18, rubato 5 (Async::new_poly), libloading 0.9 over libvosk.dll (pinned vosk-api v0.3.45), tray-icon 0.26, windows 0.62, winreg 0.56, arc-swap, serde_json, log/env_logger.

**Spec:** `docs/CuePal PRD v2.md` (the plan argues from the spec; executors read both). This is **Plan 1 of 4** — M0 only. Per the PRD's milestone rule, M1/M2/M3 get their own plans after this gate passes.

## Global Constraints

Copied verbatim from the spec; every task implicitly includes them.

- Windows 10 build 19041 or later, Windows 11; x64 only. Below that: warn and do not launch.
- Cue latency: 400 ms or less at p95, end of cue's last word → first painted frame. M0 records first numbers; the formal measurement is M1's.
- Memory: 450 MB resident or less in live mode (Vosk small model ≈ 300 MB). Target machine is older and low-RAM — if the numbers exceed budget, the smaller-model option is scheduled M1 work.
- CPU: under 15% of one core while idle-listening.
- Startup: overlay usable within 3 s of launch.
- Zero network traffic; no telemetry, no update checks.
- Audio and transcripts stay in memory; logs hold timings and card ids only. M0's console logging under `RUST_LOG` is development-only.
- The audio thread never waits on the UI or on a lock; the matcher never draws.
- Overlay: frameless, always on top, 420×180 px default, near-black #121212, opacity default 85% (range 60–95), `WS_EX_NOACTIVATE` + `WS_EX_TOOLWINDOW`, click-through, no taskbar/Alt-Tab entry, excluded from capture via `WDA_EXCLUDEFROMCAPTURE`.
- Distribution: portable zip = cuepal.exe + libvosk.dll **plus its three MinGW runtime DLLs** (libgcc_s_seh-1.dll, libstdc++-6.dll, libwinpthread-1.dll — they are hard dependencies of the DLL) + model/ folder. (Release workflow already does this; don't regress it.)

**Decision of record (binding):** Vosk is bound at runtime with `libloading`, not via the `vosk` crate. Reason: the zip does contain `libvosk.lib`, but runtime loading keeps FR-9's rule (missing/corrupt DLL or model → error state, never a crash or link failure) and avoids MSVC↔MinGW import friction. All four DLLs ship and load from the exe's directory.

## Review Focus

Failure modes the spec implies that no unit test exercises; each is pinned by a step in the owning task.

1. Mic unplug mid-stream → status shows error, app stays alive, no panic. Pinned: Task 4 error-path handling + Task 7 manual unplug step.
2. Missing or corrupt model folder / DLL → mic status error, no crash (FR-9 fault injection). Pinned: Task 2 `#[ignore]` load-failure test + Task 7 corrupt-path step.
3. Window re-creation must re-apply all flags (FR-1: "after any re-creation"). Pinned: Task 5 `ensure_flags` re-apply hook + manual DPI-change check.
4. High-DPI (per-monitor) → overlay scales cleanly and stays capture-excluded. Pinned: Task 5 manual 100%/200% scaling check.
5. Repeated same-cue speech → no re-trigger storm (full FR-6 rules are M1; M0 needs a simple cooldown). Pinned: Task 6 cooldown + Task 7 spoken-repeats check.

---

### Task 1: Scaffold crate with pinned deps and the OS-version gate

**Files:**
- Create: `Cargo.toml`, `src/main.rs`, `src/platform/mod.rs`, `src/platform/version.rs`

**Interfaces:**
- Consumes: nothing (first task).
- Produces: `platform::version::os_build() -> u32`, `platform::version::is_supported(build: u32) -> bool`; Cargo.toml with the pinned dependency set that all later tasks import.

- [ ] **Step 1: Write the failing tests** in `src/platform/version.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn supported_boundary() {
        assert!(is_supported(19041));   // Win10 2004 — first supported build
        assert!(!is_supported(19040)); // one below the floor
        assert!(is_supported(26100));  // Win11 24H2
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test`
Expected: FAIL — `is_supported` not defined.

- [ ] **Step 3: Scaffold and implement**

`cargo init --name cuepal` at the repo root (repo already has .git; it won't disturb it). `Cargo.toml` dependencies, exactly:

```toml
[dependencies]
eframe = "0.37"
cpal = "0.18"
rubato = "5"
tray-icon = "0.26"
windows = { version = "0.62", features = ["Win32_Foundation", "Win32_UI_WindowsAndMessaging"] }
libloading = "0.9"
arc-swap = "1"
winreg = "0.56"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
log = "0.4"
env_logger = "0.11"
```

Implement `is_supported(build: u32) -> bool { build >= 19041 }` and `os_build()` reading `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion` → `CurrentBuildNumber` via winreg (parse as u32; return 0 on any failure — callers treat 0 as unsupported). `main.rs`: init env_logger, check `is_supported(os_build())`, `eprintln!` + exit(1) with a clear message if unsupported, else `log::info!("cuepal m0 scaffold ok")`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test`
Expected: PASS (1 test).

- [ ] **Step 5: Lint and commit**

Run: `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: clean.

```bash
git add -A
git commit -m "feat(m0): scaffold crate, pinned deps, OS version gate"
```

- [ ] **Step 6: Push and verify CI runs for real**

```bash
git push
```

Verify on https://github.com/sensesVoid/cuepal/actions: the CI workflow now finds Cargo.toml, runs fmt/clippy/test on windows-latest, and is **green** (first real CI run — no longer self-skipping).

### Task 2: Vosk runtime FFI over libvosk.dll

**Files:**
- Create: `src/speech/mod.rs`, `src/speech/ffi.rs`
- Fetch (local only, gitignored): `libvosk.dll` + 3 MinGW runtime DLLs at repo root; `model/` folder

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces (used by Task 4 and Task 7): `speech::ffi::VoskApi`, `Model`, `Recognizer`, `FfiError`.

- [ ] **Step 1: Fetch the pinned binaries locally**

```powershell
Invoke-WebRequest -Uri "https://github.com/alphacep/vosk-api/releases/download/v0.3.45/vosk-win64-0.3.45.zip" -OutFile vosk.zip
Expand-Archive vosk.zip -DestinationPath vosk-api
Copy-Item vosk-api/vosk-win64-0.3.45/*.dll .          # libvosk.dll + libgcc_s_seh-1.dll + libstdc++-6.dll + libwinpthread-1.dll
Invoke-WebRequest -Uri "https://alphacephei.com/vosk/models/vosk-model-small-en-us-0.15.zip" -OutFile model.zip
Expand-Archive model.zip -DestinationPath model-staging
Move-Item model-staging/vosk-model-small-en-us-0.15 model
```

All four DLLs land at repo root (next to the debug exe — the default DLL search finds them at run time). `*.zip`, `/model/`, `/vosk-api/`, `/model-staging/` and the DLLs are already gitignored. Never commit them.

- [ ] **Step 2: Write the failing test** (integration — needs the local DLL, so `#[ignore]` by default so CI stays green)

```rust
// src/speech/ffi.rs, #[cfg(test)]
// Run locally with: cargo test -- --ignored
#[test]
#[ignore = "requires local libvosk.dll and model/"]
fn loads_dll_model_and_recognizes_silence() {
    let api = unsafe { VoskApi::load(None) }.expect("libvosk.dll next to exe");
    let model = Model::new(&api, "model").expect("model loads");
    let rec = Recognizer::new(model, 16000.0).expect("recognizer");
    rec.set_grammar(crate::speech::grammar::build_grammar_json(&["hello world".into()]).as_str());
    let silence = vec![0i16; 1600];
    let _speech_ended = rec.accept_waveform(&silence);
    let partial = rec.partial_result();
    assert!(partial.contains("partial"), "got: {partial}");
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test`
Expected: FAIL — `VoskApi` not defined (the `#[ignore]`d test is not run; the compile error is the failure).

- [ ] **Step 4: Implement the FFI surface in `src/speech/ffi.rs`**

Exact C mapping (vosk_api.h, v0.3.45), loaded at runtime via `libloading`:

| Rust method | C symbol |
| --- | --- |
| `VoskApi::load(Option<&str>) -> Result<VoskApi, FfiError>` | `Library::new("libvosk.dll")` (path arg overrides) |
| `Model::new(&VoskApi, path: &str) -> Result<Model, FfiError>` | `vosk_model_new(const char*) -> *mut VoskModel` |
| `Recognizer::new(model, sample_rate: f32) -> Result<Recognizer, FfiError>` | `vosk_recognizer_new(VoskModel*, float) -> *mut VoskRecognizer` |
| `Recognizer::set_grammar(&self, grammar_json: &str)` | `vosk_recognizer_set_grm(VoskRecognizer*, const char*)` |
| `Recognizer::accept_waveform(&self, data: &[i16]) -> bool` | `vosk_recognizer_accept_waveform(VoskRecognizer*, const short*, int)` |
| `Recognizer::partial_result(&self) -> String` | `vosk_recognizer_partial_result(VoskRecognizer*) -> const char*` (JSON `{"partial":"..."}`) |
| `Recognizer::final_result(&self) -> String` | `vosk_recognizer_final_result(VoskRecognizer*) -> const char*` (JSON `{"text":"..."}`) |
| `VoskApi::set_loglevel(level: i32)` | `vosk_set_loglevel(int)` — call once with 0 to silence C logging |
| Drop impls | `vosk_recognizer_free`, `vosk_model_free` |

`FfiError` variants: `DllLoad(String)`, `Symbol(String)`, `ModelCreateFailed`, `RecognizerCreateFailed`. `VoskApi` owns the `libloading::Library` plus resolved fn pointers; `Model` holds `Arc<VoskApi>` + raw handle; `Recognizer` owns its `Model` + raw handle. Partial/final results are parsed with `serde_json` (`{"partial": "..."}` → String; missing key → empty string). Both `Model` and `Recognizer` get `unsafe impl Send` with a doc comment: "only the processing thread touches these."

- [ ] **Step 5: Run the integration test locally to verify it passes**

Run: `cargo test -- --ignored`
Expected: PASS.

- [ ] **Step 6: Lint, commit (binaries stay untracked), push**

Run: `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test && git status --short` — confirm no DLL/model files staged.

```bash
git add src/
git commit -m "feat(m0): vosk FFI via libloading, pinned vosk-api 0.3.45"
git push
```

### Task 3: Grammar builder, normalization, match detection (pure logic)

**Files:**
- Create: `src/speech/grammar.rs`

**Interfaces:**
- Consumes: nothing.
- Produces (used by Task 2's test, Task 4's engine, M1): `speech::grammar::build_grammar_json(cues: &[String]) -> String`, `speech::grammar::normalize(text: &str) -> String`, `speech::grammar::detect_match(text: &str, cues: &[String]) -> Option<usize>`.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grammar_json_shape() {
        assert_eq!(
            build_grammar_json(&["on system architecture".into(), "why this".into()]),
            r#"["on system architecture","why this","[unk]"]"#
        );
    }
    #[test]
    fn normalize_strips_case_and_punct() {
        assert_eq!(normalize("WHY This, company?!"), "why this company");
    }
    #[test]
    fn longest_cue_wins_over_prefix_overlap() {
        let cues = vec!["tell me about a project".to_string(), "project".to_string()];
        assert_eq!(detect_match("so tell me about a project", &cues), Some(0));
    }
    #[test]
    fn no_match_returns_none() {
        let cues = vec!["on system architecture".to_string()];
        assert_eq!(detect_match("i enjoy hiking", &cues), None);
    }
    #[test]
    fn punctuated_shouting_still_matches() {
        let cues = vec!["why this company".to_string()];
        assert_eq!(detect_match("WHY THIS COMPANY?!", &cues), Some(0));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test`
Expected: FAIL — functions not defined.

- [ ] **Step 3: Implement in `src/speech/grammar.rs`**

- `build_grammar_json`: `serde_json::to_string(&[cues..., "[unk]"])` — Vosk grammar is a JSON array of phrases plus the `[unk]` token last.
- `normalize`: lowercase, map every non-alphanumeric to a space, collapse runs of spaces, trim. (Filler-word dropping is M1; not here.)
- `detect_match`: normalize both sides; among cues contained in the text, return the **longest** (ties → first); None otherwise.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test`
Expected: PASS (all).

- [ ] **Step 5: Lint, commit**

Run: `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test`

```bash
git add src/
git commit -m "feat(m0): grammar builder, normalization, longest-cue match"
```

### Task 4: Audio capture → resample → recognizer thread → match events

**Files:**
- Create: `src/audio/mod.rs`, `src/speech/engine.rs`
- Modify: `src/speech/mod.rs` (module decls)

**Interfaces:**
- Consumes: `speech::ffi::{VoskApi, Model, Recognizer, FfiError}`, `speech::grammar::{build_grammar_json, normalize, detect_match}`.
- Produces (used by Task 5's app and Task 7):
  - `audio::start_capture(status_tx: mpsc::Sender<AudioStatus>, pcm_tx: mpsc::Sender<NativeChunk>) -> Result<AudioSession, AudioError>`
  - `audio::AudioStatus::{Running { device: String, rate: u32, channels: u16 }, Error(String)}`
  - `audio::NativeChunk { samples: Vec<f32>, channels: u16, rate: u32, captured_at: Instant }`
  - `speech::engine::Engine` — `Engine::spawn(cues: Arc<ArcSwap<Vec<String>>>, events_tx: mpsc::Sender<UiEvent>) -> Result<EngineHandle, FfiError>`; owns the processing thread.
  - `speech::engine::UiEvent::{Match(MatchInfo), Mic(MicState)}`
  - `speech::engine::MatchInfo { cue: String, latency_ms: u32, at: Instant }`
  - `speech::engine::MicState::{Listening, NoSignal, Error(String)}`

- [ ] **Step 1: Write the failing tests** for the pure math in `src/audio/mod.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn downmix_averages_channels() {
        let stereo = vec![0.4, -0.2, 0.6, 0.2]; // 2 frames, 2 channels
        assert_eq!(downmix_to_mono(&stereo, 2), vec![0.1, 0.4]);
    }
    #[test]
    fn chunker_cuts_on_100ms_boundaries() {
        let mut c = I16Chunker::new(1600); // 100 ms at 16 kHz
        let mut out = Vec::new();
        c.push(&vec![0i16; 3200], &mut out);
        assert_eq!(out.len(), 2);
        c.push(&vec![1i16; 2000], &mut out);
        assert_eq!(out.len(), 3); // one full chunk + 400 held
    }
    #[test]
    fn resampler_constructs_for_common_rates() {
        assert!(Resampler16k::new(48000).is_ok());
        assert!(Resampler16k::new(44100).is_ok());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test`
Expected: FAIL — items not defined.

- [ ] **Step 3: Implement `src/audio/mod.rs`**

- `start_capture`: `cpal` default input device (`default_input_device()` → else `AudioError::NoDevice`), its `default_input_config()` (native rate/channels — never force 16 kHz, many devices refuse it in shared mode), build `cpal` stream with callback: copy the f32 buffer into a `NativeChunk` + `mpsc::send` (callback does nothing else — no locks, no resampling in the callback). cpal error callback → `status_tx.send(AudioStatus::Error(...))` and log. Dropping `AudioSession` stops the stream.
- `downmix_to_mono`: average channels per frame.
- `I16Chunker`: accumulates i16 samples, emits fixed 1600-sample chunks, holds the remainder.
- `Resampler16k::new(native_rate)`: `rubato::Async::<f32>::new_poly(16000.0 / native_rate as f32, 1.05, rubato::PolynomialDegree::Cubic, 1024, 1, rubato::FixedAsync::Input)` — poly interpolation is the docs-recommended choice for a voice app on a CPU budget; fixed-input mode suits cpal's callback-sized chunks. `push(mono: &[f32], out: &mut Vec<i16>)`: buffer internally, drive `input_frames_next()` / `process_into_buffer` per the rubato stream pattern, scale f32→i16, emit via the chunker.

- [ ] **Step 4: Implement `src/speech/engine.rs` (Recognizer thread)**

`Engine::spawn` loads `VoskApi::load(None)` + `Model::new(.., "model")` + `Recognizer::new(.., 16000.0)` + `set_grammar(build_grammar_json(&cues_load()))` — any failure returns `Err(FfiError)` to the caller (FR-9: the app must not crash on a bad model). On success it spawns the processing thread that loops:

1. `pcm_rx.recv_timeout(200ms)` → on timeout send `Mic(MicState::NoSignal)` once (device silent, not an error).
2. `downmix_to_mono` → `Resampler16k::push` → i16 chunks → `recognizer.accept_waveform(&chunk)`.
3. After each chunk, read `partial_result`; if `accept_waveform` returned true, also `final_result`.
4. `detect_match` on the normalized text → on hit (and not the cue of the currently-open match — M0 keeps a `last_matched_cue` to suppress immediate repeats), send `UiEvent::Match(MatchInfo { cue, latency_ms: (Instant::now() - captured_at).as_millis() as u32, at })` and `log::info!("match cue={} latency_ms={} …", …)` (timings only — no audio, no transcript text, per the privacy constraint; card ids only once they exist in M1).
5. Any `FfiError` mid-loop → `UiEvent::Mic(MicState::Error(..))`, log, and **keep the thread alive** (retry accept on the next chunk; review-focus item 1/2 — the UI must survive).
6. Cue changes: the thread re-reads `cues.load()` each loop; when the snapshot changed, rebuild grammar via `set_grammar` (cheap, off the UI thread, satisfies "grammar rebuild off-thread").

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test`
Expected: PASS (math tests; engine's live behavior is Task 7's).

- [ ] **Step 6: Lint, commit**

Run: `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test`

```bash
git add src/
git commit -m "feat(m0): audio pipeline, resampler to 16k, recognizer thread"
```

### Task 5: Overlay window, Win32 flags, blur-test page

**Files:**
- Create: `src/app.rs`, `test/blur-test.html`
- Modify: `src/main.rs`, `src/platform/flags.rs` (new), `src/platform/mod.rs`

**Interfaces:**
- Consumes: `speech::engine::{Engine, UiEvent, MatchInfo, MicState}`, `platform::version::is_supported`.
- Produces: `platform::flags::{OVERLAY_TITLE, find_overlay_hwnd, apply_overlay_flags, set_click_through, ensure_flags}`, `app::SpikeApp`.

- [ ] **Step 1: Create `test/blur-test.html`** (committed; used for the focus-integrity check)

A minimal page: it listens for `blur` on `window` and `visibilitychange` on `document`, appends each event with a timestamp to an on-page log and `localStorage["blurlog"]`, shows a running counter, and has a Clear button. The pass condition for every manual check below: **counter stays 0**.

- [ ] **Step 2: Implement `src/platform/flags.rs`**

- `pub const OVERLAY_TITLE: &str = "CuePal Overlay (spike)";` — unique title so `FindWindowW` can find the window without depending on eframe internals (version-proof HWND discovery).
- `find_overlay_hwnd() -> Option<HWND>`: `FindWindowW(None, OVERLAY_TITLE.wide())`.
- `apply_overlay_flags(hwnd) -> Result<(), String>`: `SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE)`; read `GWL_EXSTYLE` via `GetWindowLongPtrW`, OR in `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_TRANSPARENT`, write back via `SetWindowLongPtrW`. Return `Err(GetLastError formatted)` on failure. Call once at window discovery — do **not** write WS_EX_NOACTIVATE etc. into eframe options and assume they stick; this function is the single source of truth.
- `set_click_through(hwnd, on)`: set/clear only `WS_EX_TRANSPARENT` (keep the rest).
- `ensure_flags(hwnd)`: cheap idempotent check — re-read style and `GetWindowDisplayAffinity`; if either drifted from the expected values, re-apply. Called from `App::update` every frame (review-focus item 3: re-creation re-applies flags).

- [ ] **Step 3: Implement `src/app.rs`**

`SpikeApp` (eframe `App`):

- State: `mic: MicState`, `cues: Arc<ArcSwap<Vec<String>>>` (shared with the engine), `opacity: f32` (default 0.85, slider 0.60–0.95), `click_through: bool`, `last_match: Option<MatchInfo>`, `cooldown_until: Instant`, `events_rx: mpsc::Receiver<UiEvent>`.
- `new(cc)`: store `Arc<ArcSwap>` with the 5 spike cues; start `audio::start_capture` + `speech::engine::Engine::spawn`; wire channels.
- `update(ctx, _frame)`: drain `events_rx` (`try_recv` loop — Match updates `last_match` and sets a 1.5 s cooldown (review-focus item 5); Mic updates `mic`); then `flags::ensure_flags(hwnd)`; then draw:
  - full-rect background `Color32::from_rgba_unmultiplied(18, 18, 18, (opacity * 255.0) as u8)` (per-pixel alpha carries the opacity slider; if a backend renders it opaque-black, record it and fall back to `winit` opacity — decision recorded in spike notes),
  - title row "CuePal (spike)" + mic dot: green `Listening`, amber `NoSignal`, red `Error`,
  - cue list (dimmed, small) when no match; when `last_match`: the matched cue in large text + `latency_ms`,
  - opacity slider (works only while click-through is off — note it on-screen).

- [ ] **Step 4: Rewrite `src/main.rs`**

Order matters: version check → `flags` watcher thread → `eframe::run_native`.

- Watcher thread: poll `find_overlay_hwnd()` every 100 ms up to 10 s; on first hit, `apply_overlay_flags` + store HWND in a `OnceLock<HWND>` (global `static OVERLAY_HWND`); log result. (`run_native` blocks the main thread, so discovery must be off-thread.)
- `eframe::NativeOptions`: `viewport: ViewportBuilder::default().with_inner_size([420.0, 180.0]).with_resizable(true).with_transparent(true).with_always_on_top().with_decorations(false).with_taskbar_icon(false).with_active(false)` — winit handles these visuals; `flags.rs` still asserts the Win32 styles itself. Default renderer (glow) for this task.

- [ ] **Step 5: Build and run the manual window gate** (record every result in `docs/superpowers/spike-notes-m0.md`)

Run: `cargo run`, with `test/blur-test.html` open in a browser on the same desktop.

1. No taskbar entry, no Alt-Tab entry, no decorations, stays on top.
2. Click-through: click the overlay — the click lands on the app beneath; blur page counter **stays 0**.
3. Opacity slider changes transparency.
4. **Capture exclusion (the headline gate)**: Zoom → Share Screen (entire desktop): the overlay's region shows the desktop **without** the overlay; then share a single window. Also `Win+Shift+S` snip and `PrintScreen`: no trace, no black box.
5. Blur page counter stays 0 while clicking through and during opacity changes.
6. DPI: repeat checks 1–5 at 100% and 200% display scaling.
7. Re-run with `cargo run --features wgpu` after temporarily enabling the `wgpu` feature in Cargo.toml (`eframe = { version = "0.37", features = ["wgpu"] }`): repeat checks 1–6. Decision of record: whichever backend passes all checks becomes the M1 default (if both pass, glow stays).

- [ ] **Step 6: Commit (feature flag reverted if glow won), push**

```bash
git add src/ test/
git commit -m "feat(m0): transparent no-activate click-through overlay with capture exclusion"
git push
```

CI must be green (windows-latest compiles the win32 code — first true validation of the `windows` 0.62 feature set).

### Task 6: Tray icon and commands

**Files:**
- Create: `src/platform/tray.rs`
- Modify: `src/app.rs`, `src/main.rs`

**Interfaces:**
- Consumes: `platform::flags::{OVERLAY_HWND, set_click_through}`.
- Produces: `platform::tray::{TrayCommand, create_tray}` — `create_tray() -> Result<tray_icon::TrayIcon, String>`; `TrayCommand::{ToggleVisible, ToggleClickThrough, Quit}`; commands are mapped in `app.rs` from `tray_icon::menu::MenuEvent::receiver()` (global channel; menu ids: `toggle_visible`, `toggle_click_through`, `quit`).

- [ ] **Step 1: Implement `src/platform/tray.rs`**

- `create_tray()`: menu = "Show / Hide overlay" (`toggle_visible`), "Toggle click-through" (`toggle_click_through`), "Quit" (`quit`); icon = 16×16 RGBA solid (76,175,80,255) via `tray_icon::Icon::from_rgba`; tooltip "CuePal (spike)". Build on the main thread **before** `run_native` — winit's message pump then drives tray events; `App::update` polls `MenuEvent::receiver().try_recv()`.
- `app.rs` maps menu ids → `TrayCommand`: `ToggleVisible` → `ctx.send_viewport_cmd(egui::ViewportCommand::Visible(!visible))` (track `visible` in state); `ToggleClickThrough` → `flags::set_click_through(hwnd, !on)` (review-focus item 5's UI surface: with click-through off you can interact, incl. the opacity slider); `Quit` → `ctx.send_viewport_cmd(egui::ViewportCommand::Close)`.

- [ ] **Step 2: Manual verify**

Run: `cargo run`. Tray appears; Show/Hide toggles the overlay; click-through toggle restores mouse interaction; Quit exits cleanly (no zombie process, no panic).

- [ ] **Step 3: Lint, commit, push**

Run: `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test`

```bash
git add src/
git commit -m "feat(m0): tray icon with show/hide, click-through toggle, quit"
git push
```

### Task 7: Live end-to-end verification and the M0 gate numbers

**Files:**
- Create: `docs/superpowers/spike-notes-m0.md` (committed — numbers of record)
- Modify: nothing else (behavior unchanged)

**Interfaces:**
- Consumes: everything from Tasks 1–6 assembled in `main.rs`.
- Produces: the M0 gate verdict and the numbers that steer M1 (backend choice, memory budget fit, latency ballpark, model fit on the low-RAM machine).

- [ ] **Step 1: Live recognition run**

Run: `RUST_LOG=debug cargo run`. Speak each of the 5 cues aloud (clear, natural pace), 3× each, with pauses.

Expected: each spoken cue paints its banner; latency_ms logged per match (the M0 ballpark, not the formal p95); repeated quick repeats of the same cue do **not** re-trigger during the 1.5 s cooldown.

- [ ] **Step 2: Record memory and CPU**

```powershell
Get-Process cuepal | Select-Object Id, @{n='WS_MB';e={[math]::Round($_.WorkingSet64/1MB,1)}}
```

Record: after model load, and again after 2 minutes of idle listening. Sample CPU% for ~1 minute in Task Manager while the app idles on mic. Compare against the budgets: memory ≤ 450 MB resident, CPU < 15% of one core. If memory exceeds budget, write "smaller-model option → M1 scheduled work" in the notes (the low-RAM machine makes this a real possibility — that finding is the gate working, not a failure).

- [ ] **Step 3: Fault injection (review-focus items 1 and 2)**

1. Unplug the USB headset mid-run → mic dot turns red / `NoSignal`→`Error`, app alive. Re-plug → record what happens (full recovery logic is M3; M0 only requires no crash).
2. Quit, rename `model/` to `model.bak/`, run → mic status `Error`, no crash, clean log line. Rename back.

- [ ] **Step 4: Write `docs/superpowers/spike-notes-m0.md`**

Template — one row per M0 gate criterion (from the PRD), each with PASS/FAIL + number or note:

| Criterion | Result | Number / note |
| --- | --- | --- |
| Overlay invisible in Zoom desktop share | | |
| Overlay invisible in Zoom window share + Snipping Tool | | |
| Blur test page quiet during all interactions | | |
| Backend decision (glow vs wgpu) | | |
| Memory after model load / after 2 min | | MB |
| CPU idle-listening | | % |
| Latency ballpark per cue | | ms (informal; formal p95 is M1) |
| Startup to usable overlay | | s (≤ 3) |
| Fault injection: unplug / corrupt model | | behavior observed |
| Small-model fit on low-RAM machine | | verdict for M1 |

- [ ] **Step 5: Final commit and CI gate**

```bash
git add docs/
git commit -m "docs(m0): spike gate numbers of record"
git push
```

Verify: CI green on the final push. The M0 gate is done only when every PRD M0 criterion has a row with PASS (or a recorded, justified deviation that M1 must address).
