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

/// Herdr's default session, a local named one, or a session on an SSH host.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ProfileRef {
    Default,
    Named(ProfileName),
    Remote {
        name: ProfileName,
        target: String,
        session: Option<String>,
    },
}

impl ProfileRef {
    /// Parses a local profile name. Remote profiles need their settings, see
    /// `ProfileStore::resolve`.
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
            Self::Named(name) | Self::Remote { name, .. } => name.as_str(),
        }
    }

    pub fn is_default(&self) -> bool {
        matches!(self, Self::Default)
    }

    /// What an attached `herdr` client for this profile looks like, see
    /// `herdr::attached_sessions`.
    pub fn attach_key(&self) -> String {
        match self {
            Self::Remote {
                target, session, ..
            } => remote_attach_key(target, session.as_deref()),
            _ => self.name().to_owned(),
        }
    }

    pub fn session_args(&self) -> Vec<String> {
        match self {
            Self::Default => Vec::new(),
            Self::Named(name) => vec!["--session".to_owned(), name.to_string()],
            Self::Remote {
                target, session, ..
            } => {
                let mut args = vec!["--remote".to_owned(), target.clone()];
                if let Some(session) = session {
                    args.extend(["--session".to_owned(), session.clone()]);
                }
                args
            }
        }
    }
}

pub fn remote_attach_key(target: &str, session: Option<&str>) -> String {
    format!("{target}#{}", session.unwrap_or(DEFAULT_SESSION))
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
    /// SSH target for `herdr --remote`; absent for local profiles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote: Option<String>,
    /// Named session on the remote host; absent means its default session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_session: Option<String>,
}

impl Profile {
    pub fn label(&self) -> &str {
        self.label.as_deref().unwrap_or(self.name.as_str())
    }

    pub fn to_ref(&self) -> ProfileRef {
        match &self.remote {
            Some(target) => ProfileRef::Remote {
                name: self.name.clone(),
                target: target.clone(),
                session: self.remote_session.clone(),
            },
            None => ProfileRef::Named(self.name.clone()),
        }
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
    fn remote_profile_args_and_attach_key() {
        let profile = Profile {
            name: "box".parse().unwrap(),
            label: None,
            remote: Some("workbox".into()),
            remote_session: Some("agents".into()),
        };
        let remote = profile.to_ref();
        assert_eq!(remote.name(), "box");
        assert_eq!(
            remote.session_args(),
            ["--remote", "workbox", "--session", "agents"]
        );
        assert_eq!(remote.attach_key(), "workbox#agents");
        let plain = Profile {
            remote_session: None,
            ..profile
        }
        .to_ref();
        assert_eq!(plain.session_args(), ["--remote", "workbox"]);
        assert_eq!(plain.attach_key(), "workbox#default");
    }

    #[test]
    fn settings_roundtrip_and_corrupt_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.json");
        let mut settings = Settings::default();
        settings.profiles.push(Profile {
            name: "work".parse().unwrap(),
            label: Some("Work".into()),
            remote: None,
            remote_session: None,
        });
        settings.save(&path).unwrap();
        let loaded = Settings::load(&path);
        assert_eq!(loaded, settings);
        assert!(loaded.chooser_on_launch);
        fs::write(&path, "{not json").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
    }

    #[test]
    fn settings_ignore_invalid_profile_names() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.json");
        fs::write(&path, r#"{"profiles":[{"name":"bad name"}]}"#).unwrap();
        assert!(Settings::load(&path).profiles.is_empty());
    }
}
