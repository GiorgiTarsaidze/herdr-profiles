use std::collections::HashSet;
use std::path::PathBuf;

use anyhow::{bail, Result};
use serde::Serialize;

use crate::herdr::{self, Herdr, Session};
use crate::launcher::{self, Launcher, Platform};
use crate::settings::{self, Profile, ProfileName, ProfileRef, Settings};

/// One line of the chooser: a profile and what Herdr currently knows about it.
#[derive(Clone, Debug, Serialize)]
pub struct ProfileRow {
    #[serde(skip)]
    pub id: ProfileRef,
    name: String,
    pub label: String,
    pub running: bool,
    pub attached: bool,
    pub session_dir: PathBuf,
    pub socket_path: PathBuf,
    pub current: bool,
    pub protected: bool,
    pub spaces: usize,
    pub blocked: usize,
    /// `target` or `target/session` of a remote profile.
    pub remote: Option<String>,
}

impl ProfileRow {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn state(&self) -> &'static str {
        match (self.attached, self.running, self.remote.is_some()) {
            (true, _, _) => "open",
            (false, _, true) => "remote",
            (false, true, _) => "running",
            (false, false, _) => "stopped",
        }
    }
}

/// Saved profiles plus the operations the chooser and CLI perform on them.
pub struct ProfileStore<'a> {
    herdr: &'a Herdr,
    settings: Settings,
    path: PathBuf,
    current: ProfileRef,
}

impl<'a> ProfileStore<'a> {
    pub fn load(herdr: &'a Herdr) -> Self {
        let path = settings::settings_path();
        Self {
            herdr,
            settings: Settings::load(&path),
            path,
            current: Herdr::current_session(),
        }
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn rows(&self, with_counts: bool) -> Vec<ProfileRow> {
        self.rows_from(&self.herdr.sessions(), with_counts)
    }

    fn rows_from(&self, sessions: &[Session], with_counts: bool) -> Vec<ProfileRow> {
        let attached = herdr::attached_sessions();
        self.rows_with(sessions, &attached, with_counts)
    }

    fn rows_with(
        &self,
        sessions: &[Session],
        attached: &HashSet<String>,
        with_counts: bool,
    ) -> Vec<ProfileRow> {
        let mut order = vec![ProfileRef::Default];
        order.extend(self.settings.profiles.iter().map(Profile::to_ref));
        let mut adopted: Vec<ProfileName> = sessions
            .iter()
            .filter(|s| !herdr::is_default_session(s))
            .filter_map(|s| s.name.parse().ok())
            .filter(|name| self.settings.profile(name).is_none())
            .collect();
        adopted.sort();
        order.extend(adopted.into_iter().map(ProfileRef::Named));
        order
            .into_iter()
            .map(|id| self.row(id, sessions, attached, with_counts))
            .collect()
    }

    fn row(
        &self,
        id: ProfileRef,
        sessions: &[Session],
        attached: &HashSet<String>,
        with_counts: bool,
    ) -> ProfileRow {
        let session = sessions.iter().find(|s| match &id {
            ProfileRef::Default => herdr::is_default_session(s),
            ProfileRef::Named(name) => s.name == name.as_str(),
            ProfileRef::Remote { .. } => false,
        });
        let session_dir = session
            .and_then(|s| s.session_dir.clone())
            .unwrap_or_else(|| self.herdr.session_dir(&id));
        let socket_path = session
            .and_then(|s| s.socket_path.clone())
            .unwrap_or_else(|| session_dir.join("herdr.sock"));
        let running = session.is_some_and(|s| s.running);
        let attached = attached.contains(&id.attach_key());
        let label = match &id {
            ProfileRef::Default => id.name().to_owned(),
            ProfileRef::Named(name) | ProfileRef::Remote { name, .. } => self
                .settings
                .profile(name)
                .map_or_else(|| name.to_string(), |p| p.label().to_owned()),
        };
        let remote = match &id {
            ProfileRef::Remote {
                target, session, ..
            } => Some(match session {
                Some(session) => format!("{target}/{session}"),
                None => target.clone(),
            }),
            _ => None,
        };
        // ponytail: no counts for remote profiles, that would mean an SSH round trip.
        let with_counts = with_counts && remote.is_none();
        let counts = match (with_counts, running) {
            (false, _) => None,
            (true, true) => herdr::live_counts(&socket_path),
            (true, false) => None,
        }
        .unwrap_or_else(|| herdr::Counts {
            spaces: if with_counts {
                herdr::saved_space_count(&session_dir)
            } else {
                0
            },
            blocked: 0,
        });
        ProfileRow {
            name: id.name().to_owned(),
            label,
            running,
            attached,
            session_dir,
            socket_path,
            current: id == self.current,
            protected: id.is_default(),
            spaces: counts.spaces,
            blocked: counts.blocked,
            remote,
            id,
        }
    }

    /// A profile by name: a saved one (local or remote), or a plain session name.
    pub fn resolve(&self, name: &str) -> Result<ProfileRef> {
        let id = ProfileRef::parse(name)?;
        let ProfileRef::Named(name) = &id else {
            return Ok(id);
        };
        Ok(self
            .settings
            .profile(name)
            .map_or(id.clone(), Profile::to_ref))
    }

    pub fn add(
        &mut self,
        name: &str,
        label: Option<String>,
        remote: Option<String>,
        remote_session: Option<String>,
    ) -> Result<()> {
        let name: ProfileName = name.parse()?;
        if self.settings.profile(&name).is_some() {
            bail!("profile '{name}' already exists");
        }
        let label = label.filter(|l| !l.is_empty() && l != name.as_str());
        let remote = remote.filter(|t| !t.is_empty());
        let remote_session = remote_session.filter(|s| !s.is_empty() && remote.is_some());
        self.settings.profiles.push(Profile {
            name,
            label,
            remote,
            remote_session,
        });
        self.settings.save(&self.path)
    }

    pub fn remove(&mut self, target: &ProfileRef) -> Result<()> {
        self.remove_with(target, &self.herdr.sessions())
    }

    fn remove_with(&mut self, target: &ProfileRef, sessions: &[Session]) -> Result<()> {
        let name = match target {
            ProfileRef::Default => bail!("the default profile cannot be deleted"),
            // Only forgets the profile; the host keeps its session.
            ProfileRef::Remote { name, .. } => name,
            ProfileRef::Named(name) => {
                if sessions
                    .iter()
                    .any(|s| s.name == name.as_str() && s.running)
                {
                    bail!("profile '{name}' is running; stop it first");
                }
                if self.herdr.session_dir(target).is_dir() {
                    self.herdr.delete_session(name)?;
                }
                name
            }
        };
        self.settings.profiles.retain(|p| &p.name != name);
        if self.settings.last.as_deref() == Some(name.as_str()) {
            self.settings.last = None;
        }
        self.settings.save(&self.path)
    }

    pub fn stop(&self, target: &ProfileRef) -> Result<()> {
        match target {
            ProfileRef::Default => {
                bail!("the default profile is Herdr itself; use `herdr server stop`")
            }
            ProfileRef::Named(name) => self.herdr.stop_session(name),
            ProfileRef::Remote { target, .. } => {
                bail!("'{target}' is a remote host; stop the session there")
            }
        }
    }

    /// Opens the profile in a new terminal window and remembers it as the last used.
    pub fn open(&mut self, target: &ProfileRef, dry_run: bool) -> Result<Vec<String>> {
        if herdr::attached_sessions().contains(&target.attach_key()) {
            if target == &self.current {
                bail!("'{target}' is this window");
            }
            bail!("'{target}' is already open in another window");
        }
        let env = herdr::env_map();
        let launcher = Launcher::new(
            self.herdr.bin(),
            self.settings.terminal.as_deref(),
            &env,
            Platform::current(),
            &launcher::is_on_path,
        );
        let Some(argv) = launcher.argv(target) else {
            bail!(
                "no terminal emulator found; set \"terminal\" in {}",
                self.path.display()
            );
        };
        if dry_run {
            return Ok(argv);
        }
        launcher::spawn_detached(&argv)?;
        self.settings.last = Some(target.name().to_owned());
        self.settings.save(&self.path)?;
        Ok(argv)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn store<'a>(dir: &std::path::Path, herdr: &'a Herdr) -> ProfileStore<'a> {
        let path = dir.join("profiles.json");
        ProfileStore {
            herdr,
            settings: Settings::load(&path),
            path,
            current: ProfileRef::Default,
        }
    }

    #[test]
    fn rows_merge_saved_profiles_with_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let herdr = Herdr::from_env();
        let mut store = store(dir.path(), &herdr);
        store.add("work", Some("Work".into()), None, None).unwrap();
        store
            .add("box", None, Some("workbox".into()), Some("agents".into()))
            .unwrap();
        let study_dir = dir.path().join("study");
        fs::create_dir_all(&study_dir).unwrap();
        fs::write(
            study_dir.join("session.json"),
            r#"{"workspaces":[{"id":"w1"},{"id":"w2"}]}"#,
        )
        .unwrap();
        let sessions = vec![
            Session {
                name: "default".into(),
                default: true,
                running: true,
                ..Session::default()
            },
            Session {
                name: "study".into(),
                session_dir: Some(study_dir),
                ..Session::default()
            },
        ];
        let attached = HashSet::from(["study".to_owned()]);
        let rows = store.rows_with(&sessions, &attached, true);
        let names: Vec<&str> = rows.iter().map(ProfileRow::name).collect();
        assert_eq!(names, ["default", "work", "box", "study"]);
        assert!(rows[0].protected && rows[0].running && rows[0].current);
        assert_eq!(
            (
                rows[0].state(),
                rows[1].state(),
                rows[2].state(),
                rows[3].state()
            ),
            ("running", "stopped", "remote", "open")
        );
        assert_eq!((rows[1].label.as_str(), rows[1].protected), ("Work", false));
        assert_eq!(rows[2].remote.as_deref(), Some("workbox/agents"));
        assert_eq!((rows[3].spaces, rows[3].running), (2, false));
        let attached = HashSet::from(["workbox#agents".to_owned()]);
        assert_eq!(
            store.rows_with(&sessions, &attached, false)[2].state(),
            "open"
        );
    }

    #[test]
    fn add_validates_and_rejects_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let herdr = Herdr::from_env();
        let mut store = store(dir.path(), &herdr);
        store.add("work", None, None, None).unwrap();
        assert!(store.add("work", None, None, None).is_err());
        assert!(store.add("bad name", None, None, None).is_err());
        assert!(store.add("default", None, None, None).is_err());
        let reloaded = Settings::load(&dir.path().join("profiles.json"));
        assert_eq!(reloaded.profiles.len(), 1);
        assert_eq!(reloaded.profiles[0].label(), "work");
    }

    #[test]
    fn remove_and_stop_rules() {
        let dir = tempfile::tempdir().unwrap();
        let herdr = Herdr::from_env();
        let mut store = store(dir.path(), &herdr);
        store.add("work", None, None, None).unwrap();
        store
            .add("box", None, Some("workbox".into()), None)
            .unwrap();
        let work = ProfileRef::parse("work").unwrap();
        let remote = store.resolve("box").unwrap();
        assert_eq!(remote.session_args(), ["--remote", "workbox"]);
        assert!(store.stop(&remote).is_err());
        store.remove_with(&remote, &[]).unwrap();
        let running = vec![Session {
            name: "work".into(),
            running: true,
            ..Session::default()
        }];
        assert!(store.remove_with(&work, &running).is_err());
        assert!(store.remove_with(&ProfileRef::Default, &[]).is_err());
        assert!(store.stop(&ProfileRef::Default).is_err());
        store.remove_with(&work, &[]).unwrap();
        assert!(Settings::load(&dir.path().join("profiles.json"))
            .profiles
            .is_empty());
    }
}
