use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Result};
use serde::Deserialize;
use serde_json::json;

use crate::settings::{self, ProfileName, ProfileRef, DEFAULT_SESSION};
use crate::socket;

/// The Herdr installation this plugin talks to: its binary and config directory.
#[derive(Clone, Debug)]
pub struct Herdr {
    bin: PathBuf,
    config_dir: PathBuf,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct Session {
    pub name: String,
    #[serde(default)]
    pub default: bool,
    #[serde(default)]
    pub running: bool,
    #[serde(default)]
    pub session_dir: Option<PathBuf>,
    #[serde(default)]
    pub socket_path: Option<PathBuf>,
}

#[derive(Deserialize)]
struct SessionList {
    sessions: Vec<Session>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub spaces: usize,
    pub blocked: usize,
}

impl Herdr {
    pub fn from_env() -> Self {
        let bin = std::env::var_os("HERDR_BIN_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("herdr"));
        Self {
            bin,
            config_dir: settings::xdg_config_home().join("herdr"),
        }
    }

    pub fn bin(&self) -> &Path {
        &self.bin
    }

    pub fn config_path(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    pub fn session_dir(&self, profile: &ProfileRef) -> PathBuf {
        match profile {
            ProfileRef::Default => self.config_dir.clone(),
            ProfileRef::Named(name) => self.config_dir.join("sessions").join(name.as_str()),
        }
    }

    /// The session this process runs inside, taken from the environment Herdr sets.
    pub fn current_session() -> ProfileRef {
        std::env::var("HERDR_SESSION")
            .ok()
            .and_then(|name| name.parse::<ProfileName>().ok())
            .map_or(ProfileRef::Default, ProfileRef::Named)
    }

    /// Environment for processes that must not think they run inside Herdr.
    pub fn clean_env() -> impl Iterator<Item = (OsString, OsString)> {
        std::env::vars_os().filter(|(key, _)| !key.to_string_lossy().starts_with("HERDR_"))
    }

    pub fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(&self.bin);
        command.args(args).env_clear().envs(Self::clean_env());
        command
    }

    pub fn sessions(&self) -> Vec<Session> {
        self.command(&["session", "list", "--json"])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| serde_json::from_slice::<SessionList>(&output.stdout).ok())
            .map(|list| list.sessions)
            .unwrap_or_default()
    }

    pub fn stop_session(&self, name: &ProfileName) -> Result<()> {
        self.run(
            &["session", "stop", name.as_str()],
            "herdr session stop failed",
        )
    }

    pub fn delete_session(&self, name: &ProfileName) -> Result<()> {
        self.run(
            &["session", "delete", name.as_str()],
            "herdr session delete failed",
        )
    }

    pub fn reload_config(&self) {
        let _ = self.command(&["server", "reload-config"]).output();
    }

    pub fn default_config(&self) -> String {
        self.command(&["--default-config"])
            .output()
            .ok()
            .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
            .unwrap_or_default()
    }

    fn run(&self, args: &[&str], fallback: &str) -> Result<()> {
        let output = self.command(args).output()?;
        if output.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let message = [stderr.trim(), stdout.trim(), fallback]
            .into_iter()
            .find(|text| !text.is_empty())
            .unwrap_or(fallback);
        bail!("{message}")
    }
}

/// Sessions with a Herdr client attached, meaning a window is showing them.
/// Clients are `herdr`, `herdr --session <name>` or `herdr session attach <name>`
/// processes that own a terminal; servers run without one.
pub fn attached_sessions() -> HashSet<String> {
    Command::new("ps")
        .args(["-eo", "tty=,args="])
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .map(|listing| parse_attached_sessions(&listing))
        .unwrap_or_default()
}

fn parse_attached_sessions(listing: &str) -> HashSet<String> {
    listing.lines().filter_map(attached_session_of).collect()
}

fn attached_session_of(line: &str) -> Option<String> {
    let mut fields = line.split_whitespace();
    let tty = fields.next()?;
    if tty == "?" || tty == "??" || tty == "-" {
        return None;
    }
    let program = fields.next()?;
    if program != "herdr" && !program.ends_with("/herdr") {
        return None;
    }
    let args: Vec<&str> = fields.collect();
    match args.as_slice() {
        [] => Some(DEFAULT_SESSION.to_owned()),
        ["--session", name] | ["session", "attach", name] => Some((*name).to_owned()),
        [flag] => flag.strip_prefix("--session=").map(str::to_owned),
        _ => None,
    }
}

pub fn saved_space_count(session_dir: &Path) -> usize {
    #[derive(Deserialize)]
    struct Snapshot {
        #[serde(default)]
        workspaces: Vec<serde_json::Value>,
    }
    fs::read(session_dir.join("session.json"))
        .ok()
        .and_then(|data| serde_json::from_slice::<Snapshot>(&data).ok())
        .map_or(0, |snapshot| snapshot.workspaces.len())
}

/// Space and blocked-agent counts of a running session, read over its socket.
pub fn live_counts(socket_path: &Path) -> Option<Counts> {
    let workspaces = socket::call(socket_path, "workspace.list", json!({})).ok()?;
    let spaces = workspaces["workspaces"].as_array().map_or(0, Vec::len);
    let blocked = socket::call(socket_path, "agent.list", json!({}))
        .ok()
        .and_then(|agents| agents["agents"].as_array().cloned())
        .map_or(0, |agents| {
            agents
                .iter()
                .filter(|agent| agent["agent_status"] == "blocked")
                .count()
        });
    Some(Counts { spaces, blocked })
}

pub fn env_map() -> HashMap<String, String> {
    std::env::vars().collect()
}

pub fn is_default_session(session: &Session) -> bool {
    session.default || session.name == DEFAULT_SESSION
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_env_drops_herdr_variables() {
        std::env::set_var("HERDR_TEST_PROBE", "1");
        assert!(Herdr::clean_env().all(|(key, _)| key != "HERDR_TEST_PROBE"));
        std::env::remove_var("HERDR_TEST_PROBE");
    }

    #[test]
    fn detects_attached_clients_from_process_listing() {
        let listing = "\
?        /home/alice/.local/bin/herdr server
pts/24   /home/alice/.local/bin/herdr --session personal
pts/3    herdr
pts/5    /usr/bin/herdr session attach study
pts/6    herdr --session=work
pts/7    /usr/bin/herdr plugin list
ttys001  /usr/local/bin/herdr --session mac
";
        let attached = parse_attached_sessions(listing);
        let mut names: Vec<&str> = attached.iter().map(String::as_str).collect();
        names.sort_unstable();
        assert_eq!(names, ["default", "mac", "personal", "study", "work"]);
    }

    #[test]
    fn counts_saved_spaces() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(saved_space_count(dir.path()), 0);
        fs::write(
            dir.path().join("session.json"),
            r#"{"version":3,"workspaces":[{"id":"w1"},{"id":"w2"}]}"#,
        )
        .unwrap();
        assert_eq!(saved_space_count(dir.path()), 2);
    }

    #[test]
    fn session_dirs_follow_herdr_layout() {
        let herdr = Herdr {
            bin: "herdr".into(),
            config_dir: "/cfg/herdr".into(),
        };
        assert_eq!(
            herdr.session_dir(&ProfileRef::Default),
            PathBuf::from("/cfg/herdr")
        );
        let work = ProfileRef::parse("work").unwrap();
        assert_eq!(
            herdr.session_dir(&work),
            PathBuf::from("/cfg/herdr/sessions/work")
        );
        assert_eq!(herdr.config_path(), PathBuf::from("/cfg/herdr/config.toml"));
    }
}
