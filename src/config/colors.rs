//! Color configuration for the TUI.

use ratatui::style::Color;
use serde::{de, Deserialize, Deserializer};

/// Configuration for all TUI colors.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ColorConfig {
    #[serde(deserialize_with = "deserialize_color")]
    pub active_border: Color,
    #[serde(deserialize_with = "deserialize_color")]
    pub inactive_border: Color,
    #[serde(deserialize_with = "deserialize_color")]
    pub selection_bg_active: Color,
    #[serde(deserialize_with = "deserialize_color")]
    pub selection_fg_active: Color,
    #[serde(deserialize_with = "deserialize_color")]
    pub selection_bg_inactive: Color,
    #[serde(deserialize_with = "deserialize_color")]
    pub selection_fg_inactive: Color,
    #[serde(deserialize_with = "deserialize_color")]
    pub read_item: Color,
    #[serde(deserialize_with = "deserialize_color")]
    pub unread_item: Color,
    #[serde(deserialize_with = "deserialize_color")]
    pub metadata_author: Color,
    #[serde(deserialize_with = "deserialize_color")]
    pub metadata_date: Color,
    #[serde(deserialize_with = "deserialize_color")]
    pub metadata_link: Color,
    #[serde(deserialize_with = "deserialize_color")]
    pub status_fg: Color,
    #[serde(deserialize_with = "deserialize_color")]
    pub status_bg: Color,
}

impl Default for ColorConfig {
    fn default() -> Self {
        Self {
            active_border: Color::Cyan,
            inactive_border: Color::DarkGray,
            selection_bg_active: Color::Cyan,
            selection_fg_active: Color::Black,
            selection_bg_inactive: Color::DarkGray,
            selection_fg_inactive: Color::White,
            read_item: Color::DarkGray,
            unread_item: Color::White,
            metadata_author: Color::Yellow,
            metadata_date: Color::Yellow,
            metadata_link: Color::Blue,
            status_fg: Color::White,
            status_bg: Color::DarkGray,
        }
    }
}

/// Custom deserializer for Color that supports named colors and hex codes.
fn deserialize_color<'de, D>(deserializer: D) -> Result<Color, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    parse_color_string(&s).map_err(de::Error::custom)
}

/// Parse a color string into a ratatui Color.
///
/// Supports:
/// - Named colors: "Black", "Red", "Green", "Yellow", "Blue", "Magenta", "Cyan", "Gray",
///   "DarkGray", "LightRed", "LightGreen", "LightYellow", "LightBlue", "LightMagenta",
///   "LightCyan", "White", "Reset"
/// - Hex colors: "#RRGGBB" or "#RGB"
pub fn parse_color_string(s: &str) -> Result<Color, String> {
    let s = s.trim();

    // Handle hex colors
    if s.starts_with('#') {
        return parse_hex_color(s);
    }

    // Handle named colors (case-insensitive)
    match s.to_lowercase().as_str() {
        "black" => Ok(Color::Black),
        "red" => Ok(Color::Red),
        "green" => Ok(Color::Green),
        "yellow" => Ok(Color::Yellow),
        "blue" => Ok(Color::Blue),
        "magenta" => Ok(Color::Magenta),
        "cyan" => Ok(Color::Cyan),
        "gray" | "grey" => Ok(Color::Gray),
        "darkgray" | "darkgrey" => Ok(Color::DarkGray),
        "lightred" => Ok(Color::LightRed),
        "lightgreen" => Ok(Color::LightGreen),
        "lightyellow" => Ok(Color::LightYellow),
        "lightblue" => Ok(Color::LightBlue),
        "lightmagenta" => Ok(Color::LightMagenta),
        "lightcyan" => Ok(Color::LightCyan),
        "white" => Ok(Color::White),
        "reset" => Ok(Color::Reset),
        _ => Err(format!("Unknown color: {}", s)),
    }
}

/// Parse a hex color string into a ratatui Color.
///
/// Supports "#RRGGBB" and "#RGB" formats.
fn parse_hex_color(s: &str) -> Result<Color, String> {
    let hex = s.trim_start_matches('#');

    match hex.len() {
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16)
                .map_err(|_| format!("Invalid hex color: {}", s))?;
            let g = u8::from_str_radix(&hex[2..4], 16)
                .map_err(|_| format!("Invalid hex color: {}", s))?;
            let b = u8::from_str_radix(&hex[4..6], 16)
                .map_err(|_| format!("Invalid hex color: {}", s))?;
            Ok(Color::Rgb(r, g, b))
        }
        3 => {
            // Expand #RGB to #RRGGBB
            let r = u8::from_str_radix(&hex[0..1], 16)
                .map_err(|_| format!("Invalid hex color: {}", s))?;
            let g = u8::from_str_radix(&hex[1..2], 16)
                .map_err(|_| format!("Invalid hex color: {}", s))?;
            let b = u8::from_str_radix(&hex[2..3], 16)
                .map_err(|_| format!("Invalid hex color: {}", s))?;
            Ok(Color::Rgb(r * 17, g * 17, b * 17))
        }
        _ => Err(format!("Invalid hex color format: {}", s)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_named_colors() {
        assert_eq!(parse_color_string("Cyan").unwrap(), Color::Cyan);
        assert_eq!(parse_color_string("cyan").unwrap(), Color::Cyan);
        assert_eq!(parse_color_string("CYAN").unwrap(), Color::Cyan);
        assert_eq!(parse_color_string("DarkGray").unwrap(), Color::DarkGray);
        assert_eq!(parse_color_string("darkgray").unwrap(), Color::DarkGray);
    }

    #[test]
    fn test_parse_hex_colors() {
        assert_eq!(
            parse_color_string("#FF0000").unwrap(),
            Color::Rgb(255, 0, 0)
        );
        assert_eq!(
            parse_color_string("#00ff00").unwrap(),
            Color::Rgb(0, 255, 0)
        );
        assert_eq!(
            parse_color_string("#0000FF").unwrap(),
            Color::Rgb(0, 0, 255)
        );
    }

    #[test]
    fn test_parse_short_hex_colors() {
        assert_eq!(parse_color_string("#F00").unwrap(), Color::Rgb(255, 0, 0));
        assert_eq!(parse_color_string("#0F0").unwrap(), Color::Rgb(0, 255, 0));
        assert_eq!(parse_color_string("#00F").unwrap(), Color::Rgb(0, 0, 255));
        assert_eq!(
            parse_color_string("#FFF").unwrap(),
            Color::Rgb(255, 255, 255)
        );
    }

    #[test]
    fn test_parse_invalid_colors() {
        assert!(parse_color_string("invalid").is_err());
        assert!(parse_color_string("#GGGGGG").is_err());
        assert!(parse_color_string("#12345").is_err());
    }
}
