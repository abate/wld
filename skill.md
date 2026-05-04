# wld — WLED CLI Tool

`wld` is a command-line tool for controlling WLED smart LED controllers. It also exposes an MCP (Model Context Protocol) server so AI assistants can control your lights.

## Installation

```bash
cargo install --path .
# or with a pre-built binary:
# Download from GitHub releases and place in $PATH
```

## Global Flags

| Flag | Description |
|------|-------------|
| `--dry-run` | Preview what would happen without making any changes |
| `-d, --device <name_or_ip>` | Target a specific device by saved name or direct IP address |

## Device Management

```bash
# Add a device (first device becomes default)
wld add living_room 192.168.1.100

# List all saved devices
wld ls

# Set a different default device
wld set-default bedroom

# Remove a saved device
wld delete living_room
```

## Power & Brightness Control

```bash
# Turn default device on or off
wld on
wld off

# Turn a specific device on
wld on -d bedroom

# Set brightness (0-255)
wld brightness 128

# Set brightness as a percentage (0-100)
wld brightness 50 --percentage
wld brightness 75 -p -d living_room
```

## Segment Management

Segments divide your LED strip into independently controlled sections.

```bash
# List all segments on a device
wld segment list
wld segment list -d living_room

# Create or update a segment
wld segment set --id 0 --start 0 --stop 60 --color "255,0,0" --effect 1
wld segment set --id 1 --color "#00FF80" --brightness 200
wld segment set --id 0 --on
wld segment set --id 0 --off

# Delete a segment
wld segment delete --id 1

# Export segments to a JSON file (for backup or transfer)
wld segment export
wld segment export -o segments.json

# Import segments from a JSON file
wld segment import segments.json
wld --dry-run segment import segments.json
```

## Preset Management

Presets store the full lighting state for quick recall.

```bash
# List all presets on a device
wld preset list

# Save the current state as preset 1
wld preset save --id 1 --name "Cozy Evening"

# Load a preset
wld preset load --id 1

# Delete a preset
wld preset delete --id 1
```

## Device Configuration

```bash
# Export full device configuration to JSON
wld config export
wld config export -o my-device.json

# Apply a configuration JSON file to a device
wld config apply my-device.json
wld --dry-run config apply my-device.json

# Show differences between a local config file and the device
wld config diff my-device.json
# Exit code 0 = identical, 1 = differences found
# Lines prefixed with '+' are only in the local file
# Lines prefixed with '-' are only on the device
# Lines prefixed with '~' show changed values (local -> device)

# Configure WiFi
wld config wifi --ssid "MyNetwork" --password "secret"

# Configure OTA (Over-The-Air) updates
wld config ota --lock        # Prevent firmware updates
wld config ota --unlock      # Allow firmware updates
wld config ota --password "newpassword"

# Configure LED hardware
wld config led --power 3000 --led-type WS2812B --count 144
wld config led --led-type SK6812 --count 60 --pin 2
```

## Debug & Inspection

```bash
# Show device info (version, uptime, memory, WiFi, LEDs)
wld debug info
wld debug info --json          # Raw JSON output

# Show live LED values
wld debug live

# List available effects
wld debug effects

# List available color palettes
wld debug palettes

# Full state and info JSON dump
wld debug dump
wld debug dump -o dump.json

# Continuously watch device health stats
wld debug watch                # Polls every 2 seconds
wld debug watch --interval 5   # Poll every 5 seconds
# Press Ctrl+C to stop
```

## Firmware Update

```bash
# Update to the latest WLED firmware
wld update

# Check what update is available without downloading
wld update --check

# Update to a specific version
wld update --version 0.15.0

# Override platform detection
wld update --platform ESP02

# Skip the confirmation prompt
wld update --yes
wld update -y

# Dry-run: show what would happen without downloading
wld --dry-run update
```

When both `.bin.gz` and `.bin` firmware files are available, `wld` prefers `.bin.gz` because it is smaller and more likely to fit within ESP8266 2MB flash OTA partitions.

## IaC (Infrastructure-as-Code) Workflow

You can treat WLED device configuration like infrastructure code:

```bash
# 1. Export the current config to a file
wld config export -o devices/living-room.json

# 2. Commit it to git
git add devices/living-room.json
git commit -m "Export living room WLED config"

# 3. Edit the config in your editor
# (change effect, colors, LED count, etc.)

# 4. Review changes before applying
wld config diff devices/living-room.json

# 5. Apply the changes
wld config apply devices/living-room.json

# 6. Use --dry-run to preview without applying
wld --dry-run config apply devices/living-room.json
```

Similarly for segments:

```bash
# Export segments
wld segment export -o segments/living-room-segs.json

# Edit, diff, import
wld segment import segments/living-room-segs.json --dry-run
wld segment import segments/living-room-segs.json
```

## MCP Server

`wld` includes an MCP (Model Context Protocol) server that lets AI assistants like Claude control your lights.

### Configure in Claude Desktop

Add to `~/Library/Application Support/Claude/claude_desktop_config.json` (macOS):

```json
{
  "mcpServers": {
    "wld": {
      "command": "/path/to/wld",
      "args": ["mcp"]
    }
  }
}
```

### Start the MCP server manually

```bash
wld mcp
```

### Available MCP Tools

| Tool | Description |
|------|-------------|
| `wled_devices` | List all saved devices from config |
| `wled_on` | Turn a device on |
| `wled_off` | Turn a device off |
| `wled_brightness` | Set brightness (0-255) |
| `wled_status` | Check status of all devices |
| `wled_segment_list` | List all segments |
| `wled_segment_set` | Create or update a segment (id, start, stop, color, effect, speed, intensity, palette, brightness, on) |
| `wled_preset_list` | List all presets |
| `wled_preset_save` | Save current state as a preset (id, name) |
| `wled_preset_load` | Load a preset by ID |
| `wled_debug_info` | Get device info as formatted text |
| `wled_config_export` | Export device config as JSON string |
| `wled_config_diff` | Compare expected JSON config against device and return diff |

All tools accept an optional `device` parameter (device name or IP). If omitted, the default device is used.

### Example MCP usage (via Claude)

- "Turn on my living room lights"
- "Set the bedroom lights to 30% brightness"
- "Show me what effects are available"
- "Save the current state as preset 5"
- "What is the free heap on my WLED device?"
- "Compare this config with what's on the device: { ... }"

## Troubleshooting

**"no default device"** — Run `wld add <name> <ip>` to add a device first.

**"Device returned HTTP 404"** — Check the device IP is correct and WLED firmware is 0.13+.

**"Not enough space" during OTA** — The ESP8266 ESP02 module has a 2MB flash with ~1MB OTA partition. `wld` automatically prefers `.bin.gz` firmware files (~660KB vs ~895KB) to solve this.

**"/json/live returns 501"** — WLED 0.15+ disables `/json/live` when WebSocket is enabled (default). This is by design; use `wld debug info` for device health stats.

**"Could not detect device platform"** — Use `--platform` flag to specify: `ESP8266`, `ESP32`, `ESP32-S3`, `ESP32-C3-QIO`, `ESP01`, `ESP02`.
