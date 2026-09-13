//! Chrome-style profiles for Herdr.
//!
//! A profile is a Herdr named session, so every profile owns a separate set of
//! spaces, tabs, panes and agents. This binary is the whole plugin: the popup
//! chooser, the startup hook, keybinding setup and a plain command line.

mod cli;
mod herdr;
mod keybinding;
mod launcher;
mod popup;
mod profiles;
mod settings;
mod socket;
mod theme;
mod ui;

use std::process::ExitCode;

use clap::Parser;

fn main() -> ExitCode {
    match cli::Cli::parse().run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("herdr-profiles: {err:#}");
            ExitCode::FAILURE
        }
    }
}
