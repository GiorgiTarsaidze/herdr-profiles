use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::Path;
use std::str::FromStr;
use std::sync::OnceLock;

use anyhow::{bail, Result};
use regex_lite::Regex;
use serde::Serialize;

use crate::settings::write_atomically;

pub const ACTION: &str = "herdr-profiles.choose";
const BLOCK_START: &str =
    "# >>> herdr-profiles managed keybinding (edit with: herdr-profiles setup) >>>";
const BLOCK_END: &str = "# <<< herdr-profiles managed keybinding <<<";
const CANDIDATES: &[&str] = &[
    "prefix+a",
    "prefix+u",
    "prefix+f",
    "prefix+i",
    "prefix+y",
    "prefix+m",
    "prefix+d",
    "prefix+t",
    "prefix+shift+a",
    "prefix+shift+u",
];

fn quoted_key_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#""((?:ctrl|alt|shift|cmd|prefix)\+[^"\s]+)""#).unwrap())
}

fn key_line_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(?m)^\s*key\s*=\s*"([^"]+)""#).unwrap())
}

fn valid_key_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(concat!(
            r"^(?:(?:ctrl|alt|shift|cmd|prefix)\+)+",
            r"(?:[a-z0-9]|f[0-9]{1,2}|enter|tab|esc|left|right|up|down|space|minus|comma|plus|",
            r"backtick|ampersand|period|slash|semicolon|equal|backslash|bracketleft|bracketright)$",
        ))
        .unwrap()
    })
}

/// A Herdr key string such as `prefix+a`, validated against Herdr's syntax.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct KeyCombo(String);

impl KeyCombo {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for KeyCombo {
    type Err = anyhow::Error;

    fn from_str(key: &str) -> Result<Self> {
        if !valid_key_re().is_match(key) {
            bail!("key must look like prefix+a, ctrl+alt+p or prefix+shift+f2 (a modifier plus one key)");
        }
        Ok(Self(key.to_owned()))
    }
}

impl fmt::Display for KeyCombo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Managed,
    Manual,
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Managed => "managed",
            Self::Manual => "manual",
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Binding {
    pub key: KeyCombo,
    pub source: Source,
}

/// The current chooser binding plus every other key already taken.
#[derive(Clone, Debug, Serialize)]
pub struct Status {
    pub binding: Option<Binding>,
    pub config: String,
    pub used: BTreeMap<String, String>,
}

impl Status {
    pub fn owner_of(&self, key: &KeyCombo) -> Option<&str> {
        self.used.get(key.as_str()).map(String::as_str)
    }

    pub fn free_key(&self) -> Option<KeyCombo> {
        CANDIDATES
            .iter()
            .find(|key| !self.used.contains_key(**key))
            .map(|key| KeyCombo((*key).to_owned()))
    }
}

fn managed_block(key: &KeyCombo) -> String {
    format!(
        "{BLOCK_START}\n[[keys.command]]\nkey = \"{key}\"\ntype = \"plugin_action\"\ncommand = \"{ACTION}\"\ndescription = \"choose profile\"\n{BLOCK_END}\n"
    )
}

/// Splits the config into everything outside our block and the key inside it.
fn split_block(config: &str) -> (String, Option<String>) {
    let Some(start) = config.find(BLOCK_START) else {
        return (config.to_owned(), None);
    };
    let Some(end) = config[start..]
        .find(BLOCK_END)
        .map(|end| start + end + BLOCK_END.len())
    else {
        return (config.to_owned(), None);
    };
    let key = key_line_re()
        .captures(&config[start..end])
        .map(|c| c[1].to_owned());
    let rest = format!(
        "{}{}",
        &config[..start],
        config[end..].trim_start_matches('\n')
    );
    (rest, key)
}

/// Finds a `[[keys.command]]` the user wrote by hand for our action.
fn manual_binding(config: &str) -> Option<String> {
    let action = format!("\"{ACTION}\"");
    let at = config.find(&action)?;
    let table_start = config[..at].rfind("[[keys.command]]").unwrap_or(0);
    let table_end = config[at..]
        .find("[[")
        .map_or(config.len(), |next| at + next);
    key_line_re()
        .captures(&config[table_start..table_end])
        .map(|c| c[1].to_owned())
}

fn used_keys(config: &str, defaults: &str) -> BTreeMap<String, String> {
    let mut used = BTreeMap::new();
    for capture in quoted_key_re().captures_iter(defaults) {
        used.insert(capture[1].to_owned(), "a Herdr default binding".to_owned());
    }
    for capture in quoted_key_re().captures_iter(config) {
        used.insert(capture[1].to_owned(), "your config.toml".to_owned());
    }
    used
}

fn read_config(path: &Path) -> Result<String> {
    match fs::read_to_string(path) {
        Ok(config) => Ok(config),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(err) => Err(err.into()),
    }
}

pub fn status(config_path: &Path, defaults: &str) -> Result<Status> {
    let config = read_config(config_path)?;
    let (rest, managed_key) = split_block(&config);
    let binding = manual_binding(&rest)
        .map(|key| (key, Source::Manual))
        .or_else(|| managed_key.map(|key| (key, Source::Managed)))
        .map(|(key, source)| Binding {
            key: KeyCombo(key),
            source,
        });
    let mut used = used_keys(&rest, defaults);
    if let Some(binding) = &binding {
        used.remove(binding.key.as_str());
    }
    Ok(Status {
        binding,
        config: config_path.display().to_string(),
        used,
    })
}

/// Rewrites (or appends) our fenced block so it binds `key`.
pub fn apply(config_path: &Path, key: &KeyCombo) -> Result<()> {
    let (rest, _) = split_block(&read_config(config_path)?);
    let rest = rest.trim_end_matches('\n');
    let separator = if rest.is_empty() { "" } else { "\n\n" };
    write_atomically(
        config_path,
        format!("{rest}{separator}{}", managed_block(key)).as_bytes(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULTS: &str = "# previous_tab = \"prefix+p\"\n# help = \"prefix+?\"\n";

    fn key(s: &str) -> KeyCombo {
        s.parse().unwrap()
    }

    #[test]
    fn block_is_appended_then_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "onboarding = false\n[theme]\nname = \"gruvbox\"\n").unwrap();
        apply(&path, &key("prefix+a")).unwrap();
        let config = fs::read_to_string(&path).unwrap();
        assert!(config.starts_with(&format!(
            "onboarding = false\n[theme]\nname = \"gruvbox\"\n\n{BLOCK_START}"
        )));
        assert!(
            config.contains("key = \"prefix+a\"")
                && config.contains("command = \"herdr-profiles.choose\"")
        );

        apply(&path, &key("prefix+u")).unwrap();
        let config = fs::read_to_string(&path).unwrap();
        assert_eq!(config.matches(BLOCK_START).count(), 1);
        assert!(!config.contains("\"prefix+a\"") && config.contains("\"prefix+u\""));
        let (rest, managed) = split_block(&config);
        assert_eq!(managed.as_deref(), Some("prefix+u"));
        assert!(!rest.contains("keys.command") && rest.contains("gruvbox"));
    }

    #[test]
    fn missing_config_is_created() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        apply(&path, &key("prefix+a")).unwrap();
        assert!(fs::read_to_string(&path).unwrap().starts_with(BLOCK_START));
    }

    #[test]
    fn status_reports_manual_managed_and_used_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "[[keys.command]]\nkey = \"prefix+p\"\ntype = \"plugin_action\"\ncommand = \"herdr-profiles.choose\"\n").unwrap();
        let found = status(&path, DEFAULTS).unwrap();
        let binding = found.binding.unwrap();
        assert_eq!(
            (binding.key.as_str(), binding.source),
            ("prefix+p", Source::Manual)
        );
        assert!(!found.used.contains_key("prefix+p"));
        assert_eq!(found.used["prefix+?"], "a Herdr default binding");

        fs::write(
            &path,
            "[[keys.command]]\nkey = \"prefix+a\"\ncommand = \"other.plugin.thing\"\n",
        )
        .unwrap();
        apply(&path, &key("prefix+u")).unwrap();
        let found = status(&path, DEFAULTS).unwrap();
        assert_eq!(found.binding.as_ref().unwrap().source, Source::Managed);
        assert_eq!(found.used["prefix+a"], "your config.toml");
        assert_eq!(found.free_key().unwrap().as_str(), "prefix+u");
    }

    #[test]
    fn free_key_skips_taken_candidates() {
        let mut status = Status {
            binding: None,
            config: String::new(),
            used: BTreeMap::new(),
        };
        assert_eq!(status.free_key().unwrap().as_str(), "prefix+a");
        for candidate in CANDIDATES {
            status
                .used
                .insert((*candidate).to_owned(), "taken".to_owned());
        }
        assert!(status.free_key().is_none());
    }

    #[test]
    fn validates_key_syntax() {
        for ok in [
            "prefix+a",
            "prefix+shift+a",
            "ctrl+alt+p",
            "prefix+f2",
            "prefix+minus",
            "cmd+k",
        ] {
            assert!(ok.parse::<KeyCombo>().is_ok(), "{ok}");
        }
        for bad in [
            "",
            "a",
            "prefix",
            "prefix+",
            "prefix+A",
            "prefix+ab",
            "prefix a",
            "super+a",
        ] {
            assert!(bad.parse::<KeyCombo>().is_err(), "{bad}");
        }
    }
}
