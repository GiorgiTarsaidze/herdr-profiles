use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::settings::{ProfileRef, Settings};
use crate::socket;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entrypoint {
    Chooser,
    Setup,
}

impl Entrypoint {
    pub fn from_setup_flag(setup: bool) -> Self {
        if setup {
            Self::Setup
        } else {
            Self::Chooser
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::Chooser => "chooser",
            Self::Setup => "setup",
        }
    }
}

/// Asks the Herdr server that owns this process to open one of our popups.
pub fn open(entrypoint: Entrypoint, retries: u32, delay: Duration) -> Result<()> {
    let socket_path = std::env::var_os("HERDR_SOCKET_PATH").map(PathBuf::from);
    let plugin_id = std::env::var("HERDR_PLUGIN_ID").ok();
    let (Some(socket_path), Some(plugin_id)) = (socket_path, plugin_id) else {
        return Err(anyhow!(
            "not running inside Herdr (HERDR_SOCKET_PATH/HERDR_PLUGIN_ID missing)"
        ));
    };
    let params = json!({ "plugin_id": plugin_id, "entrypoint": entrypoint.id() });
    open_with(retries, delay, || {
        socket::call(&socket_path, "plugin.pane.open", params.clone())
    })
}

/// Retries transient failures: the server may not be up yet, or a client may
/// not be attached, and the popup only opens from the normal workspace view.
fn open_with(
    retries: u32,
    delay: Duration,
    mut call: impl FnMut() -> Result<Value, socket::Error>,
) -> Result<()> {
    let mut last = None;
    for attempt in 1..=retries.max(1) {
        match call() {
            Ok(_) => return Ok(()),
            Err(err) if err.to_string().contains("popup already open") => return Ok(()),
            Err(err) => {
                let transient = err.is_transient();
                last = Some(err);
                if !transient || attempt == retries {
                    break;
                }
                thread::sleep(delay);
            }
        }
    }
    Err(last.map_or_else(|| anyhow!("popup could not be opened"), Into::into))
}

/// Chrome-style launch chooser: only in the default session, only when saved profiles exist.
pub fn should_open_on_startup(settings: &Settings, session: &ProfileRef) -> bool {
    session.is_default() && settings.chooser_on_launch && !settings.profiles.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Profile;

    fn api(code: &str, message: &str) -> socket::Error {
        socket::Error::Api {
            code: code.into(),
            message: message.into(),
        }
    }

    #[test]
    fn retries_ui_busy_and_io_errors_only() {
        let mut calls = 0;
        let result = open_with(5, Duration::ZERO, || {
            calls += 1;
            if calls < 3 {
                Err(api(
                    "ui_busy",
                    "popup panes can only open from the normal workspace view",
                ))
            } else {
                Ok(json!({ "type": "ok" }))
            }
        });
        assert!(result.is_ok());
        assert_eq!(calls, 3);

        let mut calls = 0;
        let result = open_with(5, Duration::ZERO, || {
            calls += 1;
            Err(api("plugin_pane_open_failed", "nope"))
        });
        assert!(result.unwrap_err().to_string().contains("nope"));
        assert_eq!(calls, 1);

        let mut calls = 0;
        let result = open_with(3, Duration::ZERO, || {
            calls += 1;
            Err(socket::Error::Io(std::io::Error::other("no socket")))
        });
        assert!(result.is_err());
        assert_eq!(calls, 3);

        assert!(open_with(1, Duration::ZERO, || Err(api(
            "plugin_pane_open_failed",
            "popup already open"
        )))
        .is_ok());
    }

    #[test]
    fn startup_gating() {
        let mut settings = Settings::default();
        let work = ProfileRef::parse("work").unwrap();
        assert!(!should_open_on_startup(&settings, &ProfileRef::Default));
        settings.profiles.push(Profile {
            name: "work".parse().unwrap(),
            label: None,
        });
        assert!(should_open_on_startup(&settings, &ProfileRef::Default));
        assert!(!should_open_on_startup(&settings, &work));
        settings.chooser_on_launch = false;
        assert!(!should_open_on_startup(&settings, &ProfileRef::Default));
    }
}
