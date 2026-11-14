# CuePal PRD v2

Nov 14, 2026 · @Geh

## Review of v1: what to fix

v1 has a sound core, but five of its claims contradict each other or the platform, and several behaviors are undefined. Items marked **P0** would break the product if built as written.

| # | Issue in v1 | Why it matters | Fix in v2 | Pri |
| --- | --- | --- | --- | --- |
| 1 | Cues must be spoken by the user, and interviewer audio is a non-goal | Notes appear only after you start answering, which defeats a prompter | Keep user-cue as default; add opt-in local loopback capture of the call audio as a later phase | P0 |
| 2 | "Never steals focus" but the editor needs keyboard focus | One window cannot be both non-activating and typeable | Two window states: live (no-activate, click-through) and editor (activating, never used live) | P0 |
| 3 | Alt+Space and a global Esc | Alt+Space opens the Windows system menu; global Esc breaks Esc in every app | Configurable hotkeys via RegisterHotKey; defaults avoid system chords; Esc only when overlay is focused | P0 |
| 4 | "< 30 MB RAM" with a Vosk model and egui | Vosk small models need roughly 300 MB at runtime, and eframe alone exceeds 30 MB | Replace with a measured budget (see Non-functional); verify in the first spike | P0 |
| 5 | "Single native binary" | Vosk needs libvosk.dll plus a model folder | Portable zip or installer with DLL and model; say so | P1 |
| 6 | vosk "or" whisper-rs left open | Whisper is not streaming; latency target would fail | Decide: Vosk with a grammar limited to user cues | P0 |
| 7 | Matching rules undefined | False triggers from common words, overlapping cues, echo of the interviewer through speakers | Normalization, minimum cue length, cooldown, collision warnings in the editor | P1 |
| 8 | Latency stated twice (300 ms, 400 ms) and "zero latency" | Not testable | One definition: end of cue word to first painted frame, p95 | P1 |
| 9 | Fixed 2.5 s silence reset | Notes vanish mid-glance; the user may be reading, not speaking | Reset only after speech has ended, a minimum dwell, and a pin hotkey | P1 |
| 10 | Capture exclusion treated as absolute | Needs Windows 10 build 19041+; does not hide from a camera pointed at the screen or hardware capture | State supported versions and limits; add a verification checklist | P1 |
| 11 | cpal at 16 kHz mono assumed | Many devices refuse 16 kHz in shared mode | Capture at native rate, resample to 16 kHz | P1 |
| 12 | Hot-reload and in-app save both write notes.json | Torn writes and edit races | Atomic write, schema version, debounced watcher, keep last good copy | P1 |
| 13 | No testing of recognition quality, no rehearsal mode, no first-run, no logging policy | The core promise (reliable cue detection) is unmeasured | Cue test box, accuracy targets, a labeled audio test set | P2 |

## Version split and scope decisions

Amended Nov 14, 2026. CuePal ships in two stages: **V0** is the personal tool, built for the author's own interviews; **V1** is the shareable release. The core engineering is identical; the split lives in how much the product must survive other people's voices, mics and laptops.

| Area | V0 (personal) | V1 (shareable) |
| --- | --- | --- |
| Features | FR-1 to FR-9, English-only | Same core plus FR-10 rehearsal mode and FR-12 import/export promoted into the release, and a first-run notice |
| Recognition | Recall tested on the author's voice and mic | Recall proven on other voices; FR-4 warnings are primary UI, since a failed cue needs a visible reason |
| Packaging | Portable zip | Same zip with an install README covering the SmartScreen path; code signing revisited only on real user friction |
| Trust | Not needed | Zero-network proof and a plain-words privacy page |
| Capture matrix | The author's own interview apps | Full matrix from the test plan |

**Closed open questions.** Each was decided from options laid out with a recommendation; the recommendations were accepted.

- **Manual mode is in V0.** With the microphone disabled the overlay runs on hotkeys alone; the mic dot gains an off state, and no mic is a valid state, not an error. In manual mode no card auto-resets and the countdown bar is hidden.
- **Closing cues stay**, as an optional per-topic field defaulting to empty. Matching is unchanged; a closing cue just triggers the Home transition.
- **English-only, with the model path as a setting.** No language UI; `model_path` in settings names the Vosk model folder, so another model can be dropped in without code changes.
- **Zip plus docs for V0 and V1.** The README documents the SmartScreen warning path; a certificate is bought only if real users hit friction.

**Builder-check calls (Nov 14, 2026).** Two points a plan review raised; both settled as calls by the author of this amendment.

- **Next and previous hotkeys moved to Ctrl+Alt+Shift+Right and Left.** Ctrl+Alt+Arrow is claimed by Intel display-rotation hotkeys on many laptops through low-level driver hooks, which the startup conflict check cannot see; a mid-interview screen rotation is the worst failure this product can cause. Still rebindable; verified on the author's machine at M1.
- **The V1 gate "recall proven on voices other than the author's" has a fallback.** If no second human voice is realistically available by M4 planning, the gate accepts public speech corpora as the second-voice proxy. Decided at M4.

**Author context (Nov 14, 2026).** Facts the plan was missing.

- No date pressure for V0: every milestone gate runs as written, nothing is compressed.
- The target laptop is older and low on RAM: the smaller-model fallback from the risk table is scheduled work, not contingency.
- Interviews are taken on a USB wired headset, the best case for recognition.
- The M1 test set is built from the author's real cues, provided typed. A short recording of those cues on the real mic is still needed for the spoken-recall part of the M1 gate; the session is pending, not cut.
- V0 capture matrix: Zoom (desktop and window share), Microsoft Teams, Google Meet in Chrome, and one further platform the author still needs to name before M3.

## Overview, goals, non-goals

CuePal is an offline Windows overlay in Rust that shows a candidate's own bullet points when it hears a cue phrase, and hides them from screen capture. Everything runs on the local CPU with no network use.

**Goals**

- Excluded from window, display and screenshot capture on supported Windows builds.
- Cue-to-card in 400 ms or less at p95, measured end to end.
- No focus stealing in live mode: the foreground app never receives a blur event from CuePal.
- Notes are authored in the app or in a JSON file, and changes apply without a restart.
- Zero network traffic, verified by a test, and no audio or transcript written to disk.
- Manual mode: the overlay runs with the microphone disabled, driven by hotkeys alone, with no capture stream opened.

**Non-goals**

- Generative answering, cloud sync, rich text, multi-monitor layouts.
- Hiding from a physical camera, a capture card, or proctoring software that inspects processes.
- Recognition of arbitrary speech: only the user's own cue phrases are recognized.

## Users and workflows

The user is a job candidate on a Windows laptop taking video interviews. They prepare once and then glance at a small overlay under the webcam while speaking.

**Prep (editor mode)**

1. Add a topic with 1 to 3 cue phrases and 3 to 5 short bullets.
2. Use the cue test box: speak or type a phrase and see which card it would open.
3. Fix any warning (cue too short, cue shared by two cards, word missing from the speech vocabulary).
4. Save; the new cue works immediately.

**Live (overlay mode)**

1. The overlay shows the home index with the mic status dot.
2. The user says a cue, such as "on system architecture", and the card opens.
3. The card stays while the user is speaking, then returns home after the reset rule (see Functional requirements).
4. A hotkey can pin the card, step to the next or previous card, hide the overlay, or go home.

## Functional requirements

V0 covers FR-1 to FR-9; later items are marked. Each requirement is testable as written.

| ID | Requirement | Acceptance detail | Phase |
| --- | --- | --- | --- |
| FR-1 | Capture exclusion | Call SetWindowDisplayAffinity with WDA\_EXCLUDEFROMCAPTURE on every CuePal window at creation and after any re-creation. On Windows older than 10 build 19041, show a warning and do not launch in live mode. | V0 |
| FR-2 | Live overlay window | Frameless, always on top, layered with adjustable opacity, WS\_EX\_NOACTIVATE and WS\_EX\_TOOLWINDOW (no taskbar or Alt+Tab entry), click-through by default so mouse clicks reach Zoom. Draggable only while a hotkey-held move mode is on. Position and size saved per display. | V0 |
| FR-3 | Editor window | Normal activating window, separate from the live overlay. Add, edit, reorder, delete topics. Fields: title, 1 to 3 cues, 3 to 5 bullets (max 40 characters each, warned beyond), optional closing cues. Entering the editor hides the live overlay; leaving restores it. | V0 |
| FR-4 | Cue validation and test box | Warn on cues under 2 words or 6 characters, duplicate or prefix-overlapping cues across cards, and words missing from the recognizer vocabulary. The test box shows the winning card for typed or spoken input. | V0 |
| FR-5 | Persistence | notes.json in %APPDATA%/CuePal, schema-versioned, written atomically (temp file then rename), previous copy kept as notes.json.bak. External edits are picked up by a debounced file watcher. An invalid file keeps the last good notes and shows a banner with the error line. | V0 |
| FR-6 | Voice cue matching | Vosk streaming recognizer with a grammar built from all cues plus an unknown-word token. Match on normalized partial results (lowercase, punctuation stripped, filler words dropped). Longest cue wins; a cooldown of 1.5 s blocks re-triggering; a card already open ignores its own cue. | V0 |
| FR-7 | State machine | Home to Active(id) on a match. Active(id) to Home when (a) a closing cue is heard, or (b) speech has ended and silence exceeds the reset window (default 2.5 s, range 1.5 to 5.0 s) and the card has been shown for at least 3 s. A pinned card never auto-resets. Esc or the home hotkey always returns home. In manual mode (mic_enabled false) the same machine runs without speech: no card auto-resets, the countdown bar is hidden, and pin has no effect since nothing auto-resets. | V0 |
| FR-8 | Hotkeys | Registered with RegisterHotKey, no focus change. Defaults: Ctrl+Alt+G hide or show, Ctrl+Alt+H home, Ctrl+Alt+P pin, Ctrl+Alt+Shift+Right and Left next or previous card, Ctrl+Alt+M move mode. All rebindable; conflicts reported at startup. Next and previous avoid Ctrl+Alt+Arrow, which collides with Intel display-rotation chords that RegisterHotKey cannot see. | V0 |
| FR-9 | Audio input | cpal in shared mode, device picker, native sample rate resampled to 16 kHz mono, handles unplug and re-plug. A mic_enabled setting of false runs manual mode: no capture stream is opened, the dot shows off, and hotkeys alone drive cards. Mic dot states: listening, no signal, error, off. | V0 |
| FR-10 | Rehearsal mode | Editor mode that runs the live matcher and logs matches on screen, without showing the overlay. | V1 |
| FR-11 | Call-audio cues | Optional WASAPI loopback so the interviewer's words can open cards. Off by default, local only, with its own matching rules. | post-V1 |
| FR-12 | Import and export | Markdown or CSV import of topics; JSON export. | V1 |

## Non-functional requirements

Every budget below is a target to confirm in the first spike (M0), not a promise. The v1 memory figure of 30 MB is dropped because the speech model alone exceeds it.

| Area | Target | How it is measured |
| --- | --- | --- |
| Cue latency | 400 ms or less at p95, from the end of the cue's last word to the first frame showing the card | Timestamped test audio played into the mic; frame time logged in a debug build |
| Recognizer vocabulary | Grammar limited to the user's cues plus an unknown token | Grammar rebuild on notes change completes off-thread in under 2 s with no audio gap |
| Memory | 450 MB resident or less in live mode (Vosk small model is roughly 300 MB) | Task Manager and a soak test over 2 hours |
| CPU | Under 15% of one core while idle-listening; UI repaints only on state change or once per second for the countdown | Process sampling during a 30-minute test call |
| Startup | Overlay usable within 3 s of launch | Cold start on the target laptop |
| Supported OS | Windows 10 build 19041 or later, Windows 11; x64 | Manual matrix in the test plan |
| Network | None. No telemetry, no update checks | Firewall block test and packet capture show zero connections |
| Privacy | Audio and transcripts stay in memory; logs hold timings and card ids only, off by default | Code review and a log inspection test |
| Reliability | Audio device loss or recognizer failure never crashes the UI; overlay shows the mic dot in error state | Fault injection: unplug mic, corrupt model path |

## Architecture

Three threads, one rule: the audio thread never waits on the UI or on a lock.

&#91;embedded content: architecture · 9 components, 3 threads\]

The overlay never reads the microphone, and the matcher never draws.

**Decisions**

- **Speech:** Vosk small English model in streaming mode, restricted by grammar to the user's cues plus an unknown token. A restricted grammar is faster and more accurate than free dictation. Whisper is out because it is not streaming and would miss the latency target. The model loads from the `model_path` setting (default: the bundled `model` folder), so another Vosk model can be dropped in without code changes.
- **Shared data:** the audio thread reads an immutable cue index behind arc-swap. The editor builds a new index and swaps it in, so matching never blocks on a write lock. The recognizer is rebuilt off-thread when cues change.
- **Windows:** eframe with a transparent window. After creation, apply WDA\_EXCLUDEFROMCAPTURE, WS\_EX\_NOACTIVATE and WS\_EX\_TOOLWINDOW through the windows crate, and re-apply if the window is ever re-created.
- **Mic:** cpal in shared mode so Zoom keeps its own access; resample with rubato.
- **Packaging:** cuepal.exe, libvosk.dll and the model folder in one portable zip.
- **Crates:** eframe, serde, serde\_json, windows, cpal, rubato, vosk, notify, arc-swap.

## UX

CuePal has two separate windows so that typing never conflicts with the no-focus rule. Sizes are defaults at 100% display scaling and scale with DPI.

|  | Live overlay | Editor |
| --- | --- | --- |
| Purpose | Glance at notes during a call | Author and test notes before a call |
| Size | 420 x 180 px, resizable, size saved | 560 x 480 px, resizable |
| Focus | Never activates; click-through | Normal window |
| Capture | Excluded | Excluded |
| Appearance | Near-black background (#121212) at adjustable 60 to 95% opacity, default 85% | Same theme, opaque |
| Input | Hotkeys only | Mouse and keyboard |

**Home view.** An index of card titles in one or two columns, with paging when there are more than six. A small mic dot shows listening (green), no signal (amber), error (red) or off (grey, manual mode). No buttons are needed live; the editor opens by hotkey or tray icon.

**Active card.** Title plus up to 5 bullets at a minimum of 16 pt, auto-shrinking to 13 pt if they do not fit, never scrolling. A thin bar shows time left before reset, hidden in manual mode; a pin icon shows when pinned. Bullets over 40 characters are flagged in the editor, not truncated live.

**Editor.** Topic list on the left, form on the right: title, cue tags, bullets one per line with a live preview, optional closing cues, reset timer slider (1.5 to 5.0 s), overlay opacity, mic on/off toggle (manual mode), mic picker and hotkey bindings. The cue test box sits under the cue field. Save applies immediately and returns to the overlay.

**Tray icon.** Open editor, show or hide overlay, toggle the microphone (manual mode), quit. This is the recovery path if the overlay is hidden or off-screen; a reset-position item moves it to the top center of the primary display.

## Data and persistence

One file, %APPDATA%/CuePal/notes.json, holds topics and settings. Ids are stable so reordering and hand edits do not break references.

```json
{
  "schema_version": 1,
  "settings": {
    "reset_silence_ms": 2500,
    "min_dwell_ms": 3000,
    "opacity": 0.85,
    "mic_enabled": true,
    "mic_device": "default",
    "model_path": "model",
    "hotkeys": { "toggle": "Ctrl+Alt+G", "home": "Ctrl+Alt+H", "pin": "Ctrl+Alt+P" }
  },
  "topics": [
    {
      "id": "t_01",
      "title": "System architecture",
      "cues": ["on system architecture", "how i designed the system"],
      "bullets": ["Event-driven services", "Queue between stages", "Cut p99 by 40%"],
      "closing_cues": []
    }
  ]
}
```

**Rules**

- Writes go to a temp file, then rename over notes.json; the previous version is kept as notes.json.bak.
- A file watcher with a 300 ms debounce reloads on external edits. The app ignores changes it wrote itself.
- An unparseable or invalid file never replaces the loaded notes: the overlay keeps the last good set and the editor shows the error with its line number.
- Unknown fields are preserved on save so newer files survive an older build.
- Cues are stored as typed and normalized at load time, so the file stays human-readable.
- Nothing else is persisted: no audio, no transcripts.
- `model_path` is a folder relative to the install directory; a missing or corrupt model shows the mic dot in error and never crashes the UI.

## Test plan and acceptance criteria

v1 metrics were pass or fail by feel. These are measurable and each has a named test.

| Criterion | Pass condition | Test |
| --- | --- | --- |
| Capture exclusion | No trace of overlay or editor, and no black box, in each capture path below | Manual matrix, run on Windows 10 build 19041 and Windows 11 |
| Cue latency | 400 ms or less at p95 over 50 cue utterances | Recorded audio replayed into a virtual mic; frame timestamps logged |
| Cue recall | 95% or more of spoken cues open the right card | 10 cues x 5 repetitions x 2 speakers, quiet room and with fan noise |
| False triggers | At most 1 per 30 minutes of normal conversation without cues | Replay a 30-minute unrelated speech recording |
| Reset | No reset during pauses up to 1.5 s mid-sentence; reset within 300 ms of the configured window after speech ends | Scripted audio with timed gaps |
| Focus integrity | A test web page logs zero blur or visibilitychange events while cues, resets and hotkeys fire | Page open in browser during a 10-minute run |
| Authoring | New topic with 2 cues and 4 bullets works by voice within 10 s of Save, no restart | Stopwatch test |
| Persistence | 100 rapid saves plus concurrent external edits never corrupt notes.json | Stress script |
| Offline | Zero outbound connections | Firewall block and packet capture |
| Manual mode | No capture stream is opened with the mic disabled, and hotkeys step, pin and home cards | Debug log stream check plus a hotkey walkthrough |
| Soak | 2 hours with no crash, memory growth under 50 MB | Overnight run script |

**Capture matrix.** Zoom (entire desktop and single window), Microsoft Teams, Google Meet in Chrome, OBS (display and window capture), Windows Snipping Tool and Print Screen, Xbox Game Bar recording, and a remote desktop session. Cases that cannot be hidden, such as a phone camera aimed at the screen or an HDMI capture card, are documented as out of scope. V0 runs the matrix on the author's apps: Zoom (desktop and window share), Microsoft Teams, Google Meet in Chrome, plus one platform still to be named; V1 runs the full set.

## Milestones

The riskiest assumptions (window behavior, memory, recognition accuracy) are tested first, before any editor work. Each milestone has a gate; do not start the next until it passes. V0 is M0 to M3; M4 is the shareable V1 release; M5 stays deferred.

| # | Milestone | Version | Scope | Gate |
| --- | --- | --- | --- | --- |
| M0 | Platform spike | V0 | Transparent egui window with capture exclusion, no-activate, click-through, tray; Vosk with a 5-cue grammar on a live mic | Overlay invisible in Zoom desktop share; blur test page quiet; memory and latency numbers recorded |
| M1 | Voice core | V0 | Audio thread, resampler, matcher, state machine, hotkeys, manual mode, hardcoded notes; smaller-model option if M0 memory numbers exceed budget | Latency, recall and false-trigger targets met on the test set; manual-mode hotkey walkthrough passes |
| M2 | Notes and editor | V0 | notes.json, atomic save, watcher, editor, cue validation, test box | Authoring and persistence criteria pass |
| M3 | V0 hardening | V0 | Device loss, DPI and multi-display, portable zip, soak test; capture matrix on the author's own apps | Author's capture matrix, manual-mode test and 2-hour soak pass |
| M4 | Shareable release | V1 | Rehearsal mode, import and export, first-run notice, install README with the SmartScreen path, privacy page; recall and validation re-tested with other voices; full capture matrix | Full capture matrix, recall proven on voices other than the author's, and 2-hour soak pass |
| M5 | Call-audio cues | post-V1 | Optional call-audio cues through loopback | Only if user feedback shows the user-cue limit hurts |

## Risks and open questions

| Risk | Impact | Mitigation |
| --- | --- | --- |
| Capture exclusion fails on an app or driver combination | Notes visible to others | Support matrix, in-app Windows version check, M0 gate |
| Vosk accuracy low for the user's accent or mic | Missed cues | Grammar restriction, cue test box, recall test, longer cue phrases |
| Memory above budget on the low-RAM target laptop | Sluggish overlay | Known constraint: the target machine is older and low-RAM; measure in M0 and build the smaller-model option into M1 if the numbers demand it |
| Interviewer audio bleeds into the mic via speakers | False triggers | Recommend headphones; cooldown; closing-cue and pin controls |
| Unsigned binary flagged by SmartScreen or antivirus | Install friction | README documents the warning path for V0 and V1; a certificate is bought only if real users hit friction |
| Window flags break under some GPU backends | Overlay steals focus or shows a border | Test glow and wgpu backends in M0; fall back to raw Win32 window if needed |
| Interview rules forbid outside notes | The user may breach rules set by the interviewer | First-run notice ships in V1; no features that hide the app from process lists |

**Open questions**

All were closed on Nov 14, 2026; the decisions are recorded under Version split and scope decisions. Nothing is open for V0. The one conditional item is code signing, revisited if V1 users hit SmartScreen friction.
