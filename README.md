<div align="center">

# herdr-profiles

Chrome-style profiles for [Herdr](https://herdr.dev).<br>
Keep work, personal and study spaces apart, and switch between them from a popup.

<img src="https://raw.githubusercontent.com/GiorgiTarsaidze/herdr-profiles/assets/demo.gif" alt="Opening a profile from the herdr-profiles popup" width="820">

</div>

## What it does

A profile is a Herdr named session: its own spaces, agents and saved layout.
This plugin lists your profiles in a popup and opens the one you pick in a new
terminal window. When Herdr starts and you have at least one profile, the popup
appears on its own, like Chrome's profile picker.

## Install

```sh
herdr plugin install GiorgiTarsaidze/herdr-profiles
```

The installer downloads the binary for your platform, or builds it with `cargo`
if there is none, then binds the popup to a free key in `~/.config/herdr/config.toml`.
The default key is `prefix+a`, and the Herdr prefix is `ctrl+b`.

## Use

Press `ctrl+b`, then `a`.

| Key           | Action                                        |
| ------------- | --------------------------------------------- |
| `↑` `↓`       | move                                          |
| `enter`       | open the profile in a new window              |
| `n`           | new profile                                   |
| `s`           | change the hotkey                             |
| `x`           | stop a running profile                        |
| `d`           | delete a stopped profile                      |
| `q`           | close                                         |

`default` is Herdr's own session. It is always listed and cannot be deleted.
A profile that already has a window open cannot be opened twice.

## Settings

`~/.config/herdr/plugins/config/herdr-profiles/profiles.json`

```json
{
  "chooser_on_launch": true,
  "terminal": null,
  "profiles": [{ "name": "work", "label": "Work" }]
}
```

Set `chooser_on_launch` to `false` to stop the popup at startup.
Set `terminal` to pick the terminal that opens new windows, for example
`["kitty", "--title", "{title}"]`. By default the plugin uses the terminal
Herdr runs in.

## Notes

- Tested on Linux. macOS builds are provided but not yet tested.
- The popup uses your Herdr theme colors.
- Development: `cargo build --release`, `cargo test`. Releases are tagged
  `vX.Y.Z`, matching `version` in `Cargo.toml` and `herdr-plugin.toml`.
