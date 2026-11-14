//! Global hotkeys via RegisterHotKey on a dedicated message-pump thread.

#![allow(dead_code)]

use std::sync::mpsc;

pub const MOD_ALT: u32 = 0x0001;
pub const MOD_CONTROL: u32 = 0x0002;
pub const MOD_SHIFT: u32 = 0x0004;
pub const VK_RIGHT: u32 = 0x27;
pub const VK_LEFT: u32 = 0x25;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum Hotkey {
    Toggle = 0,
    Home = 1,
    Pin = 2,
    Next = 3,
    Prev = 4,
    MoveMode = 5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HotkeyBinding {
    pub id: Hotkey,
    pub modifiers: u32,
    pub vk: u32,
}

#[derive(Debug)]
pub enum HotkeyError {
    Conflict(Vec<Hotkey>),
    Thread(String),
}

pub struct HotkeySession {
    pub conflicts: Vec<Hotkey>,
    thread_id: Option<u32>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Drop for HotkeySession {
    fn drop(&mut self) {
        #[cfg(windows)]
        if let Some(tid) = self.thread_id.take() {
            unsafe {
                use windows::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};
                let _ = PostThreadMessageW(
                    tid,
                    WM_QUIT,
                    windows::Win32::Foundation::WPARAM(0),
                    windows::Win32::Foundation::LPARAM(0),
                );
            }
        }
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// Parse "Ctrl+Alt+G", "Ctrl+Alt+Shift+Right", etc. Returns (modifiers, vk).
pub fn parse_hotkey(s: &str) -> Option<(u32, u32)> {
    if s.trim().is_empty() {
        return None;
    }
    let mut modifiers: u32 = 0;
    let mut key_vk: Option<u32> = None;
    for raw in s.split('+') {
        let token = raw.trim();
        if token.is_empty() {
            return None;
        }
        match token.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => modifiers |= MOD_CONTROL,
            "alt" => modifiers |= MOD_ALT,
            "shift" => modifiers |= MOD_SHIFT,
            "right" => key_vk = Some(VK_RIGHT),
            "left" => key_vk = Some(VK_LEFT),
            "space" => key_vk = Some(0x20),
            "esc" | "escape" => key_vk = Some(0x1B),
            "home" => key_vk = Some(0x24),
            "end" => key_vk = Some(0x23),
            "pageup" => key_vk = Some(0x21),
            "pagedown" => key_vk = Some(0x22),
            "up" => key_vk = Some(0x26),
            "down" => key_vk = Some(0x28),
            t => {
                let chars: Vec<char> = t.chars().collect();
                if chars.len() == 1 {
                    let c = chars[0].to_ascii_uppercase();
                    if c.is_ascii_alphabetic() || c.is_ascii_digit() {
                        key_vk = Some(c as u32);
                    } else {
                        return None;
                    }
                } else {
                    return None;
                }
            }
        }
    }
    let vk = key_vk?;
    if modifiers == 0 {
        return None;
    }
    Some((modifiers, vk))
}

/// Return hotkey ids that share a (modifiers, vk) binding with another id.
pub fn detect_conflicts(bindings: &[HotkeyBinding]) -> Vec<Hotkey> {
    use std::collections::HashMap;
    let mut groups: HashMap<(u32, u32), Vec<Hotkey>> = HashMap::new();
    for b in bindings {
        groups.entry((b.modifiers, b.vk)).or_default().push(b.id);
    }
    let mut out: Vec<Hotkey> = Vec::new();
    for (_k, ids) in groups {
        if ids.len() > 1 {
            out.extend(ids);
        }
    }
    out
}

/// Default M1 hotkey bindings (conflict-avoidant).
pub fn default_bindings() -> Vec<HotkeyBinding> {
    vec![
        HotkeyBinding {
            id: Hotkey::Toggle,
            modifiers: MOD_CONTROL | MOD_ALT,
            vk: 0x47,
        }, // G
        HotkeyBinding {
            id: Hotkey::Home,
            modifiers: MOD_CONTROL | MOD_ALT,
            vk: 0x48,
        }, // H
        HotkeyBinding {
            id: Hotkey::Pin,
            modifiers: MOD_CONTROL | MOD_ALT,
            vk: 0x50,
        }, // P
        HotkeyBinding {
            id: Hotkey::Next,
            modifiers: MOD_CONTROL | MOD_ALT | MOD_SHIFT,
            vk: VK_RIGHT,
        },
        HotkeyBinding {
            id: Hotkey::Prev,
            modifiers: MOD_CONTROL | MOD_ALT | MOD_SHIFT,
            vk: VK_LEFT,
        },
        HotkeyBinding {
            id: Hotkey::MoveMode,
            modifiers: MOD_CONTROL | MOD_ALT,
            vk: 0x4D,
        }, // M
    ]
}

#[cfg(windows)]
pub fn start_hotkeys(
    tx: mpsc::Sender<Hotkey>,
    bindings: Vec<HotkeyBinding>,
) -> Result<HotkeySession, HotkeyError> {
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, TranslateMessage, MSG, WM_HOTKEY,
    };

    let conflicts = detect_conflicts(&bindings);
    let conflict_set: std::collections::HashSet<Hotkey> = conflicts.iter().copied().collect();

    let (tid_tx, tid_rx) = mpsc::channel::<u32>();
    let (ready_tx, ready_rx) = mpsc::channel::<Vec<Hotkey>>();

    let handle = std::thread::Builder::new()
        .name("cuepal-hotkeys".to_string())
        .spawn(move || {
            let tid = unsafe { GetCurrentThreadId() };
            let _ = tid_tx.send(tid);

            let mut runtime_conflicts: Vec<Hotkey> = Vec::new();
            for b in &bindings {
                if conflict_set.contains(&b.id) {
                    continue; // skip known duplicates
                }
                let res = unsafe {
                    RegisterHotKey(None, b.id as i32, HOT_KEY_MODIFIERS(b.modifiers), b.vk)
                };
                if res.is_err() {
                    log::warn!("hotkey registration failed for {:?}", b.id);
                    runtime_conflicts.push(b.id);
                }
            }
            let _ = ready_tx.send(runtime_conflicts);

            let mut msg = MSG::default();
            loop {
                let ret = unsafe { GetMessageW(&mut msg, None, 0, 0) };
                if ret.0 <= 0 {
                    break;
                }
                if msg.message == WM_HOTKEY {
                    let id = msg.wParam.0 as i32;
                    // Map back to Hotkey by discriminant order
                    let hk = match id {
                        0 => Some(Hotkey::Toggle),
                        1 => Some(Hotkey::Home),
                        2 => Some(Hotkey::Pin),
                        3 => Some(Hotkey::Next),
                        4 => Some(Hotkey::Prev),
                        5 => Some(Hotkey::MoveMode),
                        _ => None,
                    };
                    if let Some(hk) = hk {
                        let _ = tx.send(hk);
                    }
                }
                unsafe {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            // Unregister on exit
            for b in &bindings {
                let _ = unsafe { UnregisterHotKey(None, b.id as i32) };
            }
        })
        .map_err(|e| HotkeyError::Thread(e.to_string()))?;

    let thread_id = tid_rx.recv().ok();
    let mut all_conflicts = conflicts;
    if let Ok(mut rc) = ready_rx.recv() {
        all_conflicts.append(&mut rc);
    }

    Ok(HotkeySession {
        conflicts: all_conflicts,
        thread_id,
        handle: Some(handle),
    })
}

#[cfg(not(windows))]
pub fn start_hotkeys(
    _tx: mpsc::Sender<Hotkey>,
    _bindings: Vec<HotkeyBinding>,
) -> Result<HotkeySession, HotkeyError> {
    Ok(HotkeySession {
        conflicts: Vec::new(),
        thread_id: None,
        handle: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ctrl_alt_g() {
        assert_eq!(
            parse_hotkey("Ctrl+Alt+G"),
            Some((MOD_CONTROL | MOD_ALT, 0x47))
        );
    }

    #[test]
    fn parse_ctrl_alt_shift_arrows() {
        assert_eq!(
            parse_hotkey("Ctrl+Alt+Shift+Right"),
            Some((MOD_CONTROL | MOD_ALT | MOD_SHIFT, VK_RIGHT))
        );
    }

    #[test]
    fn parse_rejects_unknown() {
        assert_eq!(parse_hotkey("Ctrl+Alt+Zzz"), None);
        assert_eq!(parse_hotkey(""), None);
    }

    #[test]
    fn detect_duplicate_bindings() {
        let b = vec![
            HotkeyBinding {
                id: Hotkey::Home,
                modifiers: MOD_CONTROL | MOD_ALT,
                vk: 0x48,
            },
            HotkeyBinding {
                id: Hotkey::Pin,
                modifiers: MOD_CONTROL | MOD_ALT,
                vk: 0x48,
            }, // clash
        ];
        let c = detect_conflicts(&b);
        assert!(c.contains(&Hotkey::Home) || c.contains(&Hotkey::Pin));
    }
}
