//! Configuration management for Rivulet TUI.
//!
//! Configuration is read from the platform configuration directory at startup.
//! If the file doesn't exist, a default configuration with comments is created.

pub mod colors;
pub mod keybindings;

pub use colors::ColorConfig;
pub use keybindings::KeybindingConfig;

use crate::scraper::ScraperConfig;
use serde::Deserialize;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

/// Main configuration struct.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub colors: ColorConfig,
    pub keybindings: KeybindingConfig,
    pub scraper: ScraperConfig,
    pub ui: UiConfig,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct UiConfig {
    pub latest: LatestConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct LatestConfig {
    pub days: u32,
    pub limit: usize,
}

impl Default for LatestConfig {
    fn default() -> Self {
        Self {
            days: 7,
            limit: 200,
        }
    }
}

impl Config {
    /// Load configuration from the default path.
    ///
    /// If the config file doesn't exist, creates a default one with comments.
    /// If the config file exists but is invalid, returns an error.
    /// Missing fields in the config file will use default values.
    pub fn load() -> Result<Self, ConfigError> {
        let config_path = Self::default_config_path()?;

        if !config_path.exists() {
            // Create default config with comments
            Self::create_default_config(&config_path)?;
            return Ok(Self::default());
        }

        let content = fs::read_to_string(&config_path).map_err(|e| ConfigError::Io {
            path: config_path.clone(),
            source: e,
        })?;

        let config: Config = toml::from_str(&content).map_err(|e| ConfigError::Parse {
            path: config_path,
            source: e,
        })?;
        config.scraper.validate().map_err(ConfigError::Validation)?;

        Ok(config)
    }

    /// Get the default config file path under the platform configuration directory.
    pub fn default_config_path() -> Result<PathBuf, ConfigError> {
        let config_dir = dirs::config_dir().ok_or(ConfigError::NoConfigDir)?;
        Ok(config_dir.join("rivulet").join("config.toml"))
    }

    /// Create a default config file with comments.
    fn create_default_config(path: &PathBuf) -> Result<(), ConfigError> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| ConfigError::Io {
                path: parent.to_path_buf(),
                source: e,
            })?;
        }

        let default_config = Self::default_config_content();

        let mut file = fs::File::create(path).map_err(|e| ConfigError::Io {
            path: path.clone(),
            source: e,
        })?;

        file.write_all(default_config.as_bytes())
            .map_err(|e| ConfigError::Io {
                path: path.clone(),
                source: e,
            })?;

        Ok(())
    }

    /// Generate the default config file content with comments.
    fn default_config_content() -> String {
        r##"# Rivulet TUI Configuration
#
# Colors can be specified as:
# - Named colors: Black, Red, Green, Yellow, Blue, Magenta, Cyan, Gray,
#   DarkGray, LightRed, LightGreen, LightYellow, LightBlue, LightMagenta,
#   LightCyan, White, Reset
# - Hex colors: "#RRGGBB" or "#RGB"
#
# Keybindings can be specified as:
# - Single characters: "a", "A", "1"
# - Special keys: Enter, Tab, BackTab, Backspace, Delete, Home, End,
#   PageUp, PageDown, Up, Down, Left, Right, Esc, Space, F1-F12
# - With modifiers: "Ctrl+c", "Shift+Tab", "Alt+Enter"

[colors]
# Border colors
active_border = "Cyan"
inactive_border = "DarkGray"

# Selection highlight
selection_bg_active = "Cyan"
selection_fg_active = "Black"
selection_bg_inactive = "DarkGray"
selection_fg_inactive = "White"

# Item colors
read_item = "DarkGray"
unread_item = "White"

# Metadata colors in preview
metadata_author = "Yellow"
metadata_date = "Yellow"
metadata_link = "Blue"

# Status bar
status_fg = "White"
status_bg = "DarkGray"

[keybindings]
# Navigation
quit = ["q", "Ctrl+c"]
move_up = ["k", "Up"]
move_down = ["j", "Down"]
move_top = ["g"]
move_bottom = ["G", "%"]
next_page = ["n", "PageDown"]
prev_page = ["p", "PageUp"]
next_pane = ["Tab"]
prev_pane = ["BackTab", "Shift+Tab"]
# Single-key directional focus. h/l are vim-style aliases for Left/Right.
pane_left = ["Left", "h"]
pane_right = ["Right", "l"]
# vim-style window chord: press, then h (left) / l (right) / Esc (cancel)
window_chord = ["Ctrl+w"]

# Actions
select = ["Enter"]
toggle_read = ["r"]
toggle_star = ["s"]
toggle_queued = ["L"]
toggle_saved = ["S"]
toggle_archived = ["x"]
view_all = ["a"]
view_unread = ["u"]
view_starred = ["f"]
view_queued = ["Q"]
view_saved = ["v"]
view_archived = ["X"]
view_latest = ["Alt+1", "["]
view_reader = ["Alt+2", "]"]
open_in_browser = ["o"]
refresh = ["R"]
toggle_maximize = ["m"]
toggle_feed_panel = ["\\"]
delete_feed = ["d", "Delete"]

[ui.latest]
# Number of days included in the Latest tab.
days = 7

# Maximum non-new items shown below the latest refresh batch.
limit = 200

[scraper]
# Run browser in headless mode (no visible window)
headless = true

# Page load timeout in seconds
timeout_secs = 30

# Wait time after page load for dynamic content (milliseconds)
wait_after_load_ms = 1000

# Maximum concurrent browser pages
max_concurrency = 5

# Maximum extracted article size in bytes (2 MiB)
max_content_bytes = 2097152

# Block images for faster loading
block_images = true

# Block stylesheets for faster loading
block_stylesheets = true

# CSS selectors to try for article content extraction (in priority order)
content_selectors = [
    "article",
    "[role=\"main\"]",
    "main",
    ".post-content",
    ".article-content",
    ".entry-content",
    ".content",
    "#content",
    ".post",
    ".article",
]

# Elements to remove before extraction (ads, navigation, etc.)
remove_selectors = [
    "nav",
    "header",
    "footer",
    "aside",
    ".sidebar",
    ".advertisement",
    ".ad",
    ".ads",
    ".social-share",
    ".comments",
    "script",
    "style",
]

# Optional: Persistent Chrome profile directory used for authenticated scraping.
# Prefer `rivulet auth add <name> --site <url>` and `rivulet scrape --auth-profile <name>`
# for named paid/private-site sessions.
# user_data_dir = "/path/to/rivulet/auth-profiles/example"
"##
        .to_string()
    }
}

/// Configuration errors.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Could not determine config directory")]
    NoConfigDir,

    #[error("Failed to read/write config file at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("Failed to parse config file at {path}: {source}")]
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },

    #[error("Invalid configuration: {0}")]
    Validation(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_deserializes() {
        let content = Config::default_config_content();
        let config: Config = toml::from_str(&content).expect("Default config should be valid TOML");

        // Check a few values
        assert_eq!(config.colors.active_border, ratatui::style::Color::Cyan);
        assert_eq!(config.keybindings.quit, vec!["q", "Ctrl+c"]);
    }

    #[test]
    fn test_partial_config() {
        let content = r##"
[colors]
active_border = "#FF0000"
"##;
        let config: Config = toml::from_str(content).expect("Partial config should work");

        // Custom value
        assert_eq!(
            config.colors.active_border,
            ratatui::style::Color::Rgb(255, 0, 0)
        );
        // Default value
        assert_eq!(
            config.colors.inactive_border,
            ratatui::style::Color::DarkGray
        );
    }

    #[test]
    fn test_empty_config() {
        let content = "";
        let config: Config = toml::from_str(content).expect("Empty config should work");

        // All defaults
        assert_eq!(config.colors.active_border, ratatui::style::Color::Cyan);
        assert_eq!(config.keybindings.quit, vec!["q", "Ctrl+c"]);
    }
}
