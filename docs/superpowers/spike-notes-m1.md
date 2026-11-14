# M1 Voice Core — Gate Notes

**Date:** 2026-11-14 (scaffold)  
**Target:** V0 personal tool, M1 voice core (FR-6, FR-7, FR-8, manual mode)  
**Prereq:** M0 manual gate/backend decision and memory numbers still pending in `docs/superpowers/spike-notes-m0.md`.

## Automated (this environment)

| Check | Result | Notes |
|---|---|---|
| `cargo fmt --all -- --check` | PASS | clean |
| `cargo clippy --all-targets -- -D warnings` | PASS | clean |
| `cargo test` | PASS | non-ignored unit tests pass (42 passed, 4 ignored at time of Task 8/9 integration) |

## Manual gate (author, target machine) — PENDING

| Check | Result | Notes |
|---|---|---|
| Manual-mode hotkey walkthrough (`cargo run -- --manual`) | PENDING | overlay runs with no capture stream (dot grey/off), hotkeys step/pin/home cards, no auto-reset, countdown hidden |
| Latency p95 ≤ 400 ms | PENDING | `cargo test latency_p95 -- --ignored` with `testdata/latency.wav` + `testdata/latency.expected.json` |
| Recall ≥ 95% (10 cues × 5 reps) | PENDING | `cargo test recall_on_labeled_set -- --ignored` with `testdata/recall/` |
| False triggers ≤ 1 / 30 min unrelated speech | PENDING | `cargo test false_triggers_under_one_per_30min -- --ignored` with `testdata/unrelated-30min.wav` |
| Hotkey conflicts surfaced / defaults usable | PENDING | verify default bindings (Ctrl+Alt+G/H/P/M, Ctrl+Alt+Shift+←/→) register without conflict on target |
| Move mode drag + click-through restore | PENDING | MoveMode toggles draggable window and restores click-through on exit |

## Blockers / follow-ups

- M0 manual gate rows in `docs/superpowers/spike-notes-m0.md` remain pending (Zoom share, blur test, DPI, backend decision, memory/CPU/latency/startup, fault injection). M1 gate depends on M0 backend/memory decisions.
- M1 spoken-recall recording session pending (typed cues only so far); author must record 10 cues × 5 reps and unrelated-speech corpus for ignored replay tests.
- V0 capture matrix "Other platform" remains unnamed (needed by M3).
