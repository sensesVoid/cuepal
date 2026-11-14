use crate::audio::{start_capture, AudioStatus, NativeChunk};
use crate::notes::M1_NOTES;
use crate::platform::flags::{cached_hwnd, ensure_flags, set_click_through};
use crate::platform::tray;
use crate::speech::engine::{Engine, MatchInfo, MicState, UiEvent};
use crate::state::StateCtx;
use arc_swap::ArcSwap;
use eframe::egui::{self, Color32, CornerRadius, RichText, ViewportCommand};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub struct SpikeApp {
    mic: MicState,
    #[allow(dead_code)]
    cues: Arc<ArcSwap<crate::notes::CueIndex>>,
    #[allow(dead_code)]
    state: StateCtx,
    opacity: f32,
    click_through: bool,
    visible: bool,
    last_match: Option<MatchInfo>,
    cooldown_until: Instant,
    #[allow(dead_code)]
    audio_session: Option<crate::audio::AudioSession>,
    events_rx: mpsc::Receiver<UiEvent>,
    hwnd: Option<windows::Win32::Foundation::HWND>,
}

impl SpikeApp {
    pub fn new(_cc: &eframe::CreationContext<'_>, mic_enabled: bool) -> Self {
        let cues = Arc::new(ArcSwap::from_pointee(crate::notes::M1_NOTES));
        let (pcm_tx, pcm_rx) = mpsc::channel::<NativeChunk>();
        let (events_tx, events_rx) = mpsc::channel();
        let manual_mode = !mic_enabled;

        let (audio_session, initial_mic) = if mic_enabled {
            let (status_tx, status_rx) = mpsc::channel();
            let events_tx_for_thread = events_tx.clone();
            let audio_session = start_capture(status_tx.clone(), pcm_tx).ok();

            if let Err(e) = Engine::spawn(true, cues.clone(), events_tx, pcm_rx) {
                let _ = status_tx.send(AudioStatus::Error(e.to_string()));
            }

            // Forward audio status into events so the UI dot reflects them.
            std::thread::spawn(move || {
                while let Ok(st) = status_rx.recv() {
                    let ms = match st {
                        AudioStatus::Running { .. } => MicState::Listening,
                        AudioStatus::Error(e) => MicState::Error(e),
                    };
                    let _ = events_tx_for_thread.send(UiEvent::Mic(ms));
                }
            });
            (audio_session, MicState::NoSignal)
        } else {
            // Manual mode: no capture stream, engine is a no-op, mic dot grey/off.
            let _ = Engine::spawn(false, cues.clone(), events_tx, pcm_rx);
            (None, MicState::Off)
        };

        Self {
            mic: initial_mic,
            cues,
            state: StateCtx::new(manual_mode),
            opacity: 0.85,
            click_through: true,
            visible: true,
            last_match: None,
            cooldown_until: Instant::now(),
            audio_session,
            events_rx,
            hwnd: None,
        }
    }
}

impl eframe::App for SpikeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Tray commands (polled from the global menu-event channel).
        for cmd in tray::poll_commands() {
            match cmd {
                tray::TrayCommand::ToggleVisible => {
                    self.visible = !self.visible;
                    ui.ctx().send_viewport_cmd(ViewportCommand::Visible(self.visible));
                }
                tray::TrayCommand::ToggleClickThrough => {
                    self.click_through = !self.click_through;
                    if let Some(h) = self.hwnd {
                        unsafe { set_click_through(h, self.click_through); }
                    }
                }
                tray::TrayCommand::Quit => {
                    ui.ctx().send_viewport_cmd(ViewportCommand::Close);
                }
            }
        }

        // Drain engine events.
        while let Ok(ev) = self.events_rx.try_recv() {
            match ev {
                UiEvent::Match { topic, latency_ms, at } => {
                    if Instant::now() >= self.cooldown_until {
                        self.last_match = Some(MatchInfo {
                            topic,
                            latency_ms,
                            at,
                        });
                        self.cooldown_until = Instant::now() + Duration::from_millis(1500);
                    }
                }
                UiEvent::Mic(ms) => self.mic = ms,
                _ => {}
            }
        }

        // HWND discovery + flag re-apply (review-focus item 3).
        if self.hwnd.is_none() {
            self.hwnd = cached_hwnd();
            if let Some(h) = self.hwnd {
                unsafe {
                    let _ = crate::platform::flags::apply_overlay_flags(h);
                    if self.click_through {
                        set_click_through(h, true);
                    }
                }
            }
        }
        if let Some(h) = self.hwnd {
            unsafe {
                ensure_flags(h);
            }
        }

        let alpha = (self.opacity * 255.0).clamp(0.0, 255.0) as u8;
        let bg = Color32::from_rgba_unmultiplied(18, 18, 18, alpha);
        let rect = ui.max_rect();
        {
            let painter = ui.painter();
            painter.rect_filled(rect, CornerRadius::ZERO, bg);
        }

        ui.horizontal(|ui| {
            ui.label(
                RichText::new("CuePal (spike)")
                    .strong()
                    .size(14.0)
                    .color(Color32::WHITE),
            );
            let dot_color = match self.mic {
                MicState::Listening => Color32::from_rgb(76, 175, 80),
                MicState::NoSignal => Color32::from_rgb(255, 193, 7),
                MicState::Error(_) => Color32::from_rgb(244, 67, 54),
                MicState::Off => Color32::from_gray(120),
            };
            ui.label(RichText::new("●").color(dot_color));
            if let MicState::Error(e) = &self.mic {
                ui.label(
                    RichText::new(e)
                        .small()
                        .color(Color32::from_rgb(244, 67, 54)),
                );
            }
        });

        if let Some(m) = &self.last_match {
            ui.add_space(4.0);
            let title = M1_NOTES.get(m.topic).map(|t| t.title).unwrap_or("?");
            ui.label(RichText::new(title).size(18.0).color(Color32::WHITE));
            ui.label(
                RichText::new(format!("latency_ms={}", m.latency_ms))
                    .small()
                    .color(Color32::GRAY),
            );
        } else {
            ui.add_space(4.0);
            ui.label(
                RichText::new("say a cue phrase")
                    .small()
                    .color(Color32::GRAY),
            );
            for t in M1_NOTES {
                ui.label(
                    RichText::new(format!("• {}", t.title))
                        .small()
                        .color(Color32::from_gray(120)),
                );
            }
        }

        ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("opacity").small().color(Color32::GRAY));
                ui.add(egui::Slider::new(&mut self.opacity, 0.60..=0.95));
                let label = if self.click_through {
                    "click-through: ON"
                } else {
                    "click-through: OFF"
                };
                if ui.small_button(label).clicked() {
                    self.click_through = !self.click_through;
                    if let Some(h) = self.hwnd {
                        unsafe {
                            set_click_through(h, self.click_through);
                        }
                    }
                }
            });
        });

        ui.ctx().request_repaint_after(Duration::from_millis(100));
    }
}
