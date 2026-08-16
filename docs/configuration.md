# Configuration

Orange Yeoman reads its configuration from JSON files. Configuration is merged
from built-in defaults, then a global file, then a per-repository file. Later
sources override earlier sources per key.

## Configuration files

Global configuration (applies to every repository):

```
~/Library/Application Support/Orange Yeoman/config.json
```

Repository configuration (applies only to that repository). Place this file in
the root of the watched folder:

```
<watched-folder>/.orange-yeoman.json
```

The `.orange-yeoman.json` file is ignored by git. A safe example with placeholder
values lives at `.orange-yeoman.json.example` in this repository.

## JSON shape

```json
{
  "apiKeys": {
    "openai": "",
    "deepseek": ""
  },
  "models": {
    "small": "deepseek-v4-flash",
    "large": "openai-codex-5.6"
  }
}
```

- `apiKeys` — a map of provider name to API key. Empty strings are accepted and
  simply mean "not configured".
- `models.small` — the model used for cheap/fast work. Default:
  `deepseek-v4-flash`.
- `models.large` — the model used for heavy work. Default: `openai-codex-5.6`.

All sections are optional. A missing file is valid. A file with only some keys
overrides only those keys.

## Precedence

1. Built-in defaults.
2. Global config (`config.json`).
3. Repository config (`.orange-yeoman.json`).

Repository values override global values. Objects merge by key, so a repository
config that omits `apiKeys` does not clear the global API keys.

## Behavior notes

- No config file is created automatically. The app treats a missing file as
  absent.
- If the global parent directory does not exist, the global config is treated
  as absent.
- Invalid JSON surfaces as a user-visible error in the app; the app continues
  with the last valid/default configuration.
- When the repository `.orange-yeoman.json` changes while the app is watching,
  the merged configuration reloads automatically.
- The repository config is read only from the active watched root. The app
  never reads project config from outside that root.

## API key warning

API key values never leave the Rust core. They are not sent to the frontend, not
included in status or event payloads, and never logged. Only provider names are
shown in the UI. Do not commit real keys; the repository config file is git
ignored, but treat any copy of it as a secret.
