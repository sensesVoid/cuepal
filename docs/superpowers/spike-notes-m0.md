# M0 platform spike — gate numbers of record

Date: 2026-11-14 (code landed; manual gates pending on the author's machine).
Repo: sensesVoid/cuepal (branch main). Plan: docs/superpowers/plans/2026-11-14-m0-platform-spike.md.

## Automated (verified in CI / locally)

| Criterion | Result | Number / note |
| --- | --- | --- |
| Crate builds (cargo build) | PASS | windows 0.62, eframe 0.37, cpal 0.18, rubato 5, libloading 0.9 |
| Unit tests (cargo test) | PASS | 9 passed, 1 ignored (live FFI integration) |
| Lint (cargo clippy -D warnings) | PASS | clean |
| CI workflow (windows-latest) | PASS | run on e2b82d4 + edea863 + 0024fbe = green |
| Vosk FFI loads real DLL + model | PASS | `loads_dll_model_and_recognizes_silence` (ignored test) passes against pinned vosk-api 0.3.45 + vosk-model-small-en-us-0.15 |
| Backend decision (glow vs wgpu) | PENDING | run `cargo run` (glow) and `cargo run --features wgpu` (wgpu), repeat checks 1-6; record which passes |
| Startup to usable overlay | PENDING | target <= 3 s cold start |

## Manual (author runs on the target machine)

| Criterion | Result | Number / note |
| --- | --- | --- |
| Overlay invisible in Zoom desktop share | PENDING | open Zoom -> Share Screen (entire desktop); overlay region shows desktop without overlay |
| Overlay invisible in Zoom window share + Snipping Tool | PENDING | share a single window; Win+Shift+S snip; no trace, no black box |
| Blur test page quiet during all interactions | PENDING | open test/blur-test.html, interact with CuePal; counter stays 0 |
| No taskbar / Alt-Tab entry; stays on top | PENDING | visual check |
| Click-through: clicks land on app beneath | PENDING | click overlay -> app below receives it |
| Opacity slider changes transparency (60-95%) | PENDING | slider 0.60..=0.95 |
| DPI 100% and 200% | PENDING | repeat checks 1-5 at both scalings |
| Tray: show/hide, click-through toggle, quit | PENDING | cargo run, exercise tray menu |
| Memory after model load / after 2 min idle | PENDING | `Get-Process cuepal \| Select WorkingSet64`; target <= 450 MB |
| CPU idle-listening | PENDING | Task Manager sample ~1 min; target < 15% of one core |
| Latency ballpark per cue (informal) | PENDING | RUST_LOG=debug, say each of the 5 cues x3; latency_ms logged; formal p95 is M1 |
| Fault injection: USB unplug mid-run | PENDING | mic dot -> Error, app alive; replug recorded |
| Fault injection: corrupt model path | PENDING | rename model/ -> model.bak/, run, expect Error state + clean log, no crash |
| Small-model fit on low-RAM machine | PENDING | verdict for M1 (target laptop is older and low-RAM) |

## Review-focus items (recorded outcomes)

1. Mic unplug mid-stream -> error state, no panic (Task 7 step 3).
2. Missing/corrupt model or DLL -> error state, no crash (Task 2 ignored test + Task 7 step 3).
3. Window re-creation re-applies flags (ensure_flags every frame + manual DPI check).
4. High-DPI scaling (manual check above).
5. Repeated same-cue speech -> no re-trigger storm (1.5 s cooldown in app.rs; spoken-repeats check in step 1).

## Notes

- Backend decision (glow vs wgpu) is recorded in Task 5 step 5 item 7; if wgpu passes where glow fails, flip the default feature in Cargo.toml before M1.
- The manual rows are the author's to run on the target laptop; fill them in and re-commit before declaring the M0 gate closed.
