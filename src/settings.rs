use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

pub const DEFAULT_SESSION: &str = "default";
const SETTINGS_FILE: &str = "profiles.json";
const MAX_NAME_LEN: usize = 64;

/// A validated name for a named Herdr session. Follows Herdr's own rules.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ProfileName(String);

impl ProfileName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for ProfileName {
    type Err = anyhow::Error;

    fn from_str(name: &str) -> Result<Self> {
        if name.is_empty() {
            bail!("profile name cannot be empty");
        }
        if name == "." || name == ".." {
            bail!("profile name cannot be . or ..");
        }
        if name == DEFAULT_SESSION {
            bail!("'default' is the launcher session, pick another name");
        }
        let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-');
        if name.len() > MAX_NAME_LEN || !name.chars().all(allowed) {
            bail!("use only ASCII letters, numbers, '.', '_' and '-' (max {MAX_NAME_LEN})");
        }
        Ok(Self(name.to_owned()))
    }
}

impl fmt::Display for ProfileName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Either Herdr's default session or a named one.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ProfileRef {
    Default,
    Named(ProfileName),
}

impl ProfileRef {
    pub fn parse(name: &str) -> Result<Self> {
        if name == DEFAULT_SESSION {
            Ok(Self::Default)
        } else {
            name.parse().map(Self::Named)
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Default => DEFAULT_SESSION,
            Self::Named(name) => name.as_str(),
        }
    }

    pub fn is_default(&self) -> bool {
        matches!(self, Self::Default)
    }

    pub fn session_args(&self) -> Vec<String> {
        match self {
            Self::Default => Vec::new(),
            Self::Named(name) => vec!["--session".to_owned(), name.to_string()],
        }
    }
}

impl fmt::Display for ProfileRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub name: ProfileName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Extra environment for the profile's window, e.g. a separate Claude account.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

impl Profile {
    pub fn label(&self) -> &str {
        self.label.as_deref().unwrap_or(self.name.as_str())
    }

    /// `env` with a leading `~` expanded, since no shell sees these values.
    pub fn window_env(&self) -> Vec<(String, String)> {
        self.env
            .iter()
            .map(|(key, value)| (key.clone(), expand_home(value)))
            .collect()
    }
}

fn expand_home(value: &str) -> String {
    match value.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => {
            format!("{}{rest}", home_dir().display())
        }
        _ => value.to_owned(),
    }
}

impl<'de> Deserialize<'de> for ProfileName {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        raw.parse().map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    pub chooser_on_launch: bool,
    pub terminal: Option<Vec<String>>,
    pub last: Option<String>,
    pub profiles: Vec<Profile>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            chooser_on_launch: true,
            terminal: None,
            last: None,
            profiles: Vec::new(),
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Self {
        fs::read(path)
            .ok()
            .and_then(|data| serde_json::from_slice(&data).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let mut data = serde_json::to_vec_pretty(self)?;
        data.push(b'\n');
        write_atomically(path, &data)
    }

    pub fn profile(&self, name: &ProfileName) -> Option<&Profile> {
        self.profiles.iter().find(|p| &p.name == name)
    }

    /// The extra environment a profile's window runs with; empty for `default`.
    pub fn profile_env(&self, target: &ProfileRef) -> Vec<(String, String)> {
        match target {
            ProfileRef::Default => Vec::new(),
            ProfileRef::Named(name) => self
                .profile(name)
                .map(Profile::window_env)
                .unwrap_or_default(),
        }
    }
}

pub fn write_atomically(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, data).with_context(|| format!("writing {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))
}

pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

pub fn xdg_config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".config"))
}

/// Where profiles.json lives: the directory Herdr hands us, or a fallback
/// when the binary runs outside Herdr.
pub fn plugin_config_dir() -> PathBuf {
    std::env::var_os("HERDR_PLUGIN_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| xdg_config_home().join("herdr-profiles"))
}

pub fn settings_path() -> PathBuf {
    plugin_config_dir().join(SETTINGS_FILE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_names() {
        for name in ["work", "personal-dev", "study.2026", "a_b", &"x".repeat(64)] {
            assert!(name.parse::<ProfileName>().is_ok(), "{name}");
        }
    }

    #[test]
    fn rejects_invalid_names() {
        for name in [
            "",
            ".",
            "..",
            "default",
            "has space",
            "ünïcode",
            &"x".repeat(65),
            "a/b",
        ] {
            assert!(name.parse::<ProfileName>().is_err(), "{name}");
        }
    }

    #[test]
    fn profile_ref_parses_default_and_named() {
        assert_eq!(ProfileRef::parse("default").unwrap(), ProfileRef::Default);
        assert_eq!(ProfileRef::parse("work").unwrap().name(), "work");
        assert!(ProfileRef::Default.session_args().is_empty());
        assert_eq!(
            ProfileRef::parse("work").unwrap().session_args(),
            ["--session", "work"]
        );
    }

    #[test]
    fn settings_roundtrip_and_corrupt_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.json");
        let mut settings = Settings::default();
        settings.profiles.push(Profile {
            name: "work".parse().unwrap(),
            label: Some("Work".into()),
            env: BTreeMap::from([("CLAUDE_CODE_USE_VERTEX".into(), "1".into())]),
        });
        settings.save(&path).unwrap();
        let loaded = Settings::load(&path);
        assert_eq!(loaded, settings);
        assert!(loaded.chooser_on_launch);
        fs::write(&path, "{not json").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
    }

    #[test]
    fn window_env_expands_home() {
        let profile: Profile = serde_json::from_str(
            r#"{"name":"vertex","env":{"A":"~/.claude-vertex","B":"~","C":"x~/y","D":"~bob"}}"#,
        )
        .unwrap();
        let home = home_dir().display().to_string();
        let values: Vec<String> = profile.window_env().into_iter().map(|(_, v)| v).collect();
        assert_eq!(
            values,
            [
                format!("{home}/.claude-vertex"),
                home,
                "x~/y".into(),
                "~bob".into()
            ]
        );
    }

    #[test]
    fn settings_ignore_invalid_profile_names() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.json");
        fs::write(&path, r#"{"profiles":[{"name":"bad name"}]}"#).unwrap();
        assert!(Settings::load(&path).profiles.is_empty());
    }
}
