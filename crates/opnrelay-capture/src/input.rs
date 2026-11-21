// SPDX-License-Identifier: AGPL-3.0-only
//! Input observation by polling (no global hooks, no keystroke logging).
//!
//! The pure [`Detector`] turns successive mouse/keyboard *states* into
//! events; [`Watcher`] feeds it from the OS on a background thread.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use device_query::{DeviceQuery, DeviceState, Keycode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputEvent {
    Click {
        button: Button,
        x: i32,
        y: i32,
    },
    /// The previous click was the first half of a double-click.
    DoubleClick {
        x: i32,
        y: i32,
    },
    /// A keyboard shortcut like "Ctrl+S".
    Shortcut {
        keys: String,
    },
    /// The user typed some text and stopped (content is not recorded).
    TypingFinished {
        x: i32,
        y: i32,
    },
}

const DOUBLE_CLICK: Duration = Duration::from_millis(450);
const DOUBLE_CLICK_DIST: i32 = 6;
const TYPING_IDLE: Duration = Duration::from_millis(1300);

/// Snapshot of input devices.
#[derive(Debug, Clone, Default)]
pub struct InputState {
    pub x: i32,
    pub y: i32,
    pub left: bool,
    pub right: bool,
    pub keys: Vec<Keycode>,
}

fn is_modifier(k: &Keycode) -> bool {
    use Keycode::*;
    matches!(
        k,
        LControl
            | RControl
            | LShift
            | RShift
            | LAlt
            | RAlt
            | Command
            | RCommand
            | LOption
            | ROption
            | LMeta
            | RMeta
    )
}

fn is_printable(k: &Keycode) -> bool {
    use Keycode::*;
    let name = format!("{k:?}");
    (name.len() == 1)
        || name.starts_with("Key") && name.len() == 4
        || matches!(
            k,
            Space
                | Minus
                | Equal
                | LeftBracket
                | RightBracket
                | BackSlash
                | Semicolon
                | Apostrophe
                | Comma
                | Dot
                | Slash
                | Grave
        )
        || name.starts_with("Numpad") && name.len() == 7
}

fn key_name(k: &Keycode) -> String {
    let n = format!("{k:?}");
    n.strip_prefix("Key")
        .filter(|s| s.len() == 1)
        .map(str::to_string)
        .unwrap_or(n)
}

/// Turns input state samples into high-level events.
#[derive(Default)]
pub struct Detector {
    prev: InputState,
    last_click: Option<(Instant, i32, i32)>,
    typing_since: Option<Instant>,
    last_key: Option<Instant>,
}

impl Detector {
    pub fn update(&mut self, s: InputState, now: Instant) -> Vec<InputEvent> {
        let mut out = Vec::new();
        let pressed_left = s.left && !self.prev.left;
        let pressed_right = s.right && !self.prev.right;
        if pressed_left || pressed_right {
            // A click ends any typing burst first.
            if self.typing_since.take().is_some() {
                out.push(InputEvent::TypingFinished {
                    x: self.prev.x,
                    y: self.prev.y,
                });
            }
            if pressed_left
                && let Some((t, px, py)) = self.last_click
                && now.duration_since(t) <= DOUBLE_CLICK
                && (s.x - px).abs() <= DOUBLE_CLICK_DIST
                && (s.y - py).abs() <= DOUBLE_CLICK_DIST
            {
                out.push(InputEvent::DoubleClick { x: s.x, y: s.y });
                self.last_click = None;
            } else {
                let button = if pressed_left {
                    Button::Left
                } else {
                    Button::Right
                };
                out.push(InputEvent::Click {
                    button,
                    x: s.x,
                    y: s.y,
                });
                self.last_click = pressed_left.then_some((now, s.x, s.y));
            }
        }

        let newly: Vec<&Keycode> = s
            .keys
            .iter()
            .filter(|k| !self.prev.keys.contains(k))
            .collect();
        if !newly.is_empty() {
            let mods: Vec<&Keycode> = s.keys.iter().filter(|k| is_modifier(k)).collect();
            let has_cmd_mod = mods
                .iter()
                .any(|k| !matches!(k, Keycode::LShift | Keycode::RShift));
            for k in newly.iter().filter(|k| !is_modifier(k)) {
                if has_cmd_mod {
                    let mut parts: Vec<&str> = Vec::new();
                    if mods
                        .iter()
                        .any(|m| matches!(m, Keycode::LControl | Keycode::RControl))
                    {
                        parts.push("Ctrl");
                    }
                    if mods.iter().any(|m| {
                        matches!(
                            m,
                            Keycode::Command | Keycode::RCommand | Keycode::LMeta | Keycode::RMeta
                        )
                    }) {
                        parts.push("⌘");
                    }
                    if mods.iter().any(|m| {
                        matches!(
                            m,
                            Keycode::LAlt | Keycode::RAlt | Keycode::LOption | Keycode::ROption
                        )
                    }) {
                        parts.push("Alt");
                    }
                    if mods
                        .iter()
                        .any(|m| matches!(m, Keycode::LShift | Keycode::RShift))
                    {
                        parts.push("Shift");
                    }
                    let name = key_name(k);
                    let mut keys = parts.join("+");
                    keys.push('+');
                    keys.push_str(&name);
                    out.push(InputEvent::Shortcut { keys });
                } else if is_printable(k) || matches!(k, Keycode::Backspace) {
                    self.typing_since.get_or_insert(now);
                    self.last_key = Some(now);
                } else if matches!(k, Keycode::Enter | Keycode::Tab) {
                    if self.typing_since.take().is_some() {
                        out.push(InputEvent::TypingFinished { x: s.x, y: s.y });
                    }
                    if matches!(k, Keycode::Enter) {
                        out.push(InputEvent::Shortcut {
                            keys: "Enter".into(),
                        });
                    }
                }
            }
        }
        if self.typing_since.is_some()
            && self
                .last_key
                .is_some_and(|t| now.duration_since(t) >= TYPING_IDLE)
        {
            self.typing_since = None;
            out.push(InputEvent::TypingFinished { x: s.x, y: s.y });
        }
        self.prev = s;
        out
    }
}

/// Polls the OS input state on a background thread.
pub struct Watcher;

impl Watcher {
    /// Runs until `stop` is set. Returns an error if input cannot be observed
    /// (e.g. missing accessibility permission on macOS, or Wayland).
    pub fn spawn(
        tx: Sender<InputEvent>,
        stop: Arc<AtomicBool>,
    ) -> Result<std::thread::JoinHandle<()>, String> {
        // The OS handle is not thread-safe, so it is created on the polling
        // thread; startup success is reported back before returning.
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<Result<(), String>>(1);
        let handle = std::thread::spawn(move || {
            let Some(state) = DeviceState::checked_new() else {
                let _ = ready_tx.send(Err(
                    "cannot observe the mouse and keyboard. On macOS allow OpnRelay under System Settings → Privacy & Security → Accessibility; on Linux an X11 session is required".to_string(),
                ));
                return;
            };
            let _ = ready_tx.send(Ok(()));
            let mut det = Detector::default();
            while !stop.load(Ordering::Relaxed) {
                let m = state.get_mouse();
                let s = InputState {
                    x: m.coords.0,
                    y: m.coords.1,
                    left: m.button_pressed.get(1).copied().unwrap_or(false),
                    right: m.button_pressed.get(3).copied().unwrap_or(false),
                    keys: state.get_keys(),
                };
                for ev in det.update(s, Instant::now()) {
                    if tx.send(ev).is_err() {
                        return;
                    }
                }
                std::thread::sleep(Duration::from_millis(8));
            }
        });
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(handle),
            Ok(Err(e)) => Err(e),
            Err(_) => Err("input watcher failed to start".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(x: i32, y: i32, left: bool, keys: Vec<Keycode>) -> InputState {
        InputState {
            x,
            y,
            left,
            right: false,
            keys,
        }
    }

    #[test]
    fn clicks_and_double_clicks() {
        let mut d = Detector::default();
        let t0 = Instant::now();
        assert!(d.update(st(10, 10, false, vec![]), t0).is_empty());
        assert_eq!(
            d.update(st(10, 10, true, vec![]), t0),
            vec![InputEvent::Click {
                button: Button::Left,
                x: 10,
                y: 10
            }]
        );
        assert!(
            d.update(st(10, 10, true, vec![]), t0).is_empty(),
            "held button is one click"
        );
        d.update(st(10, 10, false, vec![]), t0);
        assert_eq!(
            d.update(st(12, 11, true, vec![]), t0 + Duration::from_millis(200)),
            vec![InputEvent::DoubleClick { x: 12, y: 11 }]
        );
        d.update(st(12, 11, false, vec![]), t0);
        let late = d.update(st(12, 11, true, vec![]), t0 + Duration::from_secs(2));
        assert!(matches!(late[0], InputEvent::Click { .. }));
    }

    #[test]
    fn shortcuts_and_typing_without_content() {
        let mut d = Detector::default();
        let t0 = Instant::now();
        let ev = d.update(st(0, 0, false, vec![Keycode::LControl, Keycode::S]), t0);
        assert_eq!(
            ev,
            vec![InputEvent::Shortcut {
                keys: "Ctrl+S".into()
            }]
        );
        d.update(st(0, 0, false, vec![]), t0);
        assert!(d.update(st(5, 5, false, vec![Keycode::H]), t0).is_empty());
        d.update(st(5, 5, false, vec![]), t0);
        assert!(
            d.update(
                st(5, 5, false, vec![Keycode::I]),
                t0 + Duration::from_millis(100)
            )
            .is_empty()
        );
        let done = d.update(st(5, 5, false, vec![]), t0 + Duration::from_secs(3));
        assert_eq!(done, vec![InputEvent::TypingFinished { x: 5, y: 5 }]);
        // Shift alone is not a shortcut (capital letters while typing).
        assert!(
            d.update(
                st(5, 5, false, vec![Keycode::LShift, Keycode::A]),
                t0 + Duration::from_secs(4)
            )
            .is_empty()
        );
    }

    #[test]
    fn click_ends_typing_first() {
        let mut d = Detector::default();
        let t0 = Instant::now();
        d.update(st(1, 1, false, vec![Keycode::A]), t0);
        d.update(st(1, 1, false, vec![]), t0);
        let ev = d.update(st(50, 60, true, vec![]), t0 + Duration::from_millis(300));
        assert_eq!(ev[0], InputEvent::TypingFinished { x: 1, y: 1 });
        assert!(matches!(ev[1], InputEvent::Click { x: 50, y: 60, .. }));
    }
}
