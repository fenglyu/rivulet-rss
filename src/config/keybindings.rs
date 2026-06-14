//! Keybinding configuration for the TUI.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::Deserialize;

use crate::tui::event::Action;

/// Configuration for all keybindings.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct KeybindingConfig {
    pub quit: Vec<String>,
    pub move_up: Vec<String>,
    pub move_down: Vec<String>,
    pub move_top: Vec<String>,
    pub move_bottom: Vec<String>,
    pub next_page: Vec<String>,
    pub prev_page: Vec<String>,
    pub next_pane: Vec<String>,
    pub prev_pane: Vec<String>,
    pub pane_left: Vec<String>,
    pub pane_right: Vec<String>,
    pub select: Vec<String>,
    pub toggle_read: Vec<String>,
    pub toggle_star: Vec<String>,
    pub toggle_queued: Vec<String>,
    pub toggle_saved: Vec<String>,
    pub toggle_archived: Vec<String>,
    pub view_all: Vec<String>,
    pub view_unread: Vec<String>,
    pub view_starred: Vec<String>,
    pub view_queued: Vec<String>,
    pub view_saved: Vec<String>,
    pub view_archived: Vec<String>,
    pub view_latest: Vec<String>,
    pub view_reader: Vec<String>,
    pub open_in_browser: Vec<String>,
    pub refresh: Vec<String>,
    pub toggle_maximize: Vec<String>,
    pub toggle_feed_panel: Vec<String>,
    pub delete_feed: Vec<String>,
    pub window_chord: Vec<String>,
}

impl Default for KeybindingConfig {
    fn default() -> Self {
        Self {
            quit: vec!["q".to_string(), "Ctrl+c".to_string()],
            move_up: vec!["k".to_string(), "Up".to_string()],
            move_down: vec!["j".to_string(), "Down".to_string()],
            move_top: vec!["g".to_string()],
            move_bottom: vec!["G".to_string(), "%".to_string()],
            next_page: vec!["n".to_string(), "PageDown".to_string()],
            prev_page: vec!["p".to_string(), "PageUp".to_string()],
            next_pane: vec!["Tab".to_string()],
            prev_pane: vec!["BackTab".to_string(), "Shift+Tab".to_string()],
            pane_left: vec!["Left".to_string(), "h".to_string()],
            pane_right: vec!["Right".to_string(), "l".to_string()],
            select: vec!["Enter".to_string()],
            toggle_read: vec!["r".to_string()],
            toggle_star: vec!["s".to_string()],
            toggle_queued: vec!["L".to_string()],
            toggle_saved: vec!["S".to_string()],
            toggle_archived: vec!["x".to_string()],
            view_all: vec!["a".to_string()],
            view_unread: vec!["u".to_string()],
            view_starred: vec!["f".to_string()],
            view_queued: vec!["Q".to_string()],
            view_saved: vec!["v".to_string()],
            view_archived: vec!["X".to_string()],
            view_latest: vec!["Alt+1".to_string(), "[".to_string()],
            view_reader: vec!["Alt+2".to_string(), "]".to_string()],
            open_in_browser: vec!["o".to_string()],
            refresh: vec!["R".to_string()],
            toggle_maximize: vec!["m".to_string()],
            toggle_feed_panel: vec!["\\".to_string()],
            delete_feed: vec!["d".to_string(), "Delete".to_string()],
            window_chord: vec!["Ctrl+w".to_string()],
        }
    }
}

impl KeybindingConfig {
    /// Get the action for a key event.
    pub fn get_action(&self, key: &KeyEvent) -> Action {
        if self.matches_key(key, &self.quit) {
            Action::Quit
        } else if self.matches_key(key, &self.move_up) {
            Action::MoveUp
        } else if self.matches_key(key, &self.move_down) {
            Action::MoveDown
        } else if self.matches_key(key, &self.move_top) {
            Action::MoveTop
        } else if self.matches_key(key, &self.move_bottom) {
            Action::MoveBottom
        } else if self.matches_key(key, &self.next_page) {
            Action::NextPage
        } else if self.matches_key(key, &self.prev_page) {
            Action::PrevPage
        } else if self.matches_key(key, &self.next_pane) {
            Action::NextPane
        } else if self.matches_key(key, &self.prev_pane) {
            Action::PrevPane
        } else if self.matches_key(key, &self.pane_left) {
            Action::FocusLeft
        } else if self.matches_key(key, &self.pane_right) {
            Action::FocusRight
        } else if self.matches_key(key, &self.select) {
            Action::Select
        } else if self.matches_key(key, &self.toggle_read) {
            Action::ToggleRead
        } else if self.matches_key(key, &self.toggle_star) {
            Action::ToggleStar
        } else if self.matches_key(key, &self.toggle_queued) {
            Action::ToggleQueued
        } else if self.matches_key(key, &self.toggle_saved) {
            Action::ToggleSaved
        } else if self.matches_key(key, &self.toggle_archived) {
            Action::ToggleArchived
        } else if self.matches_key(key, &self.view_all) {
            Action::ViewAll
        } else if self.matches_key(key, &self.view_unread) {
            Action::ViewUnread
        } else if self.matches_key(key, &self.view_starred) {
            Action::ViewStarred
        } else if self.matches_key(key, &self.view_queued) {
            Action::ViewQueued
        } else if self.matches_key(key, &self.view_saved) {
            Action::ViewSaved
        } else if self.matches_key(key, &self.view_archived) {
            Action::ViewArchived
        } else if self.matches_key(key, &self.view_latest) {
            Action::ViewLatest
        } else if self.matches_key(key, &self.view_reader) {
            Action::ViewReader
        } else if self.matches_key(key, &self.open_in_browser) {
            Action::OpenInBrowser
        } else if self.matches_key(key, &self.refresh) {
            Action::Refresh
        } else if self.matches_key(key, &self.toggle_maximize) {
            Action::ToggleMaximize
        } else if self.matches_key(key, &self.toggle_feed_panel) {
            Action::ToggleFeedPanel
        } else if self.matches_key(key, &self.delete_feed) {
            Action::DeleteFeed
        } else if self.matches_key(key, &self.window_chord) {
            Action::WindowChord
        } else {
            Action::None
        }
    }

    fn matches_key(&self, key: &KeyEvent, bindings: &[String]) -> bool {
        bindings.iter().any(|binding| {
            if let Ok(parsed) = parse_key_string(binding) {
                parsed.matches(key)
            } else {
                false
            }
        })
    }
}

/// A parsed key binding with code and modifiers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyBinding {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
}

impl KeyBinding {
    /// Check if this binding matches a key event.
    pub fn matches(&self, key: &KeyEvent) -> bool {
        self.code == key.code
            && (self.modifiers == key.modifiers
                || self.modifiers == (key.modifiers & !KeyModifiers::SHIFT))
    }
}

/// Parse a key string into a KeyBinding.
///
/// Supported formats:
/// - Single characters: "a", "A", "1", "/"
/// - Special keys: "Enter", "Tab", "BackTab", "Backspace", "Delete", "Home", "End",
///   "PageUp", "PageDown", "Up", "Down", "Left", "Right", "Esc", "Space", "F1"-"F12"
/// - With modifiers: "Ctrl+c", "Shift+Tab", "Alt+Enter", "Ctrl+Shift+a"
pub fn parse_key_string(s: &str) -> Result<KeyBinding, String> {
    let s = s.trim();
    let parts: Vec<&str> = s.split('+').collect();

    let mut modifiers = KeyModifiers::NONE;
    let key_part = if parts.len() > 1 {
        // Parse modifiers
        for part in &parts[..parts.len() - 1] {
            match part.to_lowercase().as_str() {
                "ctrl" | "control" => modifiers |= KeyModifiers::CONTROL,
                "shift" => modifiers |= KeyModifiers::SHIFT,
                "alt" => modifiers |= KeyModifiers::ALT,
                _ => return Err(format!("Unknown modifier: {}", part)),
            }
        }
        parts[parts.len() - 1]
    } else {
        s
    };

    let code = parse_key_code(key_part)?;

    Ok(KeyBinding { code, modifiers })
}

fn parse_key_code(s: &str) -> Result<KeyCode, String> {
    // Check for single character
    if s.len() == 1 {
        let c = s.chars().next().unwrap();
        return Ok(KeyCode::Char(c));
    }

    // Check for special keys (case-insensitive)
    match s.to_lowercase().as_str() {
        "enter" | "return" => Ok(KeyCode::Enter),
        "tab" => Ok(KeyCode::Tab),
        "backtab" => Ok(KeyCode::BackTab),
        "backspace" | "bs" => Ok(KeyCode::Backspace),
        "delete" | "del" => Ok(KeyCode::Delete),
        "home" => Ok(KeyCode::Home),
        "end" => Ok(KeyCode::End),
        "pageup" | "pgup" => Ok(KeyCode::PageUp),
        "pagedown" | "pgdn" => Ok(KeyCode::PageDown),
        "up" => Ok(KeyCode::Up),
        "down" => Ok(KeyCode::Down),
        "left" => Ok(KeyCode::Left),
        "right" => Ok(KeyCode::Right),
        "esc" | "escape" => Ok(KeyCode::Esc),
        "space" => Ok(KeyCode::Char(' ')),
        "f1" => Ok(KeyCode::F(1)),
        "f2" => Ok(KeyCode::F(2)),
        "f3" => Ok(KeyCode::F(3)),
        "f4" => Ok(KeyCode::F(4)),
        "f5" => Ok(KeyCode::F(5)),
        "f6" => Ok(KeyCode::F(6)),
        "f7" => Ok(KeyCode::F(7)),
        "f8" => Ok(KeyCode::F(8)),
        "f9" => Ok(KeyCode::F(9)),
        "f10" => Ok(KeyCode::F(10)),
        "f11" => Ok(KeyCode::F(11)),
        "f12" => Ok(KeyCode::F(12)),
        _ => Err(format!("Unknown key: {}", s)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_single_char() {
        let binding = parse_key_string("j").unwrap();
        assert_eq!(binding.code, KeyCode::Char('j'));
        assert_eq!(binding.modifiers, KeyModifiers::NONE);
    }

    #[test]
    fn test_parse_uppercase_char() {
        let binding = parse_key_string("R").unwrap();
        assert_eq!(binding.code, KeyCode::Char('R'));
        assert_eq!(binding.modifiers, KeyModifiers::NONE);
    }

    #[test]
    fn test_parse_special_key() {
        let binding = parse_key_string("Enter").unwrap();
        assert_eq!(binding.code, KeyCode::Enter);
        assert_eq!(binding.modifiers, KeyModifiers::NONE);

        let binding = parse_key_string("Tab").unwrap();
        assert_eq!(binding.code, KeyCode::Tab);

        let binding = parse_key_string("BackTab").unwrap();
        assert_eq!(binding.code, KeyCode::BackTab);

        let binding = parse_key_string("PageDown").unwrap();
        assert_eq!(binding.code, KeyCode::PageDown);
    }

    #[test]
    fn test_parse_ctrl_modifier() {
        let binding = parse_key_string("Ctrl+c").unwrap();
        assert_eq!(binding.code, KeyCode::Char('c'));
        assert_eq!(binding.modifiers, KeyModifiers::CONTROL);
    }

    #[test]
    fn test_parse_shift_modifier() {
        let binding = parse_key_string("Shift+Tab").unwrap();
        assert_eq!(binding.code, KeyCode::Tab);
        assert_eq!(binding.modifiers, KeyModifiers::SHIFT);
    }

    #[test]
    fn test_parse_multiple_modifiers() {
        let binding = parse_key_string("Ctrl+Shift+a").unwrap();
        assert_eq!(binding.code, KeyCode::Char('a'));
        assert_eq!(
            binding.modifiers,
            KeyModifiers::CONTROL | KeyModifiers::SHIFT
        );
    }

    #[test]
    fn test_parse_function_keys() {
        let binding = parse_key_string("F1").unwrap();
        assert_eq!(binding.code, KeyCode::F(1));

        let binding = parse_key_string("F12").unwrap();
        assert_eq!(binding.code, KeyCode::F(12));
    }

    #[test]
    fn test_keybinding_matches() {
        let binding = parse_key_string("Ctrl+c").unwrap();
        let key_event = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(binding.matches(&key_event));

        let key_event = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE);
        assert!(!binding.matches(&key_event));
    }

    #[test]
    fn test_keybinding_config_get_action() {
        let config = KeybindingConfig::default();

        let key = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
        assert_eq!(config.get_action(&key), Action::Quit);

        let key = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(config.get_action(&key), Action::Quit);

        let key = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        assert_eq!(config.get_action(&key), Action::MoveDown);

        let key = KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE);
        assert_eq!(config.get_action(&key), Action::MoveTop);

        let key = KeyEvent::new(KeyCode::Char('G'), KeyModifiers::NONE);
        assert_eq!(config.get_action(&key), Action::MoveBottom);

        let key = KeyEvent::new(KeyCode::Char('%'), KeyModifiers::SHIFT);
        assert_eq!(config.get_action(&key), Action::MoveBottom);

        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(config.get_action(&key), Action::Select);

        let key = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE);
        assert_eq!(config.get_action(&key), Action::DeleteFeed);

        let key = KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE);
        assert_eq!(config.get_action(&key), Action::DeleteFeed);

        let key = KeyEvent::new(KeyCode::Char('L'), KeyModifiers::NONE);
        assert_eq!(config.get_action(&key), Action::ToggleQueued);

        let key = KeyEvent::new(KeyCode::Char('S'), KeyModifiers::NONE);
        assert_eq!(config.get_action(&key), Action::ToggleSaved);

        let key = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE);
        assert_eq!(config.get_action(&key), Action::ToggleArchived);

        let key = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        assert_eq!(config.get_action(&key), Action::ViewAll);

        let key = KeyEvent::new(KeyCode::Char('u'), KeyModifiers::NONE);
        assert_eq!(config.get_action(&key), Action::ViewUnread);

        let key = KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE);
        assert_eq!(config.get_action(&key), Action::ViewStarred);

        // `l` now focuses the pane to the right (alongside Right arrow);
        // queue view moved to `Q` so the vim-style hjkl pane navigation can
        // be symmetric.
        let key = KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE);
        assert_eq!(config.get_action(&key), Action::FocusRight);

        let key = KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE);
        assert_eq!(config.get_action(&key), Action::FocusLeft);

        let key = KeyEvent::new(KeyCode::Char('Q'), KeyModifiers::SHIFT);
        assert_eq!(config.get_action(&key), Action::ViewQueued);

        let key = KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE);
        assert_eq!(config.get_action(&key), Action::ViewSaved);

        let key = KeyEvent::new(KeyCode::Char('X'), KeyModifiers::NONE);
        assert_eq!(config.get_action(&key), Action::ViewArchived);

        let key = KeyEvent::new(KeyCode::Char('1'), KeyModifiers::ALT);
        assert_eq!(config.get_action(&key), Action::ViewLatest);

        let key = KeyEvent::new(KeyCode::Char('['), KeyModifiers::NONE);
        assert_eq!(config.get_action(&key), Action::ViewLatest);

        let key = KeyEvent::new(KeyCode::Char('2'), KeyModifiers::ALT);
        assert_eq!(config.get_action(&key), Action::ViewReader);

        let key = KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE);
        assert_eq!(config.get_action(&key), Action::ViewReader);

        let key = KeyEvent::new(KeyCode::Char('\\'), KeyModifiers::NONE);
        assert_eq!(config.get_action(&key), Action::ToggleFeedPanel);

        let key = KeyEvent::new(KeyCode::Char('R'), KeyModifiers::SHIFT);
        assert_eq!(config.get_action(&key), Action::Refresh);

        let key = KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL);
        assert_eq!(config.get_action(&key), Action::WindowChord);

        let key = KeyEvent::new(KeyCode::Left, KeyModifiers::NONE);
        assert_eq!(config.get_action(&key), Action::FocusLeft);

        let key = KeyEvent::new(KeyCode::Right, KeyModifiers::NONE);
        assert_eq!(config.get_action(&key), Action::FocusRight);
    }
}
