use std::fs;
use std::path::Path;

use serde::Deserialize;

const DARK_FALLBACK: &str = "catppuccin";
const LIGHT_FALLBACK: &str = "catppuccin-latte";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Herdr's own light/dark rule for a host terminal background.
    pub fn appearance(self) -> Appearance {
        let luminance = u32::from(self.r) * 299 + u32::from(self.g) * 587 + u32::from(self.b) * 114;
        if luminance >= 128_000 {
            Appearance::Light
        } else {
            Appearance::Dark
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Appearance {
    Dark,
    Light,
}

/// The colors Herdr paints its own popups and highlighted rows with.
/// `None` means the terminal default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PopupColors {
    pub background: Option<Rgb>,
    pub text: Option<Rgb>,
    pub active_row: Option<Rgb>,
}

impl PopupColors {
    const fn rgb(background: Rgb, text: Rgb, active_row: Rgb) -> Self {
        Self {
            background: Some(background),
            text: Some(text),
            active_row: Some(active_row),
        }
    }
}

fn canonical_name(name: &str) -> Option<&'static str> {
    let normalized = name.to_lowercase().replace([' ', '_'], "-");
    Some(match normalized.as_str() {
        "catppuccin" | "catppuccin-mocha" => "catppuccin",
        "catppuccin-latte" | "latte" | "light" => "catppuccin-latte",
        "terminal" => "terminal",
        "tokyo-night" | "tokyonight" => "tokyo-night",
        "tokyo-night-day" | "tokyo-day" | "tokyonight-day" => "tokyo-night-day",
        "dracula" => "dracula",
        "nord" => "nord",
        "gruvbox" | "gruvbox-dark" => "gruvbox",
        "gruvbox-light" => "gruvbox-light",
        "one-dark" | "onedark" => "one-dark",
        "one-light" | "onelight" => "one-light",
        "solarized" | "solarized-dark" => "solarized",
        "solarized-light" => "solarized-light",
        "kanagawa" => "kanagawa",
        "kanagawa-lotus" | "lotus" => "kanagawa-lotus",
        "rose-pine" | "rosepine" => "rose-pine",
        "rose-pine-dawn" | "rosepine-dawn" | "dawn" => "rose-pine-dawn",
        "vesper" => "vesper",
        _ => return None,
    })
}

/// Panel background, text and active row of Herdr's built-in themes
/// (src/app/state.rs in Herdr).
fn builtin(name: &str) -> Option<PopupColors> {
    Some(match canonical_name(name)? {
        "catppuccin" => PopupColors::rgb(
            Rgb::new(24, 24, 37),
            Rgb::new(205, 214, 244),
            Rgb::new(30, 30, 46),
        ),
        "catppuccin-latte" => PopupColors::rgb(
            Rgb::new(239, 241, 245),
            Rgb::new(76, 79, 105),
            Rgb::new(230, 233, 239),
        ),
        "terminal" => PopupColors::default(),
        "tokyo-night" => PopupColors::rgb(
            Rgb::new(26, 27, 38),
            Rgb::new(192, 202, 245),
            Rgb::new(35, 38, 54),
        ),
        "tokyo-night-day" => PopupColors::rgb(
            Rgb::new(225, 226, 231),
            Rgb::new(55, 96, 191),
            Rgb::new(210, 211, 218),
        ),
        "dracula" => PopupColors::rgb(
            Rgb::new(40, 42, 54),
            Rgb::new(248, 248, 242),
            Rgb::new(55, 60, 82),
        ),
        "nord" => PopupColors::rgb(
            Rgb::new(46, 52, 64),
            Rgb::new(236, 239, 244),
            Rgb::new(67, 76, 94),
        ),
        "gruvbox" => PopupColors::rgb(
            Rgb::new(40, 40, 40),
            Rgb::new(235, 219, 178),
            Rgb::new(50, 49, 48),
        ),
        "gruvbox-light" => PopupColors::rgb(
            Rgb::new(251, 241, 199),
            Rgb::new(60, 56, 54),
            Rgb::new(242, 229, 188),
        ),
        "one-dark" => PopupColors::rgb(
            Rgb::new(40, 44, 52),
            Rgb::new(171, 178, 191),
            Rgb::new(49, 54, 64),
        ),
        "one-light" => PopupColors::rgb(
            Rgb::new(250, 250, 250),
            Rgb::new(56, 58, 66),
            Rgb::new(216, 219, 226),
        ),
        "solarized" => PopupColors::rgb(
            Rgb::new(0, 43, 54),
            Rgb::new(147, 161, 161),
            Rgb::new(22, 75, 87),
        ),
        "solarized-light" => PopupColors::rgb(
            Rgb::new(253, 246, 227),
            Rgb::new(101, 123, 131),
            Rgb::new(238, 232, 213),
        ),
        "kanagawa" => PopupColors::rgb(
            Rgb::new(31, 31, 40),
            Rgb::new(220, 215, 186),
            Rgb::new(54, 54, 70),
        ),
        "kanagawa-lotus" => PopupColors::rgb(
            Rgb::new(242, 236, 188),
            Rgb::new(84, 84, 100),
            Rgb::new(213, 206, 163),
        ),
        "rose-pine" => PopupColors::rgb(
            Rgb::new(25, 23, 36),
            Rgb::new(224, 222, 244),
            Rgb::new(38, 35, 58),
        ),
        "rose-pine-dawn" => PopupColors::rgb(
            Rgb::new(250, 244, 237),
            Rgb::new(70, 66, 97),
            Rgb::new(227, 217, 207),
        ),
        "vesper" => PopupColors::rgb(
            Rgb::new(26, 26, 26),
            Rgb::new(255, 255, 255),
            Rgb::new(16, 16, 16),
        ),
        _ => return None,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ColorOverride {
    Terminal,
    Rgb(Rgb),
}

/// Parses the color forms Herdr accepts in `[theme.custom]`; named palette
/// colors are left to the base theme.
fn parse_override(value: &str) -> Option<ColorOverride> {
    let value = value.trim().to_lowercase();
    if matches!(value.as_str(), "reset" | "default" | "none" | "transparent") {
        return Some(ColorOverride::Terminal);
    }
    if let Some(hex) = value.strip_prefix('#') {
        let digits: Vec<u8> = hex
            .chars()
            .map(|c| c.to_digit(16).map(|d| d as u8))
            .collect::<Option<_>>()?;
        return match digits.as_slice() {
            [r1, r2, g1, g2, b1, b2] => Some(ColorOverride::Rgb(Rgb::new(
                r1 * 16 + r2,
                g1 * 16 + g2,
                b1 * 16 + b2,
            ))),
            [r, g, b] => Some(ColorOverride::Rgb(Rgb::new(r * 17, g * 17, b * 17))),
            _ => None,
        };
    }
    let inner = value.strip_prefix("rgb(")?.strip_suffix(')')?;
    let parts: Vec<u8> = inner
        .split(',')
        .map(|p| p.trim().parse().ok())
        .collect::<Option<_>>()?;
    match parts.as_slice() {
        [r, g, b] => Some(ColorOverride::Rgb(Rgb::new(*r, *g, *b))),
        _ => None,
    }
}

fn apply(slot: &mut Option<Rgb>, value: Option<&str>) {
    match value.and_then(parse_override) {
        Some(ColorOverride::Terminal) => *slot = None,
        Some(ColorOverride::Rgb(rgb)) => *slot = Some(rgb),
        None => {}
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ModeColors {
    panel_bg: Option<String>,
    text: Option<String>,
    active_row_bg: Option<String>,
}

impl ModeColors {
    fn apply_to(&self, colors: &mut PopupColors) {
        apply(&mut colors.background, self.panel_bg.as_deref());
        apply(&mut colors.text, self.text.as_deref());
        apply(&mut colors.active_row, self.active_row_bg.as_deref());
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct CustomColors {
    #[serde(flatten)]
    base: ModeColors,
    dark: Option<ModeColors>,
    light: Option<ModeColors>,
}

/// The `[theme]` section of Herdr's config.toml.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ThemeConfig {
    name: Option<String>,
    auto_switch: bool,
    dark_name: Option<String>,
    light_name: Option<String>,
    custom: Option<CustomColors>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Config {
    theme: ThemeConfig,
}

impl ThemeConfig {
    pub fn load(config_path: &Path) -> Self {
        fs::read_to_string(config_path)
            .ok()
            .and_then(|text| toml::from_str::<Config>(&text).ok())
            .map(|config| config.theme)
            .unwrap_or_default()
    }

    pub fn follows_host_appearance(&self) -> bool {
        self.auto_switch
    }

    /// Resolves the popup colors the way Herdr resolves its palette.
    pub fn resolve(&self, appearance: Option<Appearance>) -> PopupColors {
        let custom = self.custom.as_ref();
        let (name, fallback, mode) = if self.auto_switch {
            match appearance.unwrap_or(Appearance::Dark) {
                Appearance::Dark => (
                    &self.dark_name,
                    DARK_FALLBACK,
                    custom.and_then(|c| c.dark.as_ref()),
                ),
                Appearance::Light => (
                    &self.light_name,
                    LIGHT_FALLBACK,
                    custom.and_then(|c| c.light.as_ref()),
                ),
            }
        } else {
            (&self.name, DARK_FALLBACK, None)
        };
        let mut colors = name
            .as_deref()
            .and_then(builtin)
            .or_else(|| builtin(fallback))
            .unwrap_or_default();
        if let Some(custom) = custom {
            custom.base.apply_to(&mut colors);
        }
        if let Some(mode) = mode {
            mode.apply_to(&mut colors);
        }
        colors
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme(toml_text: &str) -> ThemeConfig {
        toml::from_str::<Config>(toml_text).unwrap().theme
    }

    #[test]
    fn builtin_names_and_aliases() {
        let gruvbox = builtin("gruvbox").unwrap();
        assert_eq!(gruvbox.background, Some(Rgb::new(40, 40, 40)));
        assert_eq!(gruvbox.active_row, Some(Rgb::new(50, 49, 48)));
        assert_eq!(builtin("Gruvbox Dark"), builtin("gruvbox"));
        assert_eq!(builtin("TokyoNight"), builtin("tokyo-night"));
        assert_eq!(builtin("terminal"), Some(PopupColors::default()));
        assert!(builtin("nope").is_none());
    }

    #[test]
    fn parses_override_colors() {
        assert_eq!(
            parse_override("#282828"),
            Some(ColorOverride::Rgb(Rgb::new(40, 40, 40)))
        );
        assert_eq!(
            parse_override("#fff"),
            Some(ColorOverride::Rgb(Rgb::new(255, 255, 255)))
        );
        assert_eq!(
            parse_override("rgb(1, 2, 3)"),
            Some(ColorOverride::Rgb(Rgb::new(1, 2, 3)))
        );
        assert_eq!(parse_override("reset"), Some(ColorOverride::Terminal));
        assert_eq!(parse_override("red"), None);
        assert_eq!(parse_override("#12345"), None);
    }

    #[test]
    fn resolves_manual_theme_with_overrides() {
        let config = theme(
            "[theme]\nname = \"gruvbox\"\n[theme.custom]\npanel_bg = \"#101010\"\ntext = \"red\"\nactive_row_bg = \"reset\"\n",
        );
        let colors = config.resolve(None);
        assert_eq!(colors.background, Some(Rgb::new(16, 16, 16)));
        assert_eq!(colors.text, Some(Rgb::new(235, 219, 178)));
        assert_eq!(colors.active_row, None);
        assert_eq!(theme("").resolve(None), builtin("catppuccin").unwrap());
        assert_eq!(
            theme("[theme]\nname = \"unknown\"\n").resolve(None),
            builtin("catppuccin").unwrap()
        );
    }

    #[test]
    fn resolves_auto_switch_by_appearance() {
        let config = theme(
            "[theme]\nauto_switch = true\ndark_name = \"nord\"\nlight_name = \"one-light\"\n[theme.custom.light]\npanel_bg = \"#eeeeee\"\n",
        );
        assert!(config.follows_host_appearance());
        assert_eq!(
            config.resolve(Some(Appearance::Dark)),
            builtin("nord").unwrap()
        );
        assert_eq!(config.resolve(None), builtin("nord").unwrap());
        let light = config.resolve(Some(Appearance::Light));
        assert_eq!(light.background, Some(Rgb::new(238, 238, 238)));
        assert_eq!(light.text, builtin("one-light").unwrap().text);
        assert_eq!(
            theme("[theme]\nauto_switch = true\n").resolve(Some(Appearance::Light)),
            builtin("catppuccin-latte").unwrap()
        );
    }

    #[test]
    fn appearance_from_background_luminance() {
        assert_eq!(Rgb::new(40, 40, 40).appearance(), Appearance::Dark);
        assert_eq!(Rgb::new(250, 250, 250).appearance(), Appearance::Light);
    }

    #[test]
    fn loads_from_config_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "onboarding = false\n[theme]\nname = \"dracula\"\n").unwrap();
        assert_eq!(
            ThemeConfig::load(&path).resolve(None),
            builtin("dracula").unwrap()
        );
        assert_eq!(
            ThemeConfig::load(&dir.path().join("missing.toml")),
            ThemeConfig::default()
        );
    }
}
