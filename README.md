# `wld`

Control [WLED](https://kno.wled.ge/) lights from the command line

---

## Features

- **Device management** — save multiple WLED devices by name, set a default
- **Power & brightness** — on/off/brightness with percentage support
- **Segments** — create, modify, delete, export and import LED segments
- **Presets** — save, load, delete device presets (IDs 1-250)
- **Configuration** — WiFi, OTA, LED hardware settings; export/import full config as JSON
- **Firmware updates** — OTA from GitHub releases or local files, auto-retry with compressed firmware
- **Debug tools** — device info, live LED values, effects/palettes list, watch mode, JSON dump
- **MCP server** — 17 tools for AI agent integration (Claude Desktop, etc.)
- **Shell completions** — dynamic completions for device names, segment IDs, and preset IDs
- **IaC workflow** — example script for git-tracked device configuration management
- **Dry-run mode** — preview any mutating command with `--dry-run`

## Installation

### macOS or Linux via [Homebrew](https://brew.sh/)

```bash
brew tap timrogers/tap && brew install wld
```

### macOS, Linux or Windows via [Cargo](https://doc.rust-lang.org/cargo/)

```bash
cargo install wld
```

### Direct binary download

Download the [latest release](https://github.com/timrogers/wld/releases/latest) for your platform (macOS, Linux, Windows), add to `$PATH`, and run `wld --help`.

## Shell Completions

`wld` supports dynamic shell completions that complete device names, segment IDs (with names), and preset IDs from your live configuration.

```bash
# Bash
source <(COMPLETE=bash wld)

# Zsh
source <(COMPLETE=zsh wld)

# Fish
COMPLETE=fish wld | source
```

Static completions are also available via `wld completions <shell>`.

## Usage

### Device Management

```bash
wld add bedroom 192.168.1.100    # Add a device (first becomes default)
wld add kitchen 10.0.0.42
wld ls                           # List all devices (* = default)
wld set-default kitchen          # Change default device
wld delete bedroom               # Remove a device
```

### Power & Brightness

```bash
wld on                           # Turn on default device
wld on -d kitchen                # Turn on a specific device
wld off -d 192.168.1.100         # Turn off by IP address
wld brightness 128               # Set to ~50% (0-255)
wld brightness 75 -p             # Set to 75% using percentage
wld status                       # Check all devices (on/off/unreachable)
```

### Segments

Segments divide your LED strip into independently controlled sections.

```bash
wld segment list                              # Show all segments
wld segment set --id 0 --color "#FF0000"      # Set color to red
wld segment set --id 1 --effect 42 --speed 128
wld segment delete --id 2                     # Delete a segment
wld segment export -o segments.json           # Back up segments
wld segment import segments.json              # Restore segments
```

### Presets

Presets save the complete device state (colors, effects, segments).

```bash
wld preset list                               # Show all presets
wld preset save --id 1 --name "Movie"         # Save current state
wld preset load --id 1                        # Recall a preset
wld preset delete --id 3                      # Remove a preset
```

### Configuration

```bash
wld config export -o backup.json              # Back up full device config
wld config apply backup.json                  # Restore config from file
wld config diff backup.json                   # Compare local file vs device
wld config wifi --ssid MyNetwork --password secret
wld config wifi --mdns mydevice --phy-mode n  # Set mDNS hostname and WiFi PHY
wld config ota --unlock --password wledota    # Unlock OTA updates
wld config led --count 60 --led-type WS2812B  # Configure LED hardware
```

### Firmware Updates

```bash
wld update --check                            # Check for updates
wld update                                    # Update to latest release
wld update --version 0.15.0                   # Update to specific version
wld update -y                                 # Skip confirmation prompt
wld update --file firmware.bin.gz             # Upload local firmware file
```

OTA must be unlocked before updating. If the device reports "Not Enough Space", `wld` automatically retries with the compressed `.bin.gz` variant.

### Debug & Inspection

```bash
wld debug info                   # Version, WiFi, memory, LED type, PHY mode
wld debug info --json            # Raw JSON output
wld debug live                   # Live LED RGB values via WebSocket
wld debug effects                # List all available effects
wld debug palettes               # List all color palettes
wld debug dump -o state.json     # Full JSON state dump
wld debug watch                  # Continuously poll device stats
```

### Dry Run

Add `--dry-run` to any mutating command to preview changes without applying them:

```bash
wld config wifi --ssid NewNet --dry-run
wld segment set --id 0 --color "#00FF00" --dry-run
```

## IaC Workflow

An example script is included in `iac/wled-iac.sh` for managing device configurations as version-controlled JSON files:

```bash
./iac/wled-iac.sh init           # Export all devices + git init
# ... edit JSON files ...
./iac/wled-iac.sh diff           # Compare local files vs live devices
./iac/wled-iac.sh apply          # Push configs to all devices
./iac/wled-iac.sh apply shower   # Push to a single device
./iac/wled-iac.sh snapshot       # Re-export after manual changes
```

## MCP Server

Running `wld mcp` starts a [Model Context Protocol](https://modelcontextprotocol.io/) server over stdio for AI agent integration.

### Setup with Claude Desktop

Add to your Claude Desktop MCP config:

```json
{
  "mcpServers": {
    "wld": {
      "command": "wld",
      "args": ["mcp"]
    }
  }
}
```

### Available Tools (17)

| Tool | Description |
|------|-------------|
| `wled_devices` | List saved devices with names, IPs, and default |
| `wled_on` | Turn device on |
| `wled_off` | Turn device off |
| `wled_brightness` | Set brightness (0-255) |
| `wled_status` | Check status of all devices |
| `wled_segment_list` | List all segments |
| `wled_segment_set` | Create or modify a segment |
| `wled_segment_delete` | Delete a segment |
| `wled_preset_list` | List all presets |
| `wled_preset_save` | Save current state as a preset |
| `wled_preset_load` | Load a preset |
| `wled_preset_delete` | Delete a preset |
| `wled_debug_info` | Device info (version, WiFi, memory, LEDs) |
| `wled_debug_effects` | List available effects |
| `wled_debug_palettes` | List available color palettes |
| `wled_config_export` | Export full device config as JSON |
| `wled_config_diff` | Diff local config file vs device |
