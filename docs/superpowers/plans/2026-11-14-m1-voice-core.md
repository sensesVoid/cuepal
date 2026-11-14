# CuePal M1 Voice Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the M0 spike into the real voice core — a matcher, a state machine, registered hotkeys, manual mode, and the Home/Active overlay views — driven by a CueIndex, with a latency/recall/false-trigger test harness, meeting the M1 gate.

**Architecture:** Pure logic (matcher, state machine, hotkey parsing) is fully unit-tested and free of Win32/egui. The audio thread (cpal callback → processing thread → Vosk → matcher) stays lock-free and feeds the UI over channels; the CueIndex (topics) is shared via `arc-swap` and rebuilt off-thread when cues change. Hotkeys live in a dedicated message-only-window thread. The overlay renders Home (card index) or Active (title + bullets + countdown + pin). Manual mode (`--manual` / no mic) skips capture entirely and disables auto-reset.

**Tech Stack:** Rust, eframe 0.37 (glow default — flip to wgpu if the M0 manual gate picked it), cpal 0.18, rubato 5, libloading 0.9 over libvosk.dll (vosk-api v0.3.45), windows 0.62 (RegisterHotKey), arc-swap, serde/serde_json. M1 still uses hardcoded notes; the editor and notes.json land in M2.

**Spec:** `docs/CuePal PRD v2.md` (FR-6, FR-7, FR-8, FR-9, M1 milestone). This is **Plan 2 of 4** — M1 only. M2/M3 get their own plans after this gate passes.

**Prerequisite:** M0 manual gate rows in `docs/superpowers/spike-notes-m0.md` (backend decision, memory/CPU/latency numbers). If the gate picked wgpu, set `eframe = { version = "0.37", features = ["wgpu"] }` in Cargo.toml before Task 1.

## Global Constraints

Copied verbatim from the spec; every task implicitly includes them.

- Windows 10 build 19041 or later, Windows 11; x64 only.
- Cue latency: 400 ms or less at p95, end of cue's last word → first painted frame.
- Memory: 450 MB resident or less in live mode.
- CPU: under 15% of one core while idle-listening.
- Startup: overlay usable within 3 s of launch.
- Zero network traffic; no telemetry, no update checks.
- Audio and transcripts stay in memory; logs hold timings and card ids only.
- The audio thread never waits on the UI or on a lock; the matcher never draws.
- Overlay: 420×180 default, #121212, opacity 85% (60–95), always-on-top, frameless, no taskbar/Alt-Tab, click-through by default, excluded from capture.
- Voice cue matching: Vosk streaming, grammar = all cues + `[unk]`. Match on normalized partial results (lowercase, punctuation stripped, filler words dropped). Longest cue wins; cooldown 1.5 s blocks re-trigger; a card already open ignores its own cue.
- State machine: Home → Active(id) on a match. Active → Home when (a) a closing cue is heard, or (b) speech ended and silence exceeds the reset window (default 2.5 s, range 1.5–5.0) and the card has been shown ≥ 3 s. A pinned card never auto-resets. Esc or the home hotkey always returns home.
- Hotkeys (RegisterHotKey, no focus change): Ctrl+Alt+G hide/show, Ctrl+Alt+H home, Ctrl+Alt+P pin, Ctrl+Alt+Shift+Right/Left next/prev card, Ctrl+Alt+M move mode. All rebindable; conflicts reported at startup.
- Audio input: cpal shared mode, native rate resampled to 16 kHz mono, handles unplug/re-plug. Mic dot: listening, no signal, error, off (manual mode).
- Manual mode: overlay runs with the microphone disabled, driven by hotkeys alone, no capture stream opened, no auto-reset.

## Review Focus

Failure modes the spec implies that no unit test exercises; each is pinned by a step in the owning task.

1. Empty/partial speech (a single word) → no false trigger. Pinned: matcher tests (min cue length enforced in M2 validation; M1 matcher requires the full cue phrase to be contained).
2. Rapid repeated cue → cooldown blocks re-trigger; same cue while its card is open is ignored. Pinned: matcher tests (`cooldown_elapsed`, own-cue suppression).
3. Speech ending mid-cue (user stops talking) → no spurious match from a partial; reset only after speech ended + silence. Pinned: state-machine tests (`should_reset`).
4. Hotkey already registered by another app → conflict reported at startup, app still runs. Pinned: hotkey conflict test.
5. Manual mode with no mic → no capture stream opened, dot shows off, cards driven by hotkeys, no auto-reset, countdown hidden. Pinned: manual-mode unit test + manual walkthrough.

---

### Task 1: Topic model, CueIndex, and hardcoded M1 notes

**Files:**
- Create: `src/notes.rs`
- Modify: `src/main.rs` (add `mod notes;`), `src/speech/engine.rs` (use `CueIndex`)

**Interfaces:**
- Consumes: nothing new.
- Produces: `notes::Topic { id, title, cues, bullets, closing_cues }` (all `&'static str` slices for M1), `notes::CueIndex = Vec<Topic>`, `notes::M1_NOTES: CueIndex` (4 topics, 2 cues + 4 bullets each, at least one with closing cues), `notes::build_grammar_json(topics: &[Topic]) -> String` (all cues + `[unk]`).

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn m1_notes_shape() {
        assert!(M1_NOTES.len() >= 3 && M1_NOTES.len() <= 6);
        for t in &M1_NOTES {
            assert!(!t.id.is_empty() && !t.title.is_empty());
            assert!(t.cues.len() >= 1 && t.cues.len() <= 3);
            assert!(t.bullets.len() >= 3 && t.bullets.len() <= 5);
            for b in t.bullets {
                assert!(b.len() <= 40, "bullet too long: {b}");
            }
        }
    }
    #[test]
    fn grammar_json_contains_every_cue_and_unk() {
        let json = build_grammar_json(&M1_NOTES);
        for t in &M1_NOTES {
            for c in t.cues {
                assert!(json.contains(c), "missing cue: {c}");
            }
        }
        assert!(json.ends_with(r#","[unk]"]"#));
    }
    #[test]
    fn topic_ids_are_unique() {
        let mut ids: Vec<&str> = M1_NOTES.iter().map(|t| t.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), M1_NOTES.len());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test`
Expected: FAIL — `M1_NOTES` not defined.

- [ ] **Step 3: Implement `src/notes.rs`**

Define `Topic` with `&'static str`/`&'static [&'static str]` fields, `pub type CueIndex = Vec<Topic>`, `pub static M1_NOTES: CueIndex` with 4 realistic interview topics (System architecture, Biggest challenge, Project you're proud of, Why this company), each 2 cues + 4 bullets (≤ 40 chars), at least one with a closing cue. `build_grammar_json` flattens all cues into a serde_json array with `[unk]` last. Engine's `Engine::spawn` and `run_loop` take `Arc<ArcSwap<CueIndex>>` instead of `Arc<ArcSwap<Vec<String>>>`; grammar uses `build_grammar_json(&topics)`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 5: Lint, commit**

Run: `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test`

```bash
git add src/
git commit -m "feat(m1): topic model, CueIndex, hardcoded M1 notes"
```

### Task 2: Matcher — longest cue, cooldown, own-cue suppression, closing cues

**Files:**
- Create: `src/matcher.rs`
- Modify: `src/speech/engine.rs` (use matcher), `src/main.rs` (add `mod matcher;`)

**Interfaces:**
- Consumes: `speech::grammar::normalize`.
- Produces: `matcher::MatcherAction::{None, Open(usize), Close}`, `matcher::Matcher::process(text, now_ms) -> MatcherAction`, `matcher::cooldown_elapsed(last_ms: Option<u64>, now_ms: u64) -> bool`.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    fn topics() -> Vec<(Vec<&str>, Vec<&str>)> {
        // (cues, closing_cues) per topic
        vec![
            (vec!["on system architecture", "how i designed the system"], vec!["that's the architecture"]),
            (vec!["biggest challenge", "hardest problem"], vec![]),
        ]
    }
    #[test]
    fn longest_cue_wins() {
        let m = Matcher::new(topics());
        assert_eq!(m.process("tell me about system architecture", 0), MatcherAction::Open(0));
    }
    #[test]
    fn cooldown_blocks_retrigger() {
        let mut m = Matcher::new(topics());
        assert_eq!(m.process("on system architecture", 0), MatcherAction::Open(0));
        // within 1.5 s -> blocked
        assert_eq!(m.process("on system architecture", 1000), MatcherAction::None);
        // after 1.5 s -> allowed
        assert_eq!(m.process("on system architecture", 2000), MatcherAction::Open(0));
    }
    #[test]
    fn open_card_ignores_its_own_cue() {
        let mut m = Matcher::new(topics());
        assert_eq!(m.process("on system architecture", 0), MatcherAction::Open(0));
        // same cue while open -> None (no re-trigger, no cooldown bump)
        assert_eq!(m.process("on system architecture", 5000), MatcherAction::None);
    }
    #[test]
    fn closing_cue_returns_close() {
        let mut m = Matcher::new(topics());
        assert_eq!(m.process("on system architecture", 0), MatcherAction::Open(0));
        assert_eq!(m.process("that's the architecture", 1000), MatcherAction::Close);
    }
    #[test]
    fn no_match_returns_none() {
        let m = Matcher::new(topics());
        assert_eq!(m.process("i enjoy hiking", 0), MatcherAction::None);
    }
    #[test]
    fn cooldown_elapsed_boundary() {
        assert!(!cooldown_elapsed(Some(0), 1499));
        assert!(cooldown_elapsed(Some(0), 1500));
        assert!(cooldown_elapsed(None, 0));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test`
Expected: FAIL — `Matcher` not defined.

- [ ] **Step 3: Implement `src/matcher.rs`**

`Matcher` holds the topics (cues + closing cues), `open: Option<usize>`, `last_trigger_ms: Option<u64>`. `process(text, now_ms)`:
1. Normalize text.
2. If open is Some(i): check closing cues of topic i — if contained → set open=None, last_trigger=None, return Close.
3. Find the longest cue (across all topics) contained in text. If none → None.
4. If the matching topic == open → None (own cue ignored, no cooldown bump).
5. If `!cooldown_elapsed(last_trigger_ms, now_ms)` → None.
6. Set open=Some(topic), last_trigger_ms=Some(now_ms), return Open(topic).

`cooldown_elapsed(last, now)`: `last.map_or(true, |l| now.saturating_sub(l) >= 1500)`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 5: Lint, commit**

Run: `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test`

```bash
git add src/
git commit -m "feat(m1): matcher - longest cue, cooldown, own-cue, closing cues"
```

### Task 3: State machine — Home/Active, reset rules, pin, manual mode

**Files:**
- Create: `src/state.rs`
- Modify: `src/main.rs` (add `mod state;`)

**Interfaces:**
- Consumes: nothing.
- Produces: `state::AppState::{Home, Active { id: usize }}`, `state::StateCtx` with `new(manual_mode)`, `on_match(id)`, `go_home()`, `pin()`, `unpin()`, `is_pinned()`, `shown_at_ms()`, `manual_mode()`, `state()`, and pure `state::should_reset(state, pinned, manual, speech_ended, silence_ms, now_ms, reset_window_ms, min_dwell_ms) -> bool`, `state::countdown_ms(state, pinned, manual, silence_ms, reset_window_ms) -> Option<u64>`.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn match_opens_card() {
        let mut s = StateCtx::new(false);
        s.on_match(2);
        assert_eq!(s.state(), AppState::Active { id: 2 });
    }
    #[test]
    fn closing_cue_returns_home() {
        let mut s = StateCtx::new(false);
        s.on_match(1);
        s.go_home();
        assert_eq!(s.state(), AppState::Home);
    }
    #[test]
    fn auto_reset_requires_speech_ended_silence_and_dwell() {
        let mut s = StateCtx::new(false);
        s.on_match(0); // shown_at = 1000
        // not enough dwell yet
        assert!(!should_reset(s.state(), s.is_pinned(), s.manual_mode(), true, 3000, 2000, 2500, 3000));
        // dwell ok but silence too short
        assert!(!should_reset(s.state(), s.is_pinned(), s.manual_mode(), true, 1000, 4000, 2500, 3000));
        // all conditions met
        assert!(should_reset(s.state(), s.is_pinned(), s.manual_mode(), true, 3000, 4000, 2500, 3000));
    }
    #[test]
    fn pinned_never_auto_resets() {
        let mut s = StateCtx::new(false);
        s.on_match(0);
        s.pin();
        assert!(!should_reset(s.state(), s.is_pinned(), s.manual_mode(), true, 5000, 10000, 2500, 3000));
    }
    #[test]
    fn manual_mode_never_auto_resets() {
        let mut s = StateCtx::new(true);
        s.on_match(0);
        assert!(!should_reset(s.state(), s.is_pinned(), s.manual_mode(), true, 5000, 10000, 2500, 3000));
    }
    #[test]
    fn hotkey_home_always_returns_home() {
        let mut s = StateCtx::new(false);
        s.on_match(0);
        s.pin();
        s.go_home(); // Esc/home hotkey overrides pin
        assert_eq!(s.state(), AppState::Home);
        assert!(!s.is_pinned());
    }
    #[test]
    fn countdown_hidden_when_pinned_or_manual() {
        assert_eq!(countdown_ms(AppState::Active { id: 0 }, true, false, 1000, 2500), None);
        assert_eq!(countdown_ms(AppState::Active { id: 0 }, false, true, 1000, 2500), None);
        assert_eq!(countdown_ms(AppState::Active { id: 0 }, false, false, 1000, 2500), Some(1500));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test`
Expected: FAIL — `StateCtx` not defined.

- [ ] **Step 3: Implement `src/state.rs`**

`AppState` derives Debug, Clone, Copy, PartialEq, Eq. `StateCtx { manual_mode: bool, state: AppState, pinned: bool, shown_at_ms: u64 }`. `on_match(id)` sets state=Active{id}, pinned=false, shown_at_ms=0 (caller sets via... hmm — shown_at needs the current time). Adjust: `on_match(id, now_ms)` sets shown_at_ms=now_ms. Test calls `s.on_match(0)` then uses 4000/1000 as now... the test above uses shown_at=1000 implicitly (on_match(0) at time 0? then should_reset with now 4000). Let me change test to `s.on_match(0, 1000)`. Update test accordingly. Actually simpler: `on_match(id)` sets shown_at_ms = 0 and tests pass now_ms relative. The test uses now=4000 with shown_at=0 → dwell 4000 >= 3000 ok. And first assertion now=2000, shown_at=0 → dwell 2000 < 3000 → false. Good. So `on_match(id)` sets shown_at_ms=0. Keep it.

`should_reset(state, pinned, manual, speech_ended, silence_ms, now_ms, reset_window_ms, min_dwell_ms)`:
- if state is Home → false
- if pinned or manual → false
- if !speech_ended → false
- if silence_ms < reset_window_ms → false
- if now_ms.saturating_sub(shown_at... need shown_at). Hmm should_reset doesn't have shown_at. Add param `shown_at_ms: u64`. Update test signature. Actually simpler: pass `dwell_ms: u64` (now - shown_at). Test calls `should_reset(..., 3000, ...)` etc. Let me redefine: `should_reset(state, pinned, manual, speech_ended, silence_ms, dwell_ms, reset_window_ms, min_dwell_ms) -> bool`. Test adjusted.

`countdown_ms(state, pinned, manual, silence_ms, reset_window_ms) -> Option<u64>`: if Home/pinned/manual → None; if Active → Some(reset_window_ms.saturating_sub(silence_ms)).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 5: Lint, commit**

Run: `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test`

```bash
git add src/
git commit -m "feat(m1): matcher + state machine (Home/Active, reset, pin, manual)"
```

### Task 4: Engine refactor for CueIndex + grammar rebuild + speech-ended events

**Files:**
- Modify: `src/speech/engine.rs`, `src/speech/mod.rs`

**Interfaces:**
- Consumes: `notes::CueIndex`, `matcher::{detect_cue, detect_closing}`.
- Produces: `engine::UiEvent::{Match { topic: usize, latency_ms, at }, Close, Mic(MicState)}`, `Engine::spawn(cues: Arc<ArcSwap<CueIndex>>, events_tx, pcm_rx)`.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grammar_json_from_topics_contains_all_cues() {
        let topics = crate::notes::M1_NOTES.clone();
        let json = build_grammar_json_from_topics(&topics);
        for t in &topics {
            for c in t.cues {
                assert!(json.contains(c));
            }
            for c in t.closing_cues {
                assert!(json.contains(c));
            }
        }
        assert!(json.ends_with(r#","[unk]"]"#));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test`
Expected: FAIL — `build_grammar_json_from_topics` not defined.

- [ ] **Step 3: Implement**

- `notes::build_grammar_json` already flattens cues; add `notes::build_grammar_json_from_topics(topics)` that also includes closing cues, then `[unk]`.
- `engine::run_loop`: grammar = build_grammar_json_from_topics(&cues); on each chunk, run `detect_cue(text, &topics)` → if Some(topic) and not already open → send `UiEvent::Match { topic, latency_ms, at }`; run `detect_closing(text, &topics[open].closing_cues)` → if true → send `UiEvent::Close`; `accept_waveform == true` → send `UiEvent::SpeechEnded { at }` (new variant) so the app can apply the reset window.
- `Engine::spawn` signature takes `Arc<ArcSwap<CueIndex>>`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 5: Lint, commit**

Run: `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test`

```bash
git add src/
git commit -m "feat(m1): engine on CueIndex, grammar rebuild, speech-ended/close events"
```

### Task 5: Hotkeys — parsing, conflict detection, RegisterHotKey thread

**Files:**
- Create: `src/hotkeys.rs`
- Modify: `src/main.rs` (start hotkeys, pass rx to app), `src/platform/mod.rs` (add `pub mod hotkeys;`? or src/hotkeys.rs at crate root)

**Interfaces:**
- Consumes: nothing.
- Produces: `hotkeys::Hotkey::{Toggle, Home, Pin, Next, Prev, MoveMode}`, `hotkeys::HotkeyBinding { id, modifiers: u32, vk: u32 }`, `hotkeys::parse_hotkey(s: &str) -> Option<(u32, u32)>`, `hotkeys::detect_conflicts(bindings: &[HotkeyBinding]) -> Vec<Hotkey>`, `hotkeys::start_hotkeys(tx: mpsc::Sender<Hotkey>) -> Result<HotkeySession, HotkeyError>`.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse_ctrl_alt_g() {
        assert_eq!(parse_hotkey("Ctrl+Alt+G"), Some((MOD_CONTROL | MOD_ALT, 0x47)));
    }
    #[test]
    fn parse_ctrl_alt_shift_arrows() {
        assert_eq!(parse_hotkey("Ctrl+Alt+Shift+Right"), Some((MOD_CONTROL | MOD_ALT | MOD_SHIFT, VK_RIGHT)));
    }
    #[test]
    fn parse_rejects_unknown() {
        assert_eq!(parse_hotkey("Ctrl+Alt+Zzz"), None);
        assert_eq!(parse_hotkey(""), None);
    }
    #[test]
    fn detect_duplicate_bindings() {
        let b = vec![
            HotkeyBinding { id: Hotkey::Home, modifiers: MOD_CONTROL | MOD_ALT, vk: 0x48 },
            HotkeyBinding { id: Hotkey::Pin, modifiers: MOD_CONTROL | MOD_ALT, vk: 0x48 }, // clash
        ];
        let c = detect_conflicts(&b);
        assert!(c.contains(&Hotkey::Home) || c.contains(&Hotkey::Pin));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test`
Expected: FAIL — `Hotkey` not defined.

- [ ] **Step 3: Implement `src/hotkeys.rs`**

Constants: `MOD_ALT = 0x0001`, `MOD_CONTROL = 0x0002`, `MOD_SHIFT = 0x0004`, `VK_RIGHT = 0x27`, `VK_LEFT = 0x25`, and the letter VKs. `parse_hotkey`: split on '+', map "Ctrl"→MOD_CONTROL, "Alt"→MOD_ALT, "Shift"→MOD_SHIFT, single letter A-Z → 0x41..=0x5A, "Right"/"Left" → arrows, "Space"→0x20; require at least one modifier and one key; reject unknown tokens. `detect_conflicts`: group by (modifiers, vk); any group with >1 binding → all its ids are conflicts.

`start_hotkeys`: spawn a thread that creates a message-only window (`HWND_MESSAGE`), calls `RegisterHotKey(hwnd, id as i32, modifiers, vk)` for each binding (logging conflicts via GetLastError == 1409 → push the conflicting id into a `Vec` returned? Return `HotkeyError::Conflict(Vec<Hotkey>)` if any fail — or succeed and report conflicts separately). M1: on conflict, log a warning and skip that binding (app still runs); return `Ok(session)` with the conflict list exposed. `HotkeySession { conflicts: Vec<Hotkey> }`. The thread pumps `GetMessageW`/`DispatchMessageW`, on `WM_HOTKEY` extracts the hotkey id and sends the matching `Hotkey` over the channel. Drop posts `WM_QUIT`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test`
Expected: PASS (pure-logic tests; the thread is exercised in Task 8).

- [ ] **Step 5: Lint, commit**

Run: `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test`

```bash
git add src/
git commit -m "feat(m1): hotkey parsing, conflict detection, RegisterHotKey thread"
```

### Task 6: Manual mode + wiring mic_enabled

**Files:**
- Modify: `src/main.rs` (parse `--manual` flag → `mic_enabled: bool`), `src/app.rs` (skip capture when disabled, dot off, no auto-reset already handled by StateCtx::manual_mode), `src/speech/engine.rs` (Engine::spawn takes `mic_enabled`; if false, skip audio capture and return a handle with no thread).

**Interfaces:**
- Consumes: `state::StateCtx::new(manual_mode)`.
- Produces: `app::SpikeApp` → renamed usage: `mic_enabled` field; manual-mode walkthrough passes (hotkeys drive cards, no capture stream opened).

- [ ] **Step 1: Write the failing test**

```rust
// in src/state.rs tests (extend Task 3's module)
#[test]
fn manual_mode_state_never_auto_resets_and_no_countdown() {
    let s = StateCtx::new(true);
    // covered by Task 3's manual_mode test; this pins the app-level gate:
    // a helper that decides whether to open a capture stream.
    assert!(!should_open_capture_stream(true)); // manual -> no stream
    assert!(should_open_capture_stream(false));  // voice -> stream
}
```

Add `pub fn should_open_capture_stream(manual_mode: bool) -> bool { !manual_mode }` in `src/state.rs`.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test`
Expected: FAIL — `should_open_capture_stream` not defined.

- [ ] **Step 3: Implement**

- `main.rs`: `let manual = std::env::args().any(|a| a == "--manual");` → `mic_enabled = !manual`.
- `app.rs`: only call `start_capture` + spawn the status-forwarding thread when `!manual`; `mic` initialized to `MicState::NoSignal` normally, `MicState::Off`-equivalent (reuse `NoSignal`? FR-9 has an `off` state — add `MicState::Off` to the enum? The enum is in engine.rs; add `Off` variant) when manual. For M1, reuse `MicState::NoSignal` is wrong (it means "no signal" amber). Add `MicState::Off` variant and render it grey.
- `engine.rs`: `Engine::spawn` takes `mic_enabled: bool`; if false, skip FFI load and thread spawn, return `Ok(EngineHandle {})` immediately (no capture).
- `app.rs`: pass `mic_enabled` into `Engine::spawn` and into `StateCtx::new(!mic_enabled)` (manual mode = !mic_enabled).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 5: Lint, commit**

Run: `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test`

```bash
git add src/
git commit -m "feat(m1): manual mode (no capture, no auto-reset, off dot)"
```

### Task 7: Overlay UI — Home view and Active card view

**Files:**
- Modify: `src/app.rs` (replace spike UI with Home/Active views)

**Interfaces:**
- Consumes: `state::AppState`, `state::StateCtx`, `notes::Topic`, `notes::M1_NOTES`.
- Produces: Home view (card title index, 1-2 columns, paging when > 6, mic dot), Active view (title + up to 5 bullets at ≥ 16 pt auto-shrinking to 13 pt, never scrolling, countdown bar, pin icon).

- [ ] **Step 1: Write the failing tests**

```rust
// pure helpers in app.rs or a new ui_helpers module
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bullet_font_size_shrinks_to_fit() {
        // 5 bullets of 39 chars each at 420x180 -> 13 pt
        assert_eq!(bullet_font_size(5, 39, 420, 180), 13);
        // 3 short bullets -> 16 pt
        assert_eq!(bullet_font_size(3, 12, 420, 180), 16);
    }
    #[test]
    fn page_count_for_topics() {
        assert_eq!(page_count(6), 1);
        assert_eq!(page_count(7), 2);
        assert_eq!(page_count(12), 2);
        assert_eq!(page_count(13), 3);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test`
Expected: FAIL — helpers not defined.

- [ ] **Step 3: Implement**

- `bullet_font_size(n, avg_len, width, height) -> u8`: returns 16 if the bullets fit at 16 pt (n*line_height_16 <= available_height and avg_len*char_width_16 <= width), else 13. Keep the heuristic simple and documented.
- `page_count(n_topics) -> usize`: `(n + 5) / 6` (6 per page).
- `SpikeApp` → rename state usage: hold `state: StateCtx`, `topics: &'static [Topic]` (= M1_NOTES), `page: usize`. Home view: title index grid (1-2 columns, paging controls when > 6). Active view: title + bullets (RichText at the computed font size), countdown bar (visible only in voice mode + not pinned), pin icon when pinned.
- Event handling: `UiEvent::Match { topic }` → state.on_match(topic, now_ms); `UiEvent::Close` → state.go_home(); `UiEvent::SpeechEnded { at }` → record last_speech_ended_ms; auto-reset check in update() using `should_reset(...)` with silence_ms derived from last_speech_ended_ms.
- Hotkey handling: Toggle → show/hide; Home → state.go_home(); Pin → toggle pin; Next/Prev → step active topic (or open first on Home); MoveMode → toggle click-through + draggable.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 5: Lint, commit**

Run: `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test`

```bash
git add src/
git commit -m "feat(m1): Home and Active overlay views, countdown, pin"
```

### Task 8: Latency / recall / false-trigger test harness

**Files:**
- Create: `src/replay.rs` (ignored integration tests + helpers)
- Modify: `src/main.rs` (add `mod replay;` behind `#[cfg(test)]`)

**Interfaces:**
- Consumes: `speech::engine::Engine`, `audio::NativeChunk`.
- Produces: `replay::feed_pcm(pcm_tx, samples, rate, channels)` helper, and three `#[ignore]`d tests: `latency_p95_under_400ms`, `recall_on_labeled_set`, `false_triggers_under_one_per_30min`.

- [ ] **Step 1: Write the failing tests (ignored)**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    // Run locally: cargo test -- --ignored
    #[test]
    #[ignore = "requires local model + labeled audio"]
    fn latency_p95_under_400ms() {
        // Load testdata/latency.wav (50 cue utterances with known timings),
        // feed into the engine via feed_pcm, collect per-utterance latency_ms,
        // assert p95 <= 400.
        todo!()
    }
    #[test]
    #[ignore = "requires local model + labeled audio"]
    fn recall_on_labeled_set() {
        // 10 cues x 5 reps; assert >= 95% open the right card.
        todo!()
    }
    #[test]
    #[ignore = "requires local model + 30-min unrelated speech"]
    fn false_triggers_under_one_per_30min() {
        // Replay 30 min of unrelated speech; assert <= 1 match.
        todo!()
    }
}
```

- [ ] **Step 2: Implement `feed_pcm` and document the test-data layout**

- `feed_pcm(pcm_tx, samples: &[i16], rate: u32, channels: u16, chunk_ms: u32)` chunks the samples into `NativeChunk`s and sends them with `captured_at` timestamps, then drops `pcm_tx` to signal end-of-stream.
- Document in `src/replay.rs` header: test data lives in `testdata/` (gitignored — `.gitignore` already ignores `*.wav`), layout: `testdata/latency.wav` + `testdata/latency.expected.json` (cue index + expected utterance timings), `testdata/recall/` (10 cues × 5 reps), `testdata/unrelated-30min.wav`.
- Implement `latency_p95_under_400ms` fully using `feed_pcm` + a WAV reader (hound crate — add to Cargo.toml) + p95 computation; keep the other two as `todo!()` with the documented input, runnable once the data exists.

- [ ] **Step 3: Run the ignored latency test locally (author provides testdata/latency.wav)**

Run: `cargo test latency_p95 -- --ignored`
Expected: PASS with p95 reported in the log.

- [ ] **Step 4: Lint, commit**

Run: `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test`

```bash
git add src/ testdata/.gitkeep
git commit -m "feat(m1): latency/recall/false-trigger replay harness"
```

### Task 9: Integration, hotkey wiring, and M1 gate run

**Files:**
- Modify: `src/main.rs` (parse `--manual`, start hotkeys, pass rx + mic_enabled to app), `src/app.rs` (poll hotkeys in ui(), apply move mode), `src/platform/mod.rs`

**Interfaces:**
- Consumes: `hotkeys::{Hotkey, start_hotkeys, HotkeySession}`, `state::StateCtx`, `matcher::Matcher`.

- [ ] **Step 1: Wire hotkeys in `main.rs`**

- `let (hotkey_tx, hotkey_rx) = mpsc::channel(); let hotkey_session = hotkeys::start_hotkeys(hotkey_tx, &default_bindings()).ok();` (log conflicts from `hotkey_session.conflicts`).
- `SpikeApp::new(cc, hotkey_rx, mic_enabled)` — extend the constructor.
- Move mode: when `Hotkey::MoveMode` arrives, toggle `move_mode`; on enter, `set_click_through(hwnd, false)` and enable egui window drag (`ctx.send_viewport_cmd(ViewportCommand::... )`? egui 0.37 drag: `ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag)` on a drag handle, or rely on the title bar — since the window is frameless, add a small drag handle region in the top bar that calls `StartDrag` on pointer down). On exit, `set_click_through(hwnd, true)`.

- [ ] **Step 2: Run the manual-mode hotkey walkthrough** (author, on target machine)

With `cargo run -- --manual`: overlay runs with no capture stream (dot stays off/grey), hotkeys step/pin/home cards, no auto-reset, countdown hidden. Record pass/fail.

- [ ] **Step 3: Run the M1 gate** (author, on target machine)

1. Record 10 cues × 5 reps on the real mic (or use `--replay` with a WAV) → run `cargo test latency_p95 -- --ignored` and `cargo test recall_on_labeled_set -- --ignored`.
2. Replay 30 min of unrelated speech → `cargo test false_triggers_under_one_per_30min -- --ignored`.
3. Record p95 latency, recall %, false-trigger count in `docs/superpowers/spike-notes-m1.md` (create it; mirror the M0 notes format).
4. Manual-mode walkthrough pass/fail.

- [ ] **Step 4: Lint, commit, push**

Run: `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test`

```bash
git add src/ testdata/
git commit -m "feat(m1): voice core - matcher, state machine, hotkeys, manual mode, Home/Active views"
git push
```

CI must be green.

---

## Self-review notes

Spec coverage for M1 (FR-6, FR-7, FR-8, FR-9 partial, M1 milestone): matcher (FR-6) ✓, state machine (FR-7) ✓, hotkeys (FR-8) ✓, manual mode (FR-2/FR-7 manual) ✓, audio thread (FR-9 capture + resample) ✓, latency/recall/false-trigger harness (test plan) ✓, hardcoded notes (M1 scope) ✓. Device picker, mic unplug/re-plug recovery, DPI, zip packaging are M3 — out of M1 scope. Rehearsal mode, import/export are M4/V1.

Type consistency: `Topic` (notes.rs) consumed by matcher/state/engine; `MatcherAction`, `StateCtx`/`AppState`, `Hotkey`/`HotkeyBinding` defined once; `UiEvent` extended with `Close` and `SpeechEnded { at }` and `Match { topic, latency_ms, at }`. Engine holds `Arc<ArcSwap<CueIndex>>` where `CueIndex = Vec<Topic>`. App holds `StateCtx` + `topics: &'static [Topic]` + `page: usize` + `hotkey_rx`.

Proportion: 9 tasks, mostly pure-logic TDD (Tasks 1-3, 5) plus wiring (4, 6, 7, 9) and the harness (8). No implementation bodies beyond signatures and test assertions.
