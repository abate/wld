# Changelog

## Unreleased

### New Commands

- **`wld segment`** — manage LED segments (list, set, delete, export, import)
- **`wld preset`** — manage presets (list, save, load, delete)
- **`wld config`** — configure device settings
  - `export` / `apply` — full config backup and restore as JSON
  - `diff` — compare local config file against live device
  - `wifi` — SSID, password, mDNS hostname, PHY mode (g/n)
  - `ota` — lock/unlock OTA updates (unlock requires `--password` when locked)
  - `led` — LED count, type, pin, color order
- **`wld debug`** — inspect device state
  - `info` — version, WiFi signal, memory, LED type, PHY mode, uptime
  - `live` — real-time LED RGB values via WebSocket
  - `effects` / `palettes` — list available effects and color palettes
  - `dump` — full JSON state dump to file or stdout
  - `watch` — continuously poll device stats
- **`wld update`** — OTA firmware updates from GitHub releases
  - Auto-detects platform (ESP8266, ESP32, etc.)
  - `--check` flag for version comparison without downloading
  - `--file` flag for uploading local firmware files (skips validation)
  - `--skip-validation` flag for GitHub-sourced firmware
  - Auto-retries with `.bin.gz` when device reports "Not Enough Space"
  - Pre-checks OTA lock status before downloading
  - Strips HTML from device error responses for readable messages
- **`wld completions`** — generate static shell completions

### New Features

- **Dynamic shell completions** — tab-complete device names, segment IDs (with names), and preset IDs using `COMPLETE=bash wld` (bash/zsh/fish)
- **Global `--dry-run` flag** — preview any mutating command without applying changes
- **Percentage brightness** — `wld brightness 75 -p` for 0-100% scale
- **Segment names** — shown in `segment list` output
- **Power display** — shows estimated power with current limit
- **Long help text** — `--help` shows detailed descriptions and examples for all commands
- **IaC workflow** — example script in `iac/wled-iac.sh` for git-tracked device config management

### MCP Server

Expanded from 5 to 17 tools:

- `wled_segment_list`, `wled_segment_set`, `wled_segment_delete`
- `wled_preset_list`, `wled_preset_save`, `wled_preset_load`, `wled_preset_delete`
- `wled_debug_info`, `wled_debug_effects`, `wled_debug_palettes`
- `wled_config_export`, `wled_config_diff`

### Security

- SSRF prevention for device IP addresses
- MCP tool timeouts to prevent hanging
- Config file permission hardening

### Bug Fixes

- Fixed WebSocket binary live data parsing (0x4c header + RGB triplets)
- Fixed firmware `.bin.gz` preference logic (prefer `.bin` first, retry with `.bin.gz` on space errors)
- Local firmware uploads always skip validation (custom builds lack metadata)
