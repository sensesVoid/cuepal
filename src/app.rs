use crate::audio::{start_capture, AudioStatus, NativeChunk};
use crate::hotkeys::Hotkey;
use crate::notes::{Topic, M1_NOTES};
use crate::platform::flags::{cached_hwnd, ensure_flags, set_click_through};
use crate::platform::tray;
use crate::speech::engine::{Engine, MicState, UiEvent};
use crate::state::{countdown_ms, should_reset, AppState, StateCtx};
use arc_swap::ArcSwap;
use eframe::egui::{self, Color32, CornerRadius, RichText, ViewportCommand};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

const RESET_WINDOW_MS: u64 = 2500;
const MIN_DWELL_MS: u64 = 3000;

/// Choose bullet font size so content fits without scrolling (FR-5).
pub fn bullet_font_size(n: usize, avg_len: usize, _width: u32, _height: u32) -> u8 {
    // Heuristic: dense/long bullet blocks shrink to 13 pt; short lists stay at 16 pt.
    if n >= 5 || n * avg_len > 80 {
        13
    } else {
        16
    }
}

/// 6 topics per page in the Home index.
pub fn page_count(n_topics: usize) -> usize {
    n_topics.div_ceil(6)
}

pub struct OverlayApp {
    mic: MicState,
    #[allow(dead_code)]
    cues: Arc<ArcSwap<crate::notes::CueIndex>>,
    state: StateCtx,
    topics: &'static [Topic],
    page: usize,
    opacity: f32,
    click_through: bool,
    visible: bool,
    last_speech_ended_ms: Option<u64>,
    events_rx: mpsc::Receiver<UiEvent>,
    hotkeys_rx: Option<mpsc::Receiver<Hotkey>>,
    hwnd: Option<windows::Win32::Foundation::HWND>,
    #[allow(dead_code)]
    audio_session: Option<crate::audio::AudioSession>,
    start: Instant,
}

impl OverlayApp {
    pub fn new(_cc: &eframe::CreationContext<'_>, mic_enabled: bool) -> Self {
        let cues = Arc::new(ArcSwap::from_pointee(M1_NOTES));
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
            let _ = Engine::spawn(false, cues.clone(), events_tx, pcm_rx);
            (None, MicState::Off)
        };

        Self {
            mic: initial_mic,
            cues,
            state: StateCtx::new(manual_mode),
            topics: M1_NOTES,
            page: 0,
            opacity: 0.85,
            click_through: true,
            visible: true,
            last_speech_ended_ms: None,
            events_rx,
            hotkeys_rx: None,
            hwnd: None,
            audio_session,
            start: Instant::now(),
        }
    }

    fn now_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }

    fn handle_hotkey(&mut self, hk: Hotkey, ctx: &egui::Context) {
        match hk {
            Hotkey::Toggle => {
                self.visible = !self.visible;
                ctx.send_viewport_cmd(ViewportCommand::Visible(self.visible));
            }
            Hotkey::Home => {
                self.state.go_home();
            }
            Hotkey::Pin => {
                if self.state.is_pinned() {
                    self.state.unpin();
                } else {
                    self.state.pin();
                }
            }
            Hotkey::Next => {
                self.step_topic(1);
            }
            Hotkey::Prev => {
                self.step_topic(-1);
            }
            Hotkey::MoveMode => {
                self.click_through = !self.click_through;
                if let Some(h) = self.hwnd {
                    unsafe {
                        set_click_through(h, self.click_through);
                    }
                }
            }
        }
    }

    fn step_topic(&mut self, delta: i32) {
        let n = self.topics.len();
        if n == 0 {
            return;
        }
        let now = self.now_ms();
        match self.state.state() {
            AppState::Active { id } => {
                let next = (id as i32 + delta).rem_euclid(n as i32) as usize;
                self.state.on_match(next, now);
            }
            AppState::Home => {
                let first = if delta >= 0 { 0 } else { n.saturating_sub(1) };
                self.state.on_match(first, now);
            }
        }
    }

    fn drain_events(&mut self) {
        while let Ok(ev) = self.events_rx.try_recv() {
            match ev {
                UiEvent::Match { topic, .. } => {
                    self.state.on_match(topic, self.now_ms());
                    self.last_speech_ended_ms = None;
                }
                UiEvent::Close => {
                    self.state.go_home();
                }
                UiEvent::SpeechEnded { .. } => {
                    self.last_speech_ended_ms = Some(self.now_ms());
                }
                UiEvent::Mic(ms) => self.mic = ms,
            }
        }
    }

    fn maybe_auto_reset(&mut self) {
        let now = self.now_ms();
        let silence_ms = self
            .last_speech_ended_ms
            .map(|t| now.saturating_sub(t))
            .unwrap_or(0);
        let dwell_ms = now.saturating_sub(self.state.shown_at_ms());
        if should_reset(
            self.state.state(),
            self.state.is_pinned(),
            self.state.manual_mode(),
            self.last_speech_ended_ms.is_some(),
            silence_ms,
            dwell_ms,
            RESET_WINDOW_MS,
            MIN_DWELL_MS,
        ) {
            self.state.go_home();
            self.last_speech_ended_ms = None;
        }
    }

    fn render_home(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.label(
            RichText::new("Topics")
                .small()
                .color(Color32::GRAY),
        );
        let per_page = 6usize;
        let total_pages = page_count(self.topics.len()).max(1);
        if self.page >= total_pages {
            self.page = total_pages - 1;
        }
        let start = self.page * per_page;
        let end = (start + per_page).min(self.topics.len());
        let page_topics = &self.topics[start..end];

        let cols = if page_topics.len() > 3 { 2 } else { 1 };
        egui::Grid::new("topic_grid")
            .num_columns(cols)
            .spacing([12.0, 4.0])
            .show(ui, |ui| {
                for (i, t) in page_topics.iter().enumerate() {
                    ui.label(
                        RichText::new(format!("• {}", t.title))
                            .color(Color32::from_gray(230)),
                    );
                    if (i + 1) % cols == 0 {
                        ui.end_row();
                    }
                }
            });

        if total_pages > 1 {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.small_button("◀").clicked() && self.page > 0 {
                    self.page -= 1;
                }
                ui.label(
                    RichText::new(format!("{}/{}", self.page + 1, total_pages))
                        .small()
                        .color(Color32::GRAY),
                );
                if ui.small_button("▶").clicked() && self.page + 1 < total_pages {
                    self.page += 1;
                }
            });
        }
    }

    fn render_active(&mut self, ui: &mut egui::Ui, id: usize) {
        let Some(topic) = self.topics.get(id) else {
            self.state.go_home();
            return;
        };
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(topic.title)
                    .strong()
                    .size(16.0)
                    .color(Color32::WHITE),
            );
            if self.state.is_pinned() {
                ui.label(RichText::new("📌").small());
            }
        });
        let avg_len = if topic.bullets.is_empty() {
            0
        } else {
            topic.bullets.iter().map(|b| b.len()).sum::<usize>() / topic.bullets.len()
        };
        let font_size = bullet_font_size(topic.bullets.len(), avg_len, 420, 180) as f32;
        ui.add_space(2.0);
        for b in topic.bullets {
            ui.label(
                RichText::new(format!("• {}", b))
                    .size(font_size)
                    .color(Color32::from_gray(235)),
            );
        }

        // Countdown bar (voice mode + not pinned)
        let now = self.now_ms();
        let silence_ms = self
            .last_speech_ended_ms
            .map(|t| now.saturating_sub(t))
            .unwrap_or(0);
        if let Some(remaining) = countdown_ms(
            self.state.state(),
            self.state.is_pinned(),
            self.state.manual_mode(),
            silence_ms,
            RESET_WINDOW_MS,
        ) {
            let frac = (remaining as f32 / RESET_WINDOW_MS as f32).clamp(0.0, 1.0);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 3.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, CornerRadius::same(1), Color32::from_gray(60));
            let mut fill = rect;
            fill.set_width(rect.width() * frac);
            ui.painter().rect_filled(fill, CornerRadius::same(1), Color32::from_rgb(100, 180, 255));
        }
    }
}

impl eframe::App for OverlayApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // Tray commands.
        for cmd in tray::poll_commands() {
            match cmd {
                tray::TrayCommand::ToggleVisible => {
                    self.visible = !self.visible;
                    ctx.send_viewport_cmd(ViewportCommand::Visible(self.visible));
                }
                tray::TrayCommand::ToggleClickThrough => {
                    self.click_through = !self.click_through;
                    if let Some(h) = self.hwnd {
                        unsafe { set_click_through(h, self.click_through); }
                    }
                }
                tray::TrayCommand::Quit => {
                    ctx.send_viewport_cmd(ViewportCommand::Close);
                }
            }
        }

        // Hotkeys (wired in Task 9).
        let mut hotkeys: Vec<Hotkey> = Vec::new();
        if let Some(rx) = &self.hotkeys_rx {
            while let Ok(hk) = rx.try_recv() {
                hotkeys.push(hk);
            }
        }
        for hk in hotkeys {
            self.handle_hotkey(hk, &ctx);
        }

        self.drain_events();
        self.maybe_auto_reset();

        // HWND discovery + flag re-apply.
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
        ui.painter().rect_filled(rect, CornerRadius::ZERO, bg);

        ui.horizontal(|ui| {
            ui.label(
                RichText::new("CuePal")
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

        match self.state.state() {
            AppState::Home => self.render_home(ui),
            AppState::Active { id } => self.render_active(ui, id),
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

        ctx.request_repaint_after(Duration::from_millis(100));
    }
}

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
