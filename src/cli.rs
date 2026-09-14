use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};

use crate::herdr::Herdr;
use crate::keybinding::{self, KeyCombo};
use crate::launcher;
use crate::popup::{self, Entrypoint};
use crate::profiles::ProfileStore;
use crate::settings::{ProfileRef, Settings};
use crate::ui;

const STARTUP_RETRIES: u32 = 60;
const STARTUP_RETRY_DELAY: Duration = Duration::from_millis(500);

#[derive(Parser)]
#[command(
    name = "herdr-profiles",
    version,
    about = "Chrome-style profiles for Herdr"
)]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Interactive popup (run by Herdr)
    Chooser {
        #[arg(long)]
        setup: bool,
    },
    /// Open the popup in the current Herdr session
    OpenChooser {
        #[arg(long)]
        setup: bool,
    },
    /// Herdr startup hook
    Startup,
    /// Manage the chooser keybinding in Herdr's config.toml
    Setup {
        /// Bind this key, for example prefix+a
        #[arg(long)]
        key: Option<String>,
        /// Print the current binding and the keys already in use
        #[arg(long)]
        print: bool,
        /// Bind the key even if something else uses it
        #[arg(long)]
        force: bool,
    },
    /// List profiles
    List {
        #[arg(long)]
        json: bool,
    },
    /// Create a profile
    Add { name: String, label: Option<String> },
    /// Open a profile in a new terminal window
    Open {
        name: String,
        #[arg(long)]
        dry_run: bool,
    },
    /// Stop a running profile
    Stop { name: String },
    /// Delete a stopped profile and its saved spaces
    Remove { name: String },
    /// Run a command with a profile's environment (used by profile windows)
    #[command(hide = true)]
    Exec {
        /// profiles.json to read the environment from
        #[arg(long)]
        settings: PathBuf,
        name: String,
        #[arg(last = true, required = true)]
        command: Vec<String>,
    },
}

impl Cli {
    pub fn run(self) -> Result<()> {
        let herdr = Herdr::from_env();
        let mut store = ProfileStore::load(&herdr);
        match self.command {
            Command::Chooser { setup } => ui::run(&mut store, setup),
            Command::OpenChooser { setup } => {
                popup::open(Entrypoint::from_setup_flag(setup), 1, Duration::ZERO)
            }
            Command::Startup => {
                if popup::should_open_on_startup(store.settings(), &Herdr::current_session()) {
                    popup::open(Entrypoint::Chooser, STARTUP_RETRIES, STARTUP_RETRY_DELAY)?;
                }
                Ok(())
            }
            Command::Setup { key, print, force } => setup(&herdr, key, print, force),
            Command::List { json } => list(&store, json),
            Command::Add { name, label } => {
                store.add(&name, label)?;
                println!("created profile '{name}'");
                Ok(())
            }
            Command::Open { name, dry_run } => {
                let target = ProfileRef::parse(&name)?;
                let argv = store.open(&target, dry_run)?;
                if dry_run {
                    println!("{}", argv.join(" "));
                }
                Ok(())
            }
            Command::Stop { name } => store.stop(&ProfileRef::parse(&name)?),
            Command::Remove { name } => {
                store.remove(&ProfileRef::parse(&name)?)?;
                println!("removed profile '{name}'");
                Ok(())
            }
            Command::Exec {
                settings,
                name,
                command,
            } => {
                let env = Settings::read(&settings)?.profile_env(&ProfileRef::parse(&name)?);
                launcher::exec(&command, env)
            }
        }
    }
}

fn list(store: &ProfileStore, json: bool) -> Result<()> {
    let rows = store.rows(true);
    let mut out = io::stdout().lock();
    if json {
        serde_json::to_writer_pretty(&mut out, &rows)?;
        writeln!(out)?;
        return Ok(());
    }
    for row in rows {
        let here = if row.current { "  (this window)" } else { "" };
        let plural = if row.spaces == 1 { "" } else { "s" };
        writeln!(
            out,
            "{:<20} {:<8} {} space{plural}{here}",
            row.name(),
            row.state(),
            row.spaces
        )?;
    }
    Ok(())
}

fn setup(herdr: &Herdr, key: Option<String>, print: bool, force: bool) -> Result<()> {
    let config = herdr.config_path();
    let status = keybinding::status(&config, &herdr.default_config())?;
    if print {
        println!("{}", serde_json::to_string_pretty(&status)?);
        return Ok(());
    }
    if let Some(key) = key {
        let key: KeyCombo = key.parse()?;
        if let Some(owner) = status.owner_of(&key) {
            if !force {
                bail!("{key} is already used by {owner}; pass --force to bind it anyway");
            }
        }
        keybinding::apply(&config, &key)
            .with_context(|| format!("could not write {}", config.display()))?;
        herdr.reload_config();
        println!("herdr-profiles: bound to {key} ({})", config.display());
        return Ok(());
    }
    if let Some(binding) = &status.binding {
        println!(
            "herdr-profiles: already bound to {} ({})",
            binding.key, binding.source
        );
        return Ok(());
    }
    let Some(key) = status.free_key() else {
        bail!("no free key found; run: herdr-profiles setup --key <key>");
    };
    keybinding::apply(&config, &key)
        .with_context(|| format!("could not write {}", config.display()))?;
    herdr.reload_config();
    println!(
        "herdr-profiles: bound the profile chooser to {key} in {}",
        config.display()
    );
    println!("herdr-profiles: change it any time with `s` inside the popup, or: herdr plugin action invoke herdr-profiles.setup");
    Ok(())
}
