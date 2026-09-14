use std::collections::HashMap;
use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result};

use crate::herdr::Herdr;
use crate::settings::ProfileRef;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Linux,
    MacOs,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Linux
        }
    }
}

struct TerminalEmulator {
    binary: &'static str,
    env_hint: Option<&'static str>,
    argv: fn(&str) -> Vec<String>,
}

fn args(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_owned()).collect()
}

const TERMINALS: &[TerminalEmulator] = &[
    TerminalEmulator {
        binary: "kitty",
        env_hint: Some("KITTY_WINDOW_ID"),
        argv: |t| args(&["kitty", "--title", t]),
    },
    TerminalEmulator {
        binary: "ghostty",
        env_hint: Some("GHOSTTY_RESOURCES_DIR"),
        argv: |t| vec!["ghostty".into(), format!("--title={t}"), "-e".into()],
    },
    TerminalEmulator {
        binary: "wezterm",
        env_hint: Some("WEZTERM_PANE"),
        argv: |_| args(&["wezterm", "start", "--"]),
    },
    TerminalEmulator {
        binary: "alacritty",
        env_hint: Some("ALACRITTY_WINDOW"),
        argv: |t| args(&["alacritty", "--title", t, "-e"]),
    },
    TerminalEmulator {
        binary: "foot",
        env_hint: Some("FOOT_PID"),
        argv: |t| args(&["foot", "--title", t]),
    },
    TerminalEmulator {
        binary: "gnome-terminal",
        env_hint: Some("GNOME_TERMINAL_SCREEN"),
        argv: |t| args(&["gnome-terminal", "--window", "--title", t, "--"]),
    },
    TerminalEmulator {
        binary: "konsole",
        env_hint: Some("KONSOLE_VERSION"),
        argv: |t| {
            vec![
                "konsole".into(),
                "-p".into(),
                format!("tabtitle={t}"),
                "-e".into(),
            ]
        },
    },
    TerminalEmulator {
        binary: "xfce4-terminal",
        env_hint: None,
        argv: |t| args(&["xfce4-terminal", "--title", t, "-x"]),
    },
    TerminalEmulator {
        binary: "x-terminal-emulator",
        env_hint: None,
        argv: |_| args(&["x-terminal-emulator", "-e"]),
    },
    TerminalEmulator {
        binary: "xterm",
        env_hint: None,
        argv: |t| args(&["xterm", "-T", t, "-e"]),
    },
];

/// Builds the command that opens a profile in a new terminal window.
pub struct Launcher<'a> {
    herdr_bin: &'a Path,
    terminal_override: Option<&'a [String]>,
    env: &'a HashMap<String, String>,
    platform: Platform,
    installed: &'a dyn Fn(&str) -> bool,
    exec_prefix: Vec<String>,
}

impl<'a> Launcher<'a> {
    pub fn new(
        herdr_bin: &'a Path,
        terminal_override: Option<&'a [String]>,
        env: &'a HashMap<String, String>,
        platform: Platform,
        installed: &'a dyn Fn(&str) -> bool,
    ) -> Self {
        Self {
            herdr_bin,
            terminal_override,
            env,
            platform,
            installed,
            exec_prefix: Vec::new(),
        }
    }

    /// Runs Herdr through `<exe> exec <profile> --`, which sets the profile's
    /// environment inside the new window.
    pub fn through_exec(mut self, exe: &Path, target: &ProfileRef) -> Self {
        self.exec_prefix = vec![
            exe.to_string_lossy().into_owned(),
            "exec".to_owned(),
            target.name().to_owned(),
            "--".to_owned(),
        ];
        self
    }

    pub fn argv(&self, target: &ProfileRef) -> Option<Vec<String>> {
        let title = format!("herdr · {target}");
        let mut herdr = self.exec_prefix.clone();
        herdr.push(self.herdr_bin.to_string_lossy().into_owned());
        herdr.extend(target.session_args());

        if let Some(prefix) = self.terminal_override.filter(|prefix| !prefix.is_empty()) {
            let mut argv: Vec<String> = prefix
                .iter()
                .map(|part| part.replace("{title}", &title))
                .collect();
            argv.extend(herdr);
            return Some(argv);
        }
        match self.platform {
            Platform::MacOs => Some(macos_argv(&herdr, self.macos_app())),
            Platform::Linux => {
                let mut argv = self.detect_terminal(&title)?;
                argv.extend(herdr);
                Some(argv)
            }
        }
    }

    fn macos_app(&self) -> &'static str {
        if self.env.get("TERM_PROGRAM").map(String::as_str) == Some("iTerm.app") {
            "iTerm"
        } else {
            "Terminal"
        }
    }

    fn detect_terminal(&self, title: &str) -> Option<Vec<String>> {
        let hinted = TERMINALS.iter().find(|terminal| {
            terminal
                .env_hint
                .is_some_and(|hint| self.env.get(hint).is_some_and(|v| !v.is_empty()))
                && (self.installed)(terminal.binary)
        });
        let terminal = hinted.or_else(|| {
            TERMINALS
                .iter()
                .find(|terminal| (self.installed)(terminal.binary))
        })?;
        Some((terminal.argv)(title))
    }
}

fn macos_argv(command: &[String], app: &str) -> Vec<String> {
    let script = command.join(" ").replace('"', "\\\"");
    vec![
        "osascript".into(),
        "-e".into(),
        format!("tell application \"{app}\" to do script \"{script}\""),
        "-e".into(),
        format!("tell application \"{app}\" to activate"),
    ]
}

/// Variables that identify the terminal Herdr was started from. A new window
/// must not inherit them: gnome-terminal, for one, tries to attach to the
/// screen named in `GNOME_TERMINAL_SCREEN` and fails once that tab is gone.
const PARENT_TERMINAL_VARS: &[&str] = &[
    "GNOME_TERMINAL_SCREEN",
    "GNOME_TERMINAL_SERVICE",
    "KITTY_WINDOW_ID",
    "KITTY_PID",
    "KITTY_LISTEN_ON",
    "KITTY_PUBLIC_KEY",
    "WEZTERM_PANE",
    "WEZTERM_UNIX_SOCKET",
    "WEZTERM_EXECUTABLE",
    "ALACRITTY_WINDOW",
    "ALACRITTY_SOCKET",
    "ALACRITTY_LOG",
    "FOOT_PID",
    "KONSOLE_VERSION",
    "KONSOLE_DBUS_SERVICE",
    "KONSOLE_DBUS_SESSION",
    "KONSOLE_DBUS_WINDOW",
    "VTE_VERSION",
    "TERM_SESSION_ID",
    "ITERM_SESSION_ID",
    "WINDOWID",
];

pub fn window_env() -> impl Iterator<Item = (OsString, OsString)> {
    Herdr::clean_env().filter(|(key, _)| !PARENT_TERMINAL_VARS.iter().any(|var| key == var))
}

pub fn is_on_path(binary: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(binary).is_file()))
}

/// Replaces this process with `command`, adding `env` to the environment.
/// Only returns on failure.
pub fn exec(command: &[String], env: Vec<(String, String)>) -> Result<()> {
    use std::os::unix::process::CommandExt;

    let (program, rest) = command.split_first().context("empty command")?;
    let err = Command::new(program).args(rest).envs(env).exec();
    Err(err).with_context(|| format!("starting {program}"))
}

/// Starts the window detached from the popup, so closing the popup never kills it.
pub fn spawn_detached(argv: &[String]) -> Result<()> {
    use std::os::unix::process::CommandExt;

    let (program, rest) = argv.split_first().context("empty launch command")?;
    Command::new(program)
        .args(rest)
        .env_clear()
        .envs(window_env())
        .current_dir(crate::settings::home_dir())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .with_context(|| format!("starting {program}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    fn installed(names: &'static [&'static str]) -> impl Fn(&str) -> bool {
        move |binary| names.contains(&binary)
    }

    fn work() -> ProfileRef {
        ProfileRef::parse("work").unwrap()
    }

    #[test]
    fn terminal_that_started_herdr_wins() {
        let env = env(&[("GNOME_TERMINAL_SCREEN", "/org/x")]);
        let has = installed(&["kitty", "gnome-terminal"]);
        let launcher = Launcher::new(Path::new("/opt/herdr"), None, &env, Platform::Linux, &has);
        assert_eq!(
            launcher.argv(&work()).unwrap(),
            [
                "gnome-terminal",
                "--window",
                "--title",
                "herdr · work",
                "--",
                "/opt/herdr",
                "--session",
                "work"
            ]
        );
    }

    #[test]
    fn falls_back_to_path_order() {
        let env = env(&[]);
        let has = installed(&["xterm", "alacritty"]);
        let launcher = Launcher::new(Path::new("/opt/herdr"), None, &env, Platform::Linux, &has);
        assert_eq!(launcher.argv(&work()).unwrap()[0], "alacritty");
        let none = installed(&[]);
        let launcher = Launcher::new(Path::new("/opt/herdr"), None, &env, Platform::Linux, &none);
        assert!(launcher.argv(&work()).is_none());
    }

    #[test]
    fn default_profile_runs_plain_herdr() {
        let env = env(&[]);
        let has = installed(&["xterm"]);
        let launcher = Launcher::new(Path::new("/opt/herdr"), None, &env, Platform::Linux, &has);
        assert_eq!(
            launcher.argv(&ProfileRef::Default).unwrap(),
            ["xterm", "-T", "herdr · default", "-e", "/opt/herdr"]
        );
    }

    #[test]
    fn window_env_drops_parent_terminal_identity() {
        std::env::set_var("GNOME_TERMINAL_SCREEN", "/org/gnome/Terminal/screen/stale");
        std::env::set_var("HERDR_PROBE", "1");
        let keys: Vec<OsString> = window_env().map(|(key, _)| key).collect();
        assert!(!keys.contains(&OsString::from("GNOME_TERMINAL_SCREEN")));
        assert!(!keys.contains(&OsString::from("HERDR_PROBE")));
        assert!(keys.contains(&OsString::from("PATH")));
        std::env::remove_var("GNOME_TERMINAL_SCREEN");
        std::env::remove_var("HERDR_PROBE");
    }

    #[test]
    fn custom_terminal_and_macos() {
        let env = env(&[("TERM_PROGRAM", "Apple_Terminal")]);
        let custom = vec![
            "footclient".to_owned(),
            "--title".to_owned(),
            "{title}".to_owned(),
        ];
        let none = installed(&[]);
        let launcher = Launcher::new(
            Path::new("/opt/herdr"),
            Some(&custom),
            &env,
            Platform::Linux,
            &none,
        );
        assert_eq!(
            launcher.argv(&work()).unwrap(),
            [
                "footclient",
                "--title",
                "herdr · work",
                "/opt/herdr",
                "--session",
                "work"
            ]
        );
        let launcher = Launcher::new(Path::new("/opt/herdr"), None, &env, Platform::MacOs, &none);
        let argv = launcher.argv(&work()).unwrap();
        assert_eq!(argv[0], "osascript");
        assert_eq!(
            argv[2],
            "tell application \"Terminal\" to do script \"/opt/herdr --session work\""
        );
    }

    #[test]
    fn profile_env_runs_through_exec() {
        let env = env(&[]);
        let has = installed(&["xterm"]);
        let launcher = Launcher::new(Path::new("/opt/herdr"), None, &env, Platform::Linux, &has)
            .through_exec(Path::new("/opt/herdr-profiles"), &work());
        assert_eq!(
            launcher.argv(&work()).unwrap(),
            [
                "xterm",
                "-T",
                "herdr · work",
                "-e",
                "/opt/herdr-profiles",
                "exec",
                "work",
                "--",
                "/opt/herdr",
                "--session",
                "work"
            ]
        );
    }
}
