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
| `e`           | edit `profiles.json` in `$EDITOR`, new window |
| `s`           | change the hotkey                             |
| `x`           | stop a running profile                        |
| `d`           | delete a stopped profile                      |
| `q`           | close                                         |

`default` is Herdr's own session. It is always listed and cannot be deleted.
A profile that already has a window open cannot be opened twice.

## Settings

`~/.config/herdr/plugins/config/herdr-profiles/profiles.json` — press `e` in
the popup to open it.

```json
{
  "chooser_on_launch": true,
  "terminal": null,
  "profiles": [{ "name": "work", "label": "Work" }]
}
```

| Field               | Meaning                                                        |
| ------------------- | -------------------------------------------------------------- |
| `chooser_on_launch` | show the popup when Herdr starts                               |
| `terminal`          | command that opens new windows; `null` picks one (see below)   |
| `profiles[].name`   | session name: ASCII letters, digits, `.`, `_`, `-`             |
| `profiles[].label`  | optional display name                                          |
| `profiles[].env`    | optional environment for the profile, see below                |

By default new windows open in the terminal Herdr runs in; inside WSL that
is a new Windows Terminal tab in the same distribution. To pick one, set
`terminal`, for example `["kitty", "--title", "{title}"]`; `{title}` becomes
the window title.

If the file is not valid JSON the popup says so on start and refuses to
change profiles until it is fixed, so a typo never wipes your profiles.

### Per-profile environment

`env` sets environment variables for everything started in the profile's
window: shells, agents, tools. The typical use is a separate Claude Code
account per profile.

1. Create the profile with `n` in the popup.
2. Press `e` and add an `env` object to it.
3. If the profile is running, stop it with `x`: the environment is read
   when its session starts.
4. Open it with `enter` and check, e.g. `echo $CLAUDE_CONFIG_DIR`.

```json
{
  "profiles": [
    {
      "name": "router",
      "label": "API router",
      "env": {
        "CLAUDE_CONFIG_DIR": "~/.claude-router",
        "ANTHROPIC_BASE_URL": "https://router.example.com",
        "ANTHROPIC_DEFAULT_OPUS_MODEL": "router-opus",
        "ANTHROPIC_DEFAULT_SONNET_MODEL": "router-sonnet"
      }
    },
    {
      "name": "vertex",
      "label": "Vertex AI",
      "env": {
        "CLAUDE_CONFIG_DIR": "~/.claude-vertex",
        "CLAUDE_CODE_USE_VERTEX": "1",
        "CLOUD_ML_REGION": "us-east5",
        "ANTHROPIC_VERTEX_PROJECT_ID": "my-project"
      }
    }
  ]
}
```

- `CLAUDE_CONFIG_DIR` gives the profile its own Claude Code login, history,
  settings and MCP servers. Without it every profile shares `~/.claude`.
- For a router, put its token in `~/.claude-router/settings.json` as
  `{ "apiKeyHelper": "cat ~/.secrets/router-token" }` with the token file at
  `chmod 600`. `ANTHROPIC_AUTH_TOKEN` in `env` works too, but
  `profiles.json` is plain text.
- Vertex authenticates through Google: run
  `gcloud auth application-default login`, or add
  `GOOGLE_APPLICATION_CREDENTIALS` pointing at a service account key.
- A leading `~` in values is expanded.
- `default` is Herdr's own session and keeps the environment Herdr was
  started with.
- Deleting a profile with `d` removes its `env` too.

The window starts through `herdr-profiles exec`, which sets the environment
before running Herdr, so it works with any terminal.

## Notes

- Tested on Linux. macOS builds are provided but not yet tested.
- The popup uses your Herdr theme colors.
- Development: `cargo build --release`, `cargo test`. Releases are tagged
  `vX.Y.Z`, matching `version` in `Cargo.toml` and `herdr-plugin.toml`.
