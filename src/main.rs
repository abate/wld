mod config;

#[cfg(feature = "mcp")]
mod mcp;

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::engine::{ArgValueCompleter, CompletionCandidate};
use clap_complete::Shell;
use config::{validate_device_address, Config};
use serde_json::json;
use std::time::Duration;
use wled_json_api_library::structures::state::State;
use wled_json_api_library::wled::Wled;

fn complete_devices(current: &std::ffi::OsStr) -> Vec<CompletionCandidate> {
    let prefix = current.to_str().unwrap_or("");
    Config::load()
        .ok()
        .map(|cfg| {
            cfg.devices
                .keys()
                .filter(|name| name.starts_with(prefix))
                .map(|name| CompletionCandidate::new(name))
                .collect()
        })
        .unwrap_or_default()
}

fn complete_segments(current: &std::ffi::OsStr) -> Vec<CompletionCandidate> {
    let prefix = current.to_str().unwrap_or("");
    let ip = Config::load()
        .ok()
        .and_then(|cfg| cfg.get_device_ip(None).ok());
    let Some(ip) = ip else {
        return vec![];
    };
    get_device_state(&ip)
        .ok()
        .and_then(|state| {
            let segs = state["seg"].as_array()?;
            Some(
                segs.iter()
                    .enumerate()
                    .filter_map(|(i, seg)| {
                        let id = seg["id"].as_u64().unwrap_or(i as u64);
                        let id_str = id.to_string();
                        if !id_str.starts_with(prefix) {
                            return None;
                        }
                        let name = seg["n"].as_str().unwrap_or("");
                        let mut c = CompletionCandidate::new(id_str);
                        if !name.is_empty() {
                            c = c.help(Some(name.to_string().into()));
                        }
                        Some(c)
                    })
                    .collect(),
            )
        })
        .unwrap_or_default()
}

fn complete_presets(current: &std::ffi::OsStr) -> Vec<CompletionCandidate> {
    let prefix = current.to_str().unwrap_or("");
    let ip = Config::load()
        .ok()
        .and_then(|cfg| cfg.get_device_ip(None).ok());
    let Some(ip) = ip else {
        return vec![];
    };
    get_device_presets(&ip)
        .ok()
        .and_then(|presets| {
            let obj = presets.as_object()?;
            Some(
                obj.iter()
                    .filter_map(|(key, val)| {
                        // Skip non-numeric keys (e.g. "0" is the state)
                        let id: u16 = key.parse().ok()?;
                        if id == 0 {
                            return None;
                        }
                        let id_str = id.to_string();
                        if !id_str.starts_with(prefix) {
                            return None;
                        }
                        let name = val["n"].as_str().unwrap_or("");
                        let mut c = CompletionCandidate::new(id_str);
                        if !name.is_empty() {
                            c = c.help(Some(name.to_string().into()));
                        }
                        Some(c)
                    })
                    .collect(),
            )
        })
        .unwrap_or_default()
}

fn parse_color_order(s: &str) -> Result<u8, String> {
    match s.to_uppercase().as_str() {
        "GRB" => Ok(0),
        "RGB" => Ok(1),
        "BRG" => Ok(2),
        "RBG" => Ok(3),
        "BGR" => Ok(4),
        "GBR" => Ok(5),
        _ => Err(format!(
            "Unknown color order '{s}'. Valid options: GRB, RGB, BRG, RBG, BGR, GBR"
        )),
    }
}

fn validate_device_name(name: &str) -> Result<(), Box<dyn std::error::Error>> {
    if name.is_empty() {
        return Err("Device name cannot be empty".into());
    }
    if name.len() > 64 {
        return Err("Device name cannot exceed 64 characters".into());
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(format!(
            "Device name '{name}' contains invalid characters. \
             Use only letters, numbers, hyphens, and underscores"
        )
        .into());
    }
    Ok(())
}

pub fn parse_color(s: &str) -> Result<[u8; 3], Box<dyn std::error::Error>> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() != 6 {
            return Err("Hex color must be 6 characters (e.g. #FF0000)".into());
        }
        let r = u8::from_str_radix(&hex[0..2], 16)?;
        let g = u8::from_str_radix(&hex[2..4], 16)?;
        let b = u8::from_str_radix(&hex[4..6], 16)?;
        return Ok([r, g, b]);
    }
    let parts: Vec<&str> = s.split(',').collect();
    if parts.len() != 3 {
        return Err("Color must be R,G,B (e.g. 255,0,0) or #RRGGBB (e.g. #FF0000)".into());
    }
    let r: u8 = parts[0].trim().parse()?;
    let g: u8 = parts[1].trim().parse()?;
    let b: u8 = parts[2].trim().parse()?;
    Ok([r, g, b])
}

fn led_type_to_code(led_type: &str) -> Result<u8, Box<dyn std::error::Error>> {
    match led_type.to_uppercase().as_str() {
        "WS2812B" | "WS2812" => Ok(22),
        "WS2811" => Ok(16),
        "SK6812" => Ok(30),
        "TM1814" => Ok(24),
        "WS2801" => Ok(50),
        "APA102" => Ok(51),
        "LPD8806" => Ok(52),
        "P9813" => Ok(53),
        _ => led_type.parse::<u8>().map_err(|_| {
            format!(
                "Unknown LED type '{led_type}'. \
                 Supported: WS2812B, SK6812, TM1814, WS2801, APA102, LPD8806, P9813, \
                 or a numeric code"
            )
            .into()
        }),
    }
}

#[derive(Parser)]
#[command(name = "wld")]
#[command(
    about = "Control WLED lights from your terminal",
    long_about = "Control WLED lights from your terminal.\n\n\
        Manage devices, adjust brightness, control segments and presets, \
        update firmware, and expose an MCP server for AI agent integration.\n\n\
        Get started:\n  \
        wld add bedroom 192.168.1.100   Add a device\n  \
        wld on                          Turn on the default device\n  \
        wld brightness 128              Set brightness\n  \
        wld status                      Check all devices\n\n\
        Shell completions (dynamic):\n  \
        source <(COMPLETE=bash wld)     Bash\n  \
        source <(COMPLETE=zsh wld)      Zsh\n  \
        COMPLETE=fish wld | source      Fish"
)]
struct Cli {
    /// Preview changes without applying them
    #[arg(long, global = true)]
    dry_run: bool,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Add a new WLED device
    #[command(long_about = "Add a new WLED device.\n\n\
        Saves a device name and IP address to ~/.wld.toml. The first device \
        added automatically becomes the default.\n\n\
        Examples:\n  \
        wld add bedroom 192.168.1.100\n  \
        wld add kitchen 10.0.0.42")]
    Add {
        /// Name for the device
        name: String,
        /// IP address of the device
        ip: String,
    },
    /// Delete a saved device
    #[command(long_about = "Delete a saved device.\n\n\
        Removes the device from ~/.wld.toml. If the deleted device was the \
        default, the next available device becomes the new default.")]
    Delete {
        /// Name of the device to delete
        name: String,
    },
    /// List all saved devices
    #[command(long_about = "List all saved devices.\n\n\
        Shows all devices in ~/.wld.toml with their IP addresses. \
        The default device is marked with an asterisk (*).")]
    Ls,
    /// Discover WLED devices on the local network via mDNS
    #[command(long_about = "Discover WLED devices on the local network via mDNS.\n\n\
        Sends an mDNS query for _wled._tcp services and lists all \
        responding devices with their name, IP, and version.\n\n\
        Examples:\n  \
        wld discover                  Scan for 5 seconds\n  \
        wld discover --timeout 10    Scan for 10 seconds\n  \
        wld discover --add           Scan and add new devices\n  \
        wld discover --add --timeout 3")]
    Discover {
        /// Scan duration in seconds
        #[arg(long, default_value = "5")]
        timeout: u64,
        /// Automatically add discovered devices to config
        #[arg(long)]
        add: bool,
    },
    /// Set the default device
    #[command(long_about = "Set the default device.\n\n\
        Commands that accept --device will use this device when no \
        device is explicitly specified.\n\n\
        Example:\n  wld set-default bedroom")]
    SetDefault {
        /// Name of the device to set as default
        name: String,
    },
    /// Turn device on
    #[command(long_about = "Turn device on.\n\n\
        Powers on the device, restoring its previous color and effect state.\n\n\
        Examples:\n  \
        wld on                        Turn on default device\n  \
        wld on -d kitchen             Turn on a specific device\n  \
        wld on -d 192.168.1.100       Turn on by IP address")]
    On {
        /// Device name or IP (uses default if not specified)
        #[arg(short, long, add = ArgValueCompleter::new(complete_devices))]
        device: Option<String>,
    },
    /// Turn device off
    #[command(long_about = "Turn device off.\n\n\
        Powers off the device LEDs. The device stays connected to WiFi \
        and can be turned back on remotely.\n\n\
        Examples:\n  \
        wld off                       Turn off default device\n  \
        wld off -d bedroom            Turn off a specific device")]
    Off {
        /// Device name or IP (uses default if not specified)
        #[arg(short, long, add = ArgValueCompleter::new(complete_devices))]
        device: Option<String>,
    },
    /// Start a MCP (Model Context Protocol) server for controlling WLED devices
    #[cfg(feature = "mcp")]
    #[command(long_about = "Start a MCP (Model Context Protocol) server.\n\n\
        Runs an MCP server over stdio that exposes WLED control as tools \
        for AI agents (Claude, etc.). Supports device discovery, power \
        control, brightness, segments, presets, and debug tools.\n\n\
        Typically configured in your MCP client, not run directly.")]
    Mcp,
    /// Set device brightness (0-255)
    #[command(long_about = "Set device brightness.\n\n\
        Accepts a value from 0 (off) to 255 (max), or 0-100 with --percentage.\n\n\
        Examples:\n  \
        wld brightness 128            Set to ~50% brightness\n  \
        wld brightness 75 -p          Set to 75% using percentage\n  \
        wld brightness 255 -d kitchen Full brightness on specific device")]
    Brightness {
        /// Brightness level (0-255, or 0-100 if --percentage is used)
        value: u8,
        /// Device name or IP (uses default if not specified)
        #[arg(short, long, add = ArgValueCompleter::new(complete_devices))]
        device: Option<String>,
        /// Interpret value as a percentage (0-100) instead of 0-255
        #[arg(short, long)]
        percentage: bool,
    },
    /// Check status of all configured devices
    #[command(long_about = "Check status of all configured devices.\n\n\
        Pings each saved device and shows whether it's on/off, its \
        brightness, and current effect. The default device is marked \
        with an asterisk (*).")]
    Status,
    /// Reboot device
    #[command(long_about = "Reboot device.\n\n\
        Sends a reboot command to the device. Useful after configuration \
        changes that require a restart (e.g. mDNS hostname, WiFi settings).\n\n\
        Examples:\n  \
        wld reboot                    Reboot default device\n  \
        wld reboot -d kitchen         Reboot a specific device")]
    Reboot {
        /// Device name or IP (uses default if not specified)
        #[arg(short, long, add = ArgValueCompleter::new(complete_devices))]
        device: Option<String>,
    },
    /// Configure device settings (WiFi, OTA, LEDs)
    #[command(name = "config", long_about = "Configure device settings.\n\n\
        Manage WiFi, OTA (firmware update), and LED hardware settings. \
        You can also export/import the full device configuration as JSON.\n\n\
        Examples:\n  \
        wld config export -o backup.json       Back up device config\n  \
        wld config wifi --ssid MyNetwork --password secret\n  \
        wld config ota --unlock --password wledota\n  \
        wld config led --count 60 --led-type WS2812B")]
    Configure {
        #[command(subcommand)]
        subcommand: ConfigureCommands,
    },
    /// Manage LED segments
    #[command(long_about = "Manage LED segments.\n\n\
        Segments divide your LED strip into independently controlled \
        sections, each with its own color, effect, speed, and brightness.\n\n\
        Examples:\n  \
        wld segment list                       Show all segments\n  \
        wld segment set --id 0 --color #FF0000 Set color to red\n  \
        wld segment set --id 1 --effect 42 --speed 128\n  \
        wld segment export -o segments.json    Back up segments")]
    Segment {
        #[command(subcommand)]
        subcommand: SegmentCommands,
    },
    /// Manage presets
    #[command(long_about = "Manage presets.\n\n\
        Presets save the complete device state (colors, effects, segments) \
        so you can recall them later. IDs range from 1 to 250.\n\n\
        Examples:\n  \
        wld preset list                        Show all presets\n  \
        wld preset save --id 1 --name \"Movie\"  Save current state\n  \
        wld preset load --id 1                 Recall a preset\n  \
        wld preset delete --id 3               Remove a preset")]
    Preset {
        #[command(subcommand)]
        subcommand: PresetCommands,
    },
    /// Debug and inspect device state
    #[command(long_about = "Debug and inspect device state.\n\n\
        Tools for inspecting device internals: hardware info, live LED \
        values, available effects and palettes, and raw JSON dumps.\n\n\
        Examples:\n  \
        wld debug info              Version, WiFi, memory, LEDs\n  \
        wld debug info --json       Raw JSON output\n  \
        wld debug live              Live LED RGB values\n  \
        wld debug effects           List all effects\n  \
        wld debug watch             Continuously poll device stats")]
    Debug {
        #[command(subcommand)]
        subcommand: DebugCommands,
    },
    /// Update device firmware from GitHub releases
    #[command(long_about = "Update device firmware from GitHub releases.\n\n\
        Downloads firmware from the WLED GitHub releases and uploads it \
        to the device via HTTP OTA. Auto-detects platform (ESP8266, ESP32, etc.) \
        and prefers .bin format, falling back to .bin.gz if the device \
        reports \"Not Enough Space\".\n\n\
        OTA must be unlocked before updating. The default OTA password \
        is \"wledota\".\n\n\
        Examples:\n  \
        wld update --check            Check for updates without installing\n  \
        wld update                    Update to latest release\n  \
        wld update --version 0.15.0   Update to a specific version\n  \
        wld update -y                 Skip confirmation prompt")]
    Update {
        /// Target version (e.g. "0.15.0" or "v0.15.0"). Defaults to latest release.
        #[arg(long)]
        version: Option<String>,
        /// Platform override (e.g. ESP8266, ESP32, ESP32-S3, ESP32-C3-QIO, ESP01, ESP02).
        /// Auto-detected from device if not specified.
        #[arg(long)]
        platform: Option<String>,
        /// Device name or IP (uses default if not specified)
        #[arg(short, long, add = ArgValueCompleter::new(complete_devices))]
        device: Option<String>,
        /// Only show version comparison and available firmware without downloading
        #[arg(long)]
        check: bool,
        /// Skip confirmation prompt before downloading and uploading firmware
        #[arg(short, long)]
        yes: bool,
        /// Path to a local firmware .bin or .bin.gz file to upload directly (skips validation)
        #[arg(long, conflicts_with_all = ["version", "platform", "check"])]
        file: Option<String>,
        /// Skip firmware compatibility validation on the device
        #[arg(long)]
        skip_validation: bool,
    },
    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        shell: Shell,
    },
}

#[derive(Subcommand)]
enum ConfigureCommands {
    /// Export full device configuration to a JSON file
    #[command(long_about = "Export full device configuration to a JSON file.\n\n\
        Fetches the complete configuration from the device's /json/cfg \
        endpoint and saves it as pretty-printed JSON. Useful for backups \
        before firmware updates.\n\n\
        Examples:\n  \
        wld config export                  Print to stdout\n  \
        wld config export -o backup.json   Save to file")]
    Export {
        /// Device name or IP (uses default if not specified)
        #[arg(short, long, add = ArgValueCompleter::new(complete_devices))]
        device: Option<String>,
        /// Output file path (prints to stdout if not specified)
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Apply a configuration JSON file to a device
    Apply {
        /// Path to configuration JSON file
        file: String,
        /// Device name or IP (uses default if not specified)
        #[arg(short, long, add = ArgValueCompleter::new(complete_devices))]
        device: Option<String>,
    },
    /// Configure WiFi settings
    Wifi {
        /// WiFi network name (SSID)
        #[arg(long)]
        ssid: Option<String>,
        /// WiFi password (required with --ssid)
        #[arg(long, requires = "ssid")]
        password: Option<String>,
        /// Set the mDNS hostname (e.g. "living-room" → living-room.local)
        #[arg(long)]
        mdns: Option<String>,
        /// WiFi PHY mode: "n" (default, 802.11n) or "g" (802.11g, more stable on ESP8266)
        #[arg(long, value_parser = ["n", "g"])]
        phy_mode: Option<String>,
        /// Device name or IP (uses default if not specified)
        #[arg(short, long, add = ArgValueCompleter::new(complete_devices))]
        device: Option<String>,
    },
    /// Configure OTA (Over-The-Air) update settings
    Ota {
        /// Lock OTA updates to prevent firmware changes
        #[arg(long, conflicts_with = "unlock")]
        lock: bool,
        /// Unlock OTA updates to allow firmware changes (requires --password)
        #[arg(long, conflicts_with = "lock")]
        unlock: bool,
        /// OTA password (required for --unlock when OTA is locked)
        #[arg(long)]
        password: Option<String>,
        /// Device name or IP (uses default if not specified)
        #[arg(short, long, add = ArgValueCompleter::new(complete_devices))]
        device: Option<String>,
    },
    /// Configure LED strip settings
    Led {
        /// Maximum power budget in milliamps (e.g. 850)
        #[arg(long)]
        power: Option<u32>,
        /// Per-LED current draw in milliamps (default: 55 for WS2812B). Lower values allow higher brightness before power-capping.
        #[arg(long)]
        led_ma: Option<u16>,
        /// LED strip type (WS2812B, SK6812, TM1814, WS2801, APA102, LPD8806, P9813, or numeric code)
        #[arg(long, value_name = "TYPE")]
        led_type: Option<String>,
        /// Color order (GRB, RGB, BRG, RBG, BGR, GBR)
        #[arg(long, value_parser = parse_color_order)]
        color_order: Option<u8>,
        /// Number of LEDs in the strip
        #[arg(long)]
        count: Option<u16>,
        /// GPIO pin number for data line
        #[arg(long)]
        pin: Option<u8>,
        /// Device name or IP (uses default if not specified)
        #[arg(short, long, add = ArgValueCompleter::new(complete_devices))]
        device: Option<String>,
    },
    /// Show differences between a local config file and the device config
    Diff {
        /// Path to local configuration JSON file
        file: String,
        /// Device name or IP (uses default if not specified)
        #[arg(short, long, add = ArgValueCompleter::new(complete_devices))]
        device: Option<String>,
    },
}

#[derive(Subcommand)]
enum SegmentCommands {
    /// List all segments on a device
    List {
        /// Device name or IP (uses default if not specified)
        #[arg(short, long, add = ArgValueCompleter::new(complete_devices))]
        device: Option<String>,
    },
    /// Create or modify a segment
    Set {
        /// Segment ID (0-based)
        #[arg(long, add = ArgValueCompleter::new(complete_segments))]
        id: u8,
        /// First LED index (inclusive)
        #[arg(long)]
        start: Option<u16>,
        /// Last LED index (exclusive)
        #[arg(long)]
        stop: Option<u16>,
        /// Primary color (R,G,B or #RRGGBB)
        #[arg(long)]
        color: Option<String>,
        /// Effect ID
        #[arg(long)]
        effect: Option<u8>,
        /// Effect speed (0-255)
        #[arg(long)]
        speed: Option<u8>,
        /// Effect intensity (0-255)
        #[arg(long)]
        intensity: Option<u8>,
        /// Color palette ID
        #[arg(long)]
        palette: Option<u8>,
        /// Segment brightness (0-255)
        #[arg(long)]
        brightness: Option<u8>,
        /// Turn segment on
        #[arg(long, conflicts_with = "off")]
        on: bool,
        /// Turn segment off
        #[arg(long, conflicts_with = "on")]
        off: bool,
        /// Reverse segment direction
        #[arg(long)]
        reverse: Option<bool>,
        /// Device name or IP (uses default if not specified)
        #[arg(short, long, add = ArgValueCompleter::new(complete_devices))]
        device: Option<String>,
    },
    /// Delete a segment
    Delete {
        /// Segment ID to delete
        #[arg(long, add = ArgValueCompleter::new(complete_segments))]
        id: u8,
        /// Device name or IP (uses default if not specified)
        #[arg(short, long, add = ArgValueCompleter::new(complete_devices))]
        device: Option<String>,
    },
    /// Export segments to a JSON file
    Export {
        /// Device name or IP (uses default if not specified)
        #[arg(short, long, add = ArgValueCompleter::new(complete_devices))]
        device: Option<String>,
        /// Output file path (prints to stdout if not specified)
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Import segments from a JSON file
    Import {
        /// Path to JSON file containing segment array
        file: String,
        /// Device name or IP (uses default if not specified)
        #[arg(short, long, add = ArgValueCompleter::new(complete_devices))]
        device: Option<String>,
    },
}

#[derive(Subcommand)]
enum PresetCommands {
    /// List all presets on a device
    List {
        /// Device name or IP (uses default if not specified)
        #[arg(short, long, add = ArgValueCompleter::new(complete_devices))]
        device: Option<String>,
    },
    /// Save current state as a preset
    Save {
        /// Preset ID (1-250)
        #[arg(long, add = ArgValueCompleter::new(complete_presets))]
        id: u16,
        /// Preset name
        #[arg(long)]
        name: Option<String>,
        /// Include brightness in preset
        #[arg(long)]
        include_brightness: bool,
        /// Include segment bounds in preset
        #[arg(long)]
        include_bounds: bool,
        /// Device name or IP (uses default if not specified)
        #[arg(short, long, add = ArgValueCompleter::new(complete_devices))]
        device: Option<String>,
    },
    /// Load a preset
    Load {
        /// Preset ID to load
        #[arg(long, add = ArgValueCompleter::new(complete_presets))]
        id: u16,
        /// Device name or IP (uses default if not specified)
        #[arg(short, long, add = ArgValueCompleter::new(complete_devices))]
        device: Option<String>,
    },
    /// Delete a preset
    Delete {
        /// Preset ID to delete
        #[arg(long, add = ArgValueCompleter::new(complete_presets))]
        id: u16,
        /// Device name or IP (uses default if not specified)
        #[arg(short, long, add = ArgValueCompleter::new(complete_devices))]
        device: Option<String>,
    },
}

#[derive(Subcommand)]
enum DebugCommands {
    /// Show device info (version, memory, uptime, WiFi signal, LED stats)
    Info {
        /// Device name or IP (uses default if not specified)
        #[arg(short, long, add = ArgValueCompleter::new(complete_devices))]
        device: Option<String>,
        /// Output raw JSON instead of formatted text
        #[arg(long)]
        json: bool,
    },
    /// Show live LED color values
    Live {
        /// Device name or IP (uses default if not specified)
        #[arg(short, long, add = ArgValueCompleter::new(complete_devices))]
        device: Option<String>,
        /// Output raw JSON instead of formatted text
        #[arg(long)]
        json: bool,
    },
    /// List available effects on the device
    Effects {
        /// Device name or IP (uses default if not specified)
        #[arg(short, long, add = ArgValueCompleter::new(complete_devices))]
        device: Option<String>,
    },
    /// List available color palettes on the device
    Palettes {
        /// Device name or IP (uses default if not specified)
        #[arg(short, long, add = ArgValueCompleter::new(complete_devices))]
        device: Option<String>,
    },
    /// Show combined state and info (full JSON dump)
    Dump {
        /// Device name or IP (uses default if not specified)
        #[arg(short, long, add = ArgValueCompleter::new(complete_devices))]
        device: Option<String>,
        /// Output file path (prints to stdout if not specified)
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Continuously watch device info stats
    Watch {
        /// Device name or IP (uses default if not specified)
        #[arg(short, long, add = ArgValueCompleter::new(complete_devices))]
        device: Option<String>,
        /// Polling interval in seconds (default: 2)
        #[arg(long, default_value = "2")]
        interval: u64,
    },
}

fn diff_json(local: &serde_json::Value, device: &serde_json::Value, prefix: &str) -> Vec<String> {
    let mut lines = Vec::new();
    match (local, device) {
        (serde_json::Value::Object(lmap), serde_json::Value::Object(dmap)) => {
            // Keys only in local
            for (k, lv) in lmap {
                let key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                if let Some(dv) = dmap.get(k) {
                    let sub = diff_json(lv, dv, &key);
                    lines.extend(sub);
                } else {
                    lines.push(format!("+ {key}: {lv}"));
                }
            }
            // Keys only in device
            for (k, dv) in dmap {
                let key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                if !lmap.contains_key(k) {
                    lines.push(format!("- {key}: {dv}"));
                }
            }
        }
        _ => {
            if local != device {
                let key = if prefix.is_empty() { "(root)" } else { prefix };
                lines.push(format!("~ {key}: {local} -> {device}"));
            }
        }
    }
    lines
}

fn main() {
    clap_complete::CompleteEnv::with_factory(Cli::command).complete();
    if let Err(e) = run() {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

pub fn set_device_brightness(
    device: Option<&str>,
    brightness: u8,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::load()?;
    let ip = config.get_device_ip(device)?;

    let url = reqwest::Url::parse(&format!("http://{ip}"))?;
    let mut wled = Wled::try_from_url(&url)?;

    // Get current state
    wled.get_state_from_wled()?;

    // Update state
    if let Some(state) = &mut wled.state {
        state.bri = Some(brightness);
    } else {
        wled.state = Some(State {
            bri: Some(brightness),
            ..Default::default()
        });
    }

    // Send updated state
    wled.flush_state()?;

    println!("Set brightness to {brightness} for device at {ip}");

    Ok(())
}

pub fn set_device_power(
    device: Option<&str>,
    power_state: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::load()?;
    let ip = config.get_device_ip(device)?;

    let url = reqwest::Url::parse(&format!("http://{ip}"))?;
    let mut wled = Wled::try_from_url(&url)?;

    // Get current state
    wled.get_state_from_wled()?;

    // Update state
    if let Some(state) = &mut wled.state {
        state.on = Some(power_state);
    } else {
        wled.state = Some(State {
            on: Some(power_state),
            ..Default::default()
        });
    }

    // Send updated state
    wled.flush_state()?;

    let action = if power_state { "on" } else { "off" };
    println!("Turned {action} device at {ip}");

    Ok(())
}

#[derive(Debug)]
pub enum DeviceStatus {
    On,
    Off,
    Unreachable,
}

pub fn get_device_status(ip: &str) -> DeviceStatus {
    let url = match reqwest::Url::parse(&format!("http://{ip}")) {
        Ok(u) => u,
        Err(_) => return DeviceStatus::Unreachable,
    };

    let mut wled = match Wled::try_from_url(&url) {
        Ok(w) => w,
        Err(_) => return DeviceStatus::Unreachable,
    };

    // Try to get current state from device
    match wled.get_state_from_wled() {
        Ok(_) => {
            // Check if device is on or off
            if let Some(state) = &wled.state {
                if let Some(on) = state.on {
                    if on {
                        return DeviceStatus::On;
                    } else {
                        return DeviceStatus::Off;
                    }
                }
            }
            // If we can reach the device but can't determine state, assume it's on
            DeviceStatus::On
        }
        Err(_) => DeviceStatus::Unreachable,
    }
}

fn http_client() -> Result<reqwest::blocking::Client, Box<dyn std::error::Error>> {
    Ok(reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?)
}

pub fn post_device_config(
    ip: &str,
    payload: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = http_client()?;
    let body = serde_json::to_string(payload)?;
    let response = client
        .post(format!("http://{ip}/json/cfg"))
        .header("Content-Type", "application/json")
        .body(body)
        .send()?;

    if !response.status().is_success() {
        return Err(format!("Device returned HTTP {}", response.status()).into());
    }

    Ok(())
}

fn wled_led_type_name(type_id: u64) -> &'static str {
    match type_id {
        22 => "WS2812B",
        24 => "WS2811",
        25 => "WS2813",
        26 => "APA106",
        27 => "WS2815",
        28 => "LC8812",
        29 => "WS2805",
        30 => "SK6812",
        31 => "TM1814",
        32 => "UCS8903",
        33 => "APA109",
        34 => "UCS8904",
        40 => "On/Off",
        41 => "PWM White",
        42 => "PWM CCT",
        43 => "PWM RGB",
        44 => "PWM RGBW",
        45 => "PWM RGB+CCT",
        50 => "WS2801",
        51 => "APA102",
        52 => "LPD8806",
        53 => "P9813",
        54 => "LPD6803",
        _ => "Unknown",
    }
}

pub fn get_device_config(ip: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let client = http_client()?;
    let response = client.get(format!("http://{ip}/json/cfg")).send()?;

    if !response.status().is_success() {
        return Err(format!("Device returned HTTP {}", response.status()).into());
    }

    let text = response.text()?;
    let cfg: serde_json::Value = serde_json::from_str(&text)?;
    Ok(cfg)
}

pub fn get_device_state(ip: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let client = http_client()?;
    let response = client.get(format!("http://{ip}/json/state")).send()?;

    if !response.status().is_success() {
        return Err(format!("Device returned HTTP {}", response.status()).into());
    }

    let text = response.text()?;
    let state: serde_json::Value = serde_json::from_str(&text)?;
    Ok(state)
}

pub fn post_device_state(
    ip: &str,
    payload: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = http_client()?;
    let body = serde_json::to_string(payload)?;
    let response = client
        .post(format!("http://{ip}/json/state"))
        .header("Content-Type", "application/json")
        .body(body)
        .send()?;

    if !response.status().is_success() {
        return Err(format!("Device returned HTTP {}", response.status()).into());
    }

    Ok(())
}

pub fn get_device_presets(ip: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let client = http_client()?;
    let response = client.get(format!("http://{ip}/presets.json")).send()?;

    if !response.status().is_success() {
        return Err(format!("Device returned HTTP {}", response.status()).into());
    }

    let text = response.text()?;
    let presets: serde_json::Value = serde_json::from_str(&text)?;
    Ok(presets)
}

pub fn get_device_info(ip: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let client = http_client()?;
    let response = client.get(format!("http://{ip}/json/info")).send()?;

    if !response.status().is_success() {
        return Err(format!("Device returned HTTP {}", response.status()).into());
    }

    let text = response.text()?;
    Ok(serde_json::from_str(&text)?)
}

pub fn get_device_json(ip: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let client = http_client()?;
    let response = client.get(format!("http://{ip}/json")).send()?;

    if !response.status().is_success() {
        return Err(format!("Device returned HTTP {}", response.status()).into());
    }

    let text = response.text()?;
    Ok(serde_json::from_str(&text)?)
}

pub fn get_device_effects(ip: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let client = http_client()?;
    let response = client.get(format!("http://{ip}/json/eff")).send()?;

    if !response.status().is_success() {
        return Err(format!("Device returned HTTP {}", response.status()).into());
    }

    let text = response.text()?;
    let effects: Vec<String> = serde_json::from_str(&text)?;
    Ok(effects)
}

pub fn get_device_palettes(ip: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let client = http_client()?;
    let response = client.get(format!("http://{ip}/json/pal")).send()?;

    if !response.status().is_success() {
        return Err(format!("Device returned HTTP {}", response.status()).into());
    }

    let text = response.text()?;
    let palettes: Vec<String> = serde_json::from_str(&text)?;
    Ok(palettes)
}

pub fn get_device_live(ip: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    use tungstenite::{connect, Message};

    let url = format!("ws://{ip}/ws");
    let (mut socket, _response) = connect(&url)
        .map_err(|e| format!("Failed to connect to WebSocket at {url}: {e}"))?;

    // Request live LED data
    socket.send(Message::Text("{\"lv\":true}".into()))?;

    // Read frames until we get the live data.
    // The first frame after connection is usually the state; the lv response comes after.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        let msg = socket.read()?;
        match msg {
            Message::Text(text) => {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
                    // Live data has a "leds" array
                    if val.get("leds").is_some() {
                        let _ = socket.close(None);
                        return Ok(val);
                    }
                }
            }
            Message::Binary(data) => {
                // WLED sends live LED data as binary frames:
                // byte 0: 0x4c ('L') = live data marker
                // byte 1: mode (1 = RGB, 2 = RGBW)
                // remaining: pixel data (3 bytes per LED for RGB, 4 for RGBW)
                if data.len() >= 2 && data[0] == 0x4c {
                    let mode = data[1];
                    let pixel_data = &data[2..];
                    let bytes_per_led: usize = if mode == 2 { 4 } else { 3 };
                    let num_leds = pixel_data.len() / bytes_per_led;
                    let mut leds = Vec::with_capacity(num_leds);
                    for i in 0..num_leds {
                        let offset = i * bytes_per_led;
                        let r = pixel_data[offset] as u32;
                        let g = pixel_data[offset + 1] as u32;
                        let b = pixel_data[offset + 2] as u32;
                        // Encode as hex color string
                        leds.push(serde_json::json!(format!("{r:02X}{g:02X}{b:02X}")));
                    }
                    let _ = socket.close(None);
                    return Ok(serde_json::json!({
                        "leds": leds,
                        "n": num_leds,
                        "mode": if mode == 2 { "RGBW" } else { "RGB" },
                    }));
                }
            }
            Message::Close(_) => break,
            _ => continue,
        }
    }

    let _ = socket.close(None);
    Err("Timed out waiting for live LED data from WebSocket".into())
}

fn github_client() -> Result<reqwest::blocking::Client, Box<dyn std::error::Error>> {
    Ok(reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(60))
        .user_agent("wld-cli")
        .build()?)
}

fn get_latest_wled_release() -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let client = github_client()?;
    let response = client
        .get("https://api.github.com/repos/wled/WLED/releases/latest")
        .send()?;

    if !response.status().is_success() {
        return Err(format!(
            "Failed to fetch latest release from GitHub (HTTP {})",
            response.status()
        )
        .into());
    }

    Ok(serde_json::from_str(&response.text()?)?)
}

fn get_wled_release_by_tag(tag: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let client = github_client()?;

    // Try with "v" prefix first, then without
    let tags_to_try = if tag.starts_with('v') {
        vec![tag.to_string(), tag[1..].to_string()]
    } else {
        vec![format!("v{tag}"), tag.to_string()]
    };

    for t in &tags_to_try {
        let response = client
            .get(format!(
                "https://api.github.com/repos/wled/WLED/releases/tags/{t}"
            ))
            .send()?;

        if response.status().is_success() {
            return Ok(serde_json::from_str(&response.text()?)?);
        }
    }

    Err(format!("Release '{tag}' not found on GitHub").into())
}

fn find_firmware_asset(
    release: &serde_json::Value,
    platform: &str,
    prefer_gz: bool,
) -> Result<(String, String), Box<dyn std::error::Error>> {
    let assets = release["assets"]
        .as_array()
        .ok_or("No assets found in release")?;

    let version = release["tag_name"]
        .as_str()
        .unwrap_or("unknown")
        .trim_start_matches('v');

    let platform_upper = platform.to_uppercase();
    let candidates: Vec<String> = if prefer_gz {
        vec![
            format!("WLED_{version}_{platform_upper}.bin.gz"),
            format!("WLED_{version}_{platform}.bin.gz"),
            format!("WLED_{version}_{platform_upper}.bin"),
            format!("WLED_{version}_{platform}.bin"),
        ]
    } else {
        vec![
            format!("WLED_{version}_{platform_upper}.bin"),
            format!("WLED_{version}_{platform}.bin"),
            format!("WLED_{version}_{platform_upper}.bin.gz"),
            format!("WLED_{version}_{platform}.bin.gz"),
        ]
    };

    // Try exact matches — iterate candidates first to preserve .bin.gz preference
    for candidate in &candidates {
        for asset in assets {
            let name = asset["name"].as_str().unwrap_or("");
            if name == candidate {
                let url = asset["browser_download_url"]
                    .as_str()
                    .ok_or("Asset missing download URL")?;
                return Ok((name.to_string(), url.to_string()));
            }
        }
    }

    // Try substring match on platform
    for asset in assets {
        let name = asset["name"].as_str().unwrap_or("");
        if name.ends_with(".bin")
            && name
                .to_uppercase()
                .contains(&platform_upper)
        {
            let url = asset["browser_download_url"]
                .as_str()
                .ok_or("Asset missing download URL")?;
            return Ok((name.to_string(), url.to_string()));
        }
    }

    // List available assets for the error message
    let available: Vec<&str> = assets
        .iter()
        .filter_map(|a| a["name"].as_str())
        .filter(|n| n.ends_with(".bin") || n.ends_with(".bin.gz"))
        .collect();

    Err(format!(
        "No firmware binary found for platform '{platform}'. Available binaries:\n  {}",
        available.join("\n  ")
    )
    .into())
}

fn download_firmware(url: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let client = github_client()?;
    let response = client.get(url).send()?;

    if !response.status().is_success() {
        return Err(format!("Failed to download firmware (HTTP {})", response.status()).into());
    }

    Ok(response.bytes()?.to_vec())
}

/// Extract meaningful text from WLED HTML responses.
fn extract_wled_message(html: &str) -> String {
    // Remove script and style blocks entirely
    let mut result = html.to_string();
    for tag in &["script", "style"] {
        while let Some(start) = result.find(&format!("<{tag}")) {
            if let Some(end) = result[start..].find(&format!("</{tag}>")) {
                result.replace_range(start..start + end + tag.len() + 3, "");
            } else {
                break;
            }
        }
    }
    // Convert block elements to newlines, strip remaining tags
    let clean = result.replace("<br>", "\n").replace("</p>", "\n");
    let mut inside_tag = false;
    let stripped: String = clean
        .chars()
        .filter(|c| {
            if *c == '<' {
                inside_tag = true;
                false
            } else if *c == '>' {
                inside_tag = false;
                false
            } else {
                !inside_tag
            }
        })
        .collect();
    // Collapse whitespace and return non-empty lines
    stripped
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

struct UploadError {
    message: String,
    not_enough_space: bool,
}

impl std::fmt::Display for UploadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

fn upload_firmware(ip: &str, firmware: Vec<u8>, skip_validation: bool) -> Result<(), UploadError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| UploadError {
            message: e.to_string(),
            not_enough_space: false,
        })?;

    let part = reqwest::blocking::multipart::Part::bytes(firmware)
        .file_name("firmware.bin")
        .mime_str("application/octet-stream")
        .map_err(|e| UploadError {
            message: e.to_string(),
            not_enough_space: false,
        })?;

    let mut form = reqwest::blocking::multipart::Form::new().part("update", part);
    if skip_validation {
        form = form.text("skipValidation", "1");
    }

    let response = client
        .post(format!("http://{ip}/update"))
        .multipart(form)
        .send()
        .map_err(|e| UploadError {
            message: format!("Firmware upload failed: {e}"),
            not_enough_space: false,
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        let message = extract_wled_message(&body);
        let not_enough_space = message.to_lowercase().contains("not enough space");
        let detail = if message.is_empty() {
            String::new()
        } else {
            format!("\nDevice response: {message}")
        };
        return Err(UploadError {
            message: format!("Firmware upload failed (HTTP {status}){detail}"),
            not_enough_space,
        });
    }

    Ok(())
}

fn arch_to_default_platform(arch: &str) -> &str {
    match arch.to_lowercase().as_str() {
        "esp8266" => "ESP8266",
        "esp32" => "ESP32",
        "esp32s3" | "esp32-s3" => "ESP32-S3",
        "esp32c3" | "esp32-c3" => "ESP32-C3-QIO",
        _ => arch,
    }
}

fn resolve_device_ip(device: Option<&str>) -> Result<String, Box<dyn std::error::Error>> {
    let config = Config::load()?;
    Ok(config.get_device_ip(device)?)
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let dry_run = cli.dry_run;

    match cli.command {
        Commands::Add { name, ip } => {
            validate_device_name(&name)?;
            validate_device_address(&ip)?;
            if dry_run {
                println!("Would add device '{name}' with IP {ip}");
                return Ok(());
            }
            let mut config = Config::load()?;
            config.add_device(name.clone(), ip.clone());
            config.save()?;
            println!("Added device '{name}' with IP {ip}");

            if config.devices.len() == 1 {
                println!("Set '{name}' as the default device");
            }
        }
        Commands::Delete { name } => {
            if dry_run {
                let config = Config::load()?;
                if !config.devices.contains_key(&name) {
                    return Err(format!("Device '{name}' not found").into());
                }
                println!("Would delete device '{name}'");
                return Ok(());
            }
            let mut config = Config::load()?;
            config.remove_device(&name)?;
            config.save()?;
            println!("Deleted device '{name}'");
        }
        Commands::Ls => {
            let config = Config::load()?;

            if config.devices.is_empty() {
                println!("No devices saved");
                return Ok(());
            }

            println!("Saved devices:");
            for (name, ip) in &config.devices {
                let default_marker = if config.default_device.as_ref() == Some(name) {
                    " (default)"
                } else {
                    ""
                };
                println!("  {name} - {ip}{default_marker}");
            }
        }
        Commands::Discover { timeout, add } => {
            use mdns_sd::{ServiceDaemon, ServiceEvent};
            use std::collections::BTreeMap;

            println!("Scanning for WLED devices ({timeout}s)...");
            let mdns = ServiceDaemon::new()
                .map_err(|e| format!("Failed to start mDNS: {e}"))?;
            let receiver = mdns
                .browse("_wled._tcp.local.")
                .map_err(|e| format!("Failed to browse mDNS: {e}"))?;

            // Collect discovered devices: name -> (ip, info_text)
            let mut found: BTreeMap<String, (String, String)> = BTreeMap::new();
            let start = std::time::Instant::now();
            let duration = Duration::from_secs(timeout);

            loop {
                let remaining = duration.saturating_sub(start.elapsed());
                if remaining.is_zero() {
                    break;
                }
                match receiver.recv_timeout(remaining) {
                    Ok(ServiceEvent::ServiceResolved(info)) => {
                        let ip = info
                            .get_addresses_v4()
                            .iter()
                            .next()
                            .map(|a| a.to_string())
                            .unwrap_or_default();
                        if ip.is_empty() {
                            continue;
                        }

                        // Extract device name from mDNS instance name
                        // fullname is like "WLED-Stairs._wled._tcp.local."
                        let device_name = info
                            .get_fullname()
                            .split("._wled._tcp")
                            .next()
                            .unwrap_or("unknown")
                            .to_lowercase()
                            .replace(' ', "-");

                        // Try to get version from device
                        let version = get_device_info(&ip)
                            .ok()
                            .and_then(|i| i["ver"].as_str().map(String::from))
                            .unwrap_or_else(|| "?".into());

                        println!("  Found: {device_name} at {ip} (WLED {version})");
                        found.insert(device_name, (ip, version));
                    }
                    Ok(_) => {} // Ignore other events
                    Err(_) => break,
                }
            }

            let _ = mdns.shutdown();

            if found.is_empty() {
                println!("\nNo WLED devices found on the network.");
                return Ok(());
            }

            println!("\nDiscovered {} device(s):", found.len());
            let config = Config::load().unwrap_or_else(|_| Config {
                devices: std::collections::HashMap::new(),
                default_device: None,
            });
            let mut added = 0;

            for (name, (ip, version)) in &found {
                let already_saved = config.devices.values().any(|v| v == ip);
                let status = if already_saved { " (already saved)" } else { "" };
                println!("  {name} — {ip} (WLED {version}){status}");

                if add && !already_saved {
                    if dry_run {
                        println!("    Would add as '{name}'");
                    } else {
                        let mut cfg = Config::load().unwrap_or_else(|_| Config {
                            devices: std::collections::HashMap::new(),
                            default_device: None,
                        });
                        cfg.add_device(name.clone(), ip.clone());
                        cfg.save()?;
                        println!("    Added as '{name}'");
                        added += 1;
                    }
                }
            }

            if add && !dry_run && added > 0 {
                println!("\nAdded {added} new device(s).");
            }
        }
        Commands::SetDefault { name } => {
            if dry_run {
                let config = Config::load()?;
                if !config.devices.contains_key(&name) {
                    return Err(format!("Device '{name}' not found").into());
                }
                println!("Would set '{name}' as the default device");
                return Ok(());
            }
            let mut config = Config::load()?;
            config.set_default(&name)?;
            config.save()?;
            println!("Set '{name}' as the default device");
        }
        Commands::On { device } => {
            if dry_run {
                let ip = resolve_device_ip(device.as_deref())?;
                println!("Would turn on device at {ip}");
                return Ok(());
            }
            set_device_power(device.as_deref(), true)?;
        }
        Commands::Off { device } => {
            if dry_run {
                let ip = resolve_device_ip(device.as_deref())?;
                println!("Would turn off device at {ip}");
                return Ok(());
            }
            set_device_power(device.as_deref(), false)?;
        }
        #[cfg(feature = "mcp")]
        Commands::Mcp => {
            mcp::handle_mcp_command()?;
        }
        Commands::Brightness {
            value,
            device,
            percentage,
        } => {
            let brightness = if percentage {
                if value > 100 {
                    return Err(
                        format!("Percentage must be between 0 and 100, got {value}").into()
                    );
                }
                ((value as u16 * 255) / 100) as u8
            } else {
                value
            };
            if dry_run {
                let ip = resolve_device_ip(device.as_deref())?;
                println!("Would set brightness to {brightness} on device at {ip}");
                return Ok(());
            }
            set_device_brightness(device.as_deref(), brightness)?;
        }
        Commands::Status => {
            let config = Config::load()?;

            if config.devices.is_empty() {
                println!("No devices saved");
                return Ok(());
            }

            println!("Checking status of all devices...\n");

            let mut all_reachable = true;

            for (name, ip) in &config.devices {
                let default_marker = if config.default_device.as_ref() == Some(name) {
                    " (default)"
                } else {
                    ""
                };

                print!("  {name} ({ip}){default_marker}: ");

                match get_device_status(ip) {
                    DeviceStatus::On => {
                        println!("ON");
                    }
                    DeviceStatus::Off => {
                        println!("OFF");
                    }
                    DeviceStatus::Unreachable => {
                        println!("UNREACHABLE");
                        all_reachable = false;
                    }
                }
            }

            if !all_reachable {
                std::process::exit(1);
            }
        }
        Commands::Reboot { device } => {
            let ip = resolve_device_ip(device.as_deref())?;
            if dry_run {
                println!("Would reboot device at {ip}");
                return Ok(());
            }
            let client = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()?;
            client
                .get(format!("http://{ip}/reset"))
                .send()
                .map_err(|e| format!("Failed to reboot device: {e}"))?;
            println!("Reboot command sent to device at {ip}");
        }
        Commands::Segment { subcommand } => match subcommand {
            SegmentCommands::List { device } => {
                let ip = resolve_device_ip(device.as_deref())?;
                let state = get_device_state(&ip)?;

                let segments = state["seg"]
                    .as_array()
                    .ok_or("No segments found in device state")?;

                if segments.is_empty() {
                    println!("No segments configured");
                } else {
                    println!("Segments on device at {ip}:\n");
                    for seg in segments {
                        let id = seg["id"].as_u64().unwrap_or(0);
                        let start = seg["start"].as_u64().unwrap_or(0);
                        let stop = seg["stop"].as_u64().unwrap_or(0);
                        let on = seg["on"].as_bool().unwrap_or(false);
                        let bri = seg["bri"].as_u64().unwrap_or(0);
                        let fx = seg["fx"].as_u64().unwrap_or(0);
                        let sx = seg["sx"].as_u64().unwrap_or(0);
                        let ix = seg["ix"].as_u64().unwrap_or(0);
                        let pal = seg["pal"].as_u64().unwrap_or(0);
                        let rev = seg["rev"].as_bool().unwrap_or(false);

                        let status = if on { "ON" } else { "OFF" };
                        let name = seg["n"]
                            .as_str()
                            .filter(|n| !n.is_empty())
                            .map(|n| format!(" \"{n}\""))
                            .unwrap_or_default();
                        println!("  Segment {id}{name}: LEDs {start}-{stop} ({status})");
                        println!(
                            "    brightness={bri} effect={fx} speed={sx} intensity={ix} palette={pal} reverse={rev}"
                        );

                        if let Some(colors) = seg["col"].as_array() {
                            let color_strs: Vec<String> = colors
                                .iter()
                                .filter_map(|c| {
                                    c.as_array().map(|rgb| {
                                        let vals: Vec<u64> =
                                            rgb.iter().filter_map(|v| v.as_u64()).collect();
                                        format!(
                                            "({},{},{})",
                                            vals.first().unwrap_or(&0),
                                            vals.get(1).unwrap_or(&0),
                                            vals.get(2).unwrap_or(&0)
                                        )
                                    })
                                })
                                .collect();
                            if !color_strs.is_empty() {
                                println!("    colors: {}", color_strs.join(" "));
                            }
                        }
                    }
                }
            }
            SegmentCommands::Set {
                id,
                start,
                stop,
                color,
                effect,
                speed,
                intensity,
                palette,
                brightness,
                on,
                off,
                reverse,
                device,
            } => {
                let parsed_color = color.as_ref().map(|c| parse_color(c)).transpose()?;
                let ip = resolve_device_ip(device.as_deref())?;

                let mut seg = json!({"id": id});
                if let Some(s) = start {
                    seg["start"] = json!(s);
                }
                if let Some(s) = stop {
                    seg["stop"] = json!(s);
                }
                if let Some(rgb) = parsed_color {
                    seg["col"] = json!([[rgb[0], rgb[1], rgb[2]]]);
                }
                if let Some(fx) = effect {
                    seg["fx"] = json!(fx);
                }
                if let Some(sx) = speed {
                    seg["sx"] = json!(sx);
                }
                if let Some(ix) = intensity {
                    seg["ix"] = json!(ix);
                }
                if let Some(pal) = palette {
                    seg["pal"] = json!(pal);
                }
                if let Some(bri) = brightness {
                    seg["bri"] = json!(bri);
                }
                if on {
                    seg["on"] = json!(true);
                }
                if off {
                    seg["on"] = json!(false);
                }
                if let Some(rev) = reverse {
                    seg["rev"] = json!(rev);
                }

                let payload = json!({"seg": [seg]});

                if dry_run {
                    println!("Would set segment {id} on device at {ip}:");
                    println!("{}", serde_json::to_string_pretty(&payload)?);
                } else {
                    post_device_state(&ip, &payload)?;
                    println!("Segment {id} updated on device at {ip}");
                }
            }
            SegmentCommands::Delete { id, device } => {
                let ip = resolve_device_ip(device.as_deref())?;
                let payload = json!({"seg": [{"id": id, "stop": 0}]});

                if dry_run {
                    println!("Would delete segment {id} on device at {ip}");
                } else {
                    post_device_state(&ip, &payload)?;
                    println!("Segment {id} deleted on device at {ip}");
                }
            }
            SegmentCommands::Export { device, output } => {
                let ip = resolve_device_ip(device.as_deref())?;
                let state = get_device_state(&ip)?;
                let segments = state
                    .get("seg")
                    .cloned()
                    .unwrap_or(serde_json::Value::Array(vec![]));
                let pretty = serde_json::to_string_pretty(&segments)?;

                if let Some(path) = output {
                    std::fs::write(&path, format!("{pretty}\n"))?;
                    println!("Segments exported to {path}");
                } else {
                    println!("{pretty}");
                }
            }
            SegmentCommands::Import { file, device } => {
                let ip = resolve_device_ip(device.as_deref())?;
                let content = std::fs::read_to_string(&file)?;
                let segments: serde_json::Value = serde_json::from_str(&content)?;
                let payload = json!({"seg": segments});

                if dry_run {
                    println!("Would import segments to device at {ip}:");
                    println!("{}", serde_json::to_string_pretty(&payload)?);
                } else {
                    post_device_state(&ip, &payload)?;
                    println!("Segments imported to device at {ip}");
                }
            }
        },
        Commands::Preset { subcommand } => match subcommand {
            PresetCommands::List { device } => {
                let ip = resolve_device_ip(device.as_deref())?;
                let presets = get_device_presets(&ip)?;

                let obj = presets
                    .as_object()
                    .ok_or("Invalid presets response from device")?;

                let mut found = false;
                for (key, value) in obj {
                    // Skip non-numeric keys (metadata)
                    if key.parse::<u16>().is_err() {
                        continue;
                    }
                    found = true;
                    let name = value["n"].as_str().unwrap_or("(unnamed)");
                    let on = value["on"].as_bool();
                    let bri = value["bri"].as_u64();

                    let mut details = Vec::new();
                    if let Some(on) = on {
                        details.push(if on {
                            "on".to_string()
                        } else {
                            "off".to_string()
                        });
                    }
                    if let Some(bri) = bri {
                        details.push(format!("brightness={bri}"));
                    }
                    let detail_str = if details.is_empty() {
                        String::new()
                    } else {
                        format!(" ({})", details.join(", "))
                    };
                    println!("  Preset {key}: {name}{detail_str}");
                }

                if !found {
                    println!("No presets saved on device at {ip}");
                }
            }
            PresetCommands::Save {
                id,
                name,
                include_brightness,
                include_bounds,
                device,
            } => {
                if id == 0 || id > 250 {
                    return Err("Preset ID must be between 1 and 250".into());
                }
                let ip = resolve_device_ip(device.as_deref())?;

                let mut payload = json!({"psave": id});
                if let Some(ref n) = name {
                    payload["n"] = json!(n);
                }
                if include_brightness {
                    payload["ib"] = json!(true);
                }
                if include_bounds {
                    payload["sb"] = json!(true);
                }

                if dry_run {
                    let display_name = name.as_deref().unwrap_or("(unnamed)");
                    println!("Would save current state as preset {id} ({display_name}) on device at {ip}");
                } else {
                    post_device_state(&ip, &payload)?;
                    let display_name = name.as_deref().unwrap_or("(unnamed)");
                    println!(
                        "Saved current state as preset {id} ({display_name}) on device at {ip}"
                    );
                }
            }
            PresetCommands::Load { id, device } => {
                let ip = resolve_device_ip(device.as_deref())?;
                let payload = json!({"ps": id});

                if dry_run {
                    println!("Would load preset {id} on device at {ip}");
                } else {
                    post_device_state(&ip, &payload)?;
                    println!("Loaded preset {id} on device at {ip}");
                }
            }
            PresetCommands::Delete { id, device } => {
                let ip = resolve_device_ip(device.as_deref())?;
                let payload = json!({"pdel": id});

                if dry_run {
                    println!("Would delete preset {id} on device at {ip}");
                } else {
                    post_device_state(&ip, &payload)?;
                    println!("Deleted preset {id} on device at {ip}");
                }
            }
        },
        Commands::Debug { subcommand } => match subcommand {
            DebugCommands::Info { device, json } => {
                let ip = resolve_device_ip(device.as_deref())?;
                let info = get_device_info(&ip)?;

                if json {
                    println!("{}", serde_json::to_string_pretty(&info)?);
                } else {
                    println!("Device info for {ip}:\n");
                    if let Some(name) = info["name"].as_str() {
                        println!("  Name:       {name}");
                    }
                    if let Some(ver) = info["ver"].as_str() {
                        println!("  Version:    {ver}");
                    }
                    if let Some(vid) = info["vid"].as_u64() {
                        println!("  Build ID:   {vid}");
                    }
                    if let Some(mac) = info["mac"].as_str() {
                        println!("  MAC:        {mac}");
                    }
                    if let Some(uptime) = info["uptime"].as_u64() {
                        let hours = uptime / 3600;
                        let mins = (uptime % 3600) / 60;
                        let secs = uptime % 60;
                        println!("  Uptime:     {hours}h {mins}m {secs}s");
                    }
                    if let Some(heap) = info["freeheap"].as_u64() {
                        let warning = if heap < 10000 { " (LOW!)" } else { "" };
                        println!("  Free heap:  {heap} bytes{warning}");
                    }

                    // Fetch config once for WiFi PHY mode and LED type
                    let device_cfg = get_device_config(&ip).ok();

                    // WiFi info
                    if let Some(wifi) = info.get("wifi") {
                        println!();
                        if let Some(signal) = wifi["signal"].as_i64() {
                            let quality = match signal {
                                80..=100 => "excellent",
                                60..=79 => "good",
                                40..=59 => "fair",
                                _ => "poor",
                            };
                            println!("  WiFi:       {signal}% ({quality})");
                        }
                        if let Some(channel) = wifi["channel"].as_u64() {
                            println!("  Channel:    {channel}");
                        }
                        if let Some(bssid) = wifi["bssid"].as_str() {
                            println!("  BSSID:      {bssid}");
                        }
                        if let Some(ref cfg) = device_cfg {
                            let phy_mode = if cfg["wifi"]["phy"].as_bool().unwrap_or(false) {
                                "802.11g"
                            } else {
                                "802.11n"
                            };
                            println!("  PHY mode:   {phy_mode}");
                        }
                    }

                    // LED info
                    if let Some(leds) = info.get("leds") {
                        println!();
                        if let Some(count) = leds["count"].as_u64() {
                            let led_type = device_cfg
                                .as_ref()
                                .and_then(|cfg| {
                                    cfg["hw"]["led"]["ins"]
                                        .as_array()?
                                        .first()?
                                        .get("type")?
                                        .as_u64()
                                })
                                .map(wled_led_type_name);
                            if let Some(lt) = led_type {
                                println!("  LEDs:       {count} ({lt})");
                            } else {
                                println!("  LEDs:       {count}");
                            }
                        }
                        if let Some(fps) = leds["fps"].as_u64() {
                            println!("  FPS:        {fps}");
                        }
                        if let Some(pwr) = leds["pwr"].as_u64() {
                            if let Some(maxpwr) = leds["maxpwr"].as_u64() {
                                if maxpwr > 0 {
                                    println!(
                                        "  Power:      {pwr} mA estimated, capped to {maxpwr} mA limit",
                                    );
                                } else {
                                    println!("  Power:      {pwr} mA estimated (no limit set)");
                                }
                            } else {
                                println!("  Power:      {pwr} mA estimated");
                            }
                        }
                        if let Some(maxseg) = leds["maxseg"].as_u64() {
                            println!("  Max segs:   {maxseg}");
                        }
                    }

                    // Counts
                    if let Some(fxcount) = info["fxcount"].as_u64() {
                        println!();
                        println!("  Effects:    {fxcount}");
                    }
                    if let Some(palcount) = info["palcount"].as_u64() {
                        println!("  Palettes:   {palcount}");
                    }

                    // Live/WS status
                    if let Some(live) = info["live"].as_bool() {
                        if live {
                            println!();
                            println!("  Realtime:   active");
                        }
                    }
                    if let Some(ws) = info["ws"].as_i64() {
                        if ws > 0 {
                            println!("  WebSocket:  {ws} client(s)");
                        }
                    }
                }
            }
            DebugCommands::Live { device, json } => {
                let ip = resolve_device_ip(device.as_deref())?;
                let live = get_device_live(&ip)?;

                if json {
                    println!("{}", serde_json::to_string_pretty(&live)?);
                } else {
                    println!("Live LED values for device at {ip}:\n");

                    if let Some(leds) = live["leds"].as_array() {
                        for (i, led) in leds.iter().enumerate() {
                            if let Some(rgb) = led.as_array() {
                                let r = rgb.first().and_then(|v| v.as_u64()).unwrap_or(0);
                                let g = rgb.get(1).and_then(|v| v.as_u64()).unwrap_or(0);
                                let b = rgb.get(2).and_then(|v| v.as_u64()).unwrap_or(0);
                                println!("  LED {i:>4}: ({r:>3},{g:>3},{b:>3})");
                            } else if let Some(hex) = led.as_str() {
                                if hex.len() == 6 {
                                    if let (Ok(r), Ok(g), Ok(b)) = (
                                        u8::from_str_radix(&hex[0..2], 16),
                                        u8::from_str_radix(&hex[2..4], 16),
                                        u8::from_str_radix(&hex[4..6], 16),
                                    ) {
                                        println!("  LED {i:>4}: ({r:>3},{g:>3},{b:>3})");
                                    } else {
                                        println!("  LED {i:>4}: #{hex}");
                                    }
                                } else {
                                    println!("  LED {i:>4}: #{hex}");
                                }
                            } else if let Some(val) = led.as_u64() {
                                // WLED may return 32-bit integers (BGRAW format)
                                let r = (val >> 16) & 0xFF;
                                let g = (val >> 8) & 0xFF;
                                let b = val & 0xFF;
                                println!("  LED {i:>4}: ({r:>3},{g:>3},{b:>3})");
                            }
                        }
                        println!("\n  Total: {} LEDs", leds.len());
                    } else {
                        // Some firmware returns flat format
                        println!("{}", serde_json::to_string_pretty(&live)?);
                    }
                }
            }
            DebugCommands::Effects { device } => {
                let ip = resolve_device_ip(device.as_deref())?;
                let effects = get_device_effects(&ip)?;

                println!("Available effects ({} total):\n", effects.len());
                for (id, name) in effects.iter().enumerate() {
                    println!("  {id:>3}: {name}");
                }
            }
            DebugCommands::Palettes { device } => {
                let ip = resolve_device_ip(device.as_deref())?;
                let palettes = get_device_palettes(&ip)?;

                println!("Available palettes ({} total):\n", palettes.len());
                for (id, name) in palettes.iter().enumerate() {
                    println!("  {id:>3}: {name}");
                }
            }
            DebugCommands::Dump { device, output } => {
                let ip = resolve_device_ip(device.as_deref())?;
                let full = get_device_json(&ip)?;
                let pretty = serde_json::to_string_pretty(&full)?;

                if let Some(path) = output {
                    std::fs::write(&path, format!("{pretty}\n"))?;
                    println!("Full state dump written to {path}");
                } else {
                    println!("{pretty}");
                }
            }
            DebugCommands::Watch { device, interval } => {
                let ip = resolve_device_ip(device.as_deref())?;
                loop {
                    match get_device_info(&ip) {
                        Ok(info) => {
                            let uptime = info["uptime"].as_u64().unwrap_or(0);
                            let hours = uptime / 3600;
                            let mins = (uptime % 3600) / 60;
                            let secs = uptime % 60;
                            let heap = info["freeheap"].as_u64().unwrap_or(0);
                            let fps = info["leds"]["fps"].as_u64().unwrap_or(0);
                            let pwr = info["leds"]["pwr"].as_u64().unwrap_or(0);
                            let wifi = info["wifi"]["signal"].as_i64().unwrap_or(0);
                            println!(
                                "[{ip}] uptime={hours}h{mins}m{secs}s heap={heap}B fps={fps} power={pwr}mA wifi={wifi}%"
                            );
                        }
                        Err(e) => {
                            println!("[{ip}] ERROR: {e}");
                        }
                    }
                    std::thread::sleep(Duration::from_secs(interval));
                }
            }
        },
        Commands::Configure { subcommand } => match subcommand {
            ConfigureCommands::Export { device, output } => {
                let ip = resolve_device_ip(device.as_deref())?;
                let device_config = get_device_config(&ip)?;
                let pretty = serde_json::to_string_pretty(&device_config)?;

                if let Some(path) = output {
                    std::fs::write(&path, format!("{pretty}\n"))?;
                    println!("Configuration exported to {path}");
                } else {
                    println!("{pretty}");
                }
            }
            ConfigureCommands::Apply { file, device } => {
                let ip = resolve_device_ip(device.as_deref())?;
                let content = std::fs::read_to_string(&file)?;
                let payload: serde_json::Value = serde_json::from_str(&content)?;

                if dry_run {
                    println!("Would apply configuration to device at {ip}:");
                    println!("{}", serde_json::to_string_pretty(&payload)?);
                } else {
                    post_device_config(&ip, &payload)?;
                    println!("Configuration applied to device at {ip}");
                }
            }
            ConfigureCommands::Wifi {
                ssid,
                password,
                mdns,
                phy_mode,
                device,
            } => {
                if ssid.is_none() && mdns.is_none() && phy_mode.is_none() {
                    return Err(
                        "At least one of --ssid, --mdns, or --phy-mode must be specified".into(),
                    );
                }
                let ip = resolve_device_ip(device.as_deref())?;
                let mut payload = json!({});
                let mut display = json!({});

                if let Some(ref ssid) = ssid {
                    let pwd = password.as_deref().unwrap_or("");
                    payload["nw"] = json!({"ins": [{"ssid": ssid, "psk": pwd}]});
                    display["nw"] = json!({"ins": [{"ssid": ssid, "psk": "***"}]});
                }
                if let Some(ref name) = mdns {
                    payload["id"] = json!({"mdns": name});
                    display["id"] = json!({"mdns": name});
                }
                if let Some(ref mode) = phy_mode {
                    let force_g = mode == "g";
                    payload["wifi"] = json!({"phy": force_g});
                    display["wifi"] = json!({"phy": if force_g { "802.11g" } else { "802.11n" }});
                }

                if dry_run {
                    println!("Would configure on device at {ip}:");
                    println!("{}", serde_json::to_string_pretty(&display)?);
                } else {
                    post_device_config(&ip, &payload)?;
                    if ssid.is_some() {
                        println!("WiFi configured on device at {ip}");
                    }
                    if let Some(ref name) = mdns {
                        println!("mDNS hostname set to {name}.local on device at {ip}");
                    }
                    if let Some(ref mode) = phy_mode {
                        let label = if mode == "g" { "802.11g" } else { "802.11n" };
                        println!("WiFi PHY mode set to {label} on device at {ip}");
                    }
                    if ssid.is_some() || phy_mode.is_some() {
                        println!("Note: Device may restart to apply changes");
                    }
                }
            }
            ConfigureCommands::Ota {
                lock,
                unlock,
                password,
                device,
            } => {
                if !lock && !unlock && password.is_none() {
                    return Err(
                        "At least one option required: --lock, --unlock, or --password".into(),
                    );
                }
                let ip = resolve_device_ip(device.as_deref())?;

                // When unlocking a locked device, WLED requires the current OTA
                // password in the request to verify the unlock is authorized.
                if unlock && password.is_none() {
                    let cfg = get_device_config(&ip)?;
                    if cfg["ota"]["lock"].as_bool().unwrap_or(false) {
                        return Err(
                            "OTA is locked. Provide the current OTA password with --password to unlock.\n  \
                             The default WLED OTA password is \"wledota\"."
                                .into(),
                        );
                    }
                }

                let mut ota = json!({});
                if lock {
                    ota["lock"] = json!(true);
                }
                if unlock {
                    ota["lock"] = json!(false);
                }
                if let Some(ref psk) = password {
                    ota["psk"] = json!(psk);
                }
                let payload = json!({"ota": ota});

                if dry_run {
                    let mut display_ota = ota.clone();
                    if password.is_some() {
                        display_ota["psk"] = json!("***");
                    }
                    println!("Would configure OTA on device at {ip}:");
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&json!({"ota": display_ota}))?
                    );
                } else {
                    post_device_config(&ip, &payload)?;
                    let mut actions = Vec::new();
                    if lock {
                        actions.push("locked");
                    }
                    if unlock {
                        actions.push("unlocked");
                    }
                    if password.is_some() {
                        actions.push("password updated");
                    }
                    println!(
                        "OTA settings updated on device at {ip}: {}",
                        actions.join(", ")
                    );
                }
            }
            ConfigureCommands::Led {
                power,
                led_ma,
                led_type,
                color_order,
                count,
                pin,
                device,
            } => {
                if power.is_none()
                    && led_ma.is_none()
                    && led_type.is_none()
                    && color_order.is_none()
                    && count.is_none()
                    && pin.is_none()
                {
                    return Err(
                        "At least one option required: --power, --led-ma, --led-type, --color-order, --count, or --pin"
                            .into(),
                    );
                }

                let type_code = led_type
                    .as_ref()
                    .map(|t| led_type_to_code(t))
                    .transpose()?;
                let ip = resolve_device_ip(device.as_deref())?;

                let mut led_config = json!({});
                if let Some(p) = power {
                    led_config["maxpwr"] = json!(p);
                }
                let has_instance_field = type_code.is_some()
                    || count.is_some()
                    || pin.is_some()
                    || led_ma.is_some()
                    || color_order.is_some();
                if has_instance_field {
                    let mut instance = json!({});
                    if let Some(code) = type_code {
                        instance["type"] = json!(code);
                    }
                    if let Some(c) = count {
                        instance["len"] = json!(c);
                    }
                    if let Some(p) = pin {
                        instance["pin"] = json!([p]);
                    }
                    if let Some(ma) = led_ma {
                        instance["ledma"] = json!(ma);
                    }
                    if let Some(order) = color_order {
                        instance["order"] = json!(order);
                    }
                    led_config["ins"] = json!([instance]);
                }

                let payload = json!({"hw": {"led": led_config}});

                if dry_run {
                    println!("Would configure LEDs on device at {ip}:");
                    println!("{}", serde_json::to_string_pretty(&payload)?);
                } else {
                    post_device_config(&ip, &payload)?;
                    let mut changes = Vec::new();
                    if power.is_some() {
                        changes.push("power budget");
                    }
                    if led_ma.is_some() {
                        changes.push("per-LED mA");
                    }
                    if led_type.is_some() {
                        changes.push("LED type");
                    }
                    if color_order.is_some() {
                        changes.push("color order");
                    }
                    if count.is_some() {
                        changes.push("LED count");
                    }
                    if pin.is_some() {
                        changes.push("GPIO pin");
                    }
                    println!(
                        "LED settings updated on device at {ip}: {}",
                        changes.join(", ")
                    );
                }
            }
            ConfigureCommands::Diff { file, device } => {
                let ip = resolve_device_ip(device.as_deref())?;
                let content = std::fs::read_to_string(&file)?;
                let local: serde_json::Value = serde_json::from_str(&content)?;
                let device_cfg = get_device_config(&ip)?;

                let diffs = diff_json(&local, &device_cfg, "");

                if diffs.is_empty() {
                    println!("No differences found. Local file matches device configuration.");
                } else {
                    println!("Differences (local vs device):");
                    for line in &diffs {
                        println!("  {line}");
                    }
                    std::process::exit(1);
                }
            }
        },
        Commands::Update {
            version,
            platform,
            device,
            check,
            yes,
            file,
            skip_validation,
        } => {
            let ip = resolve_device_ip(device.as_deref())?;

            // Direct file upload mode
            if let Some(ref path) = file {
                let firmware = std::fs::read(path)
                    .map_err(|e| format!("Failed to read firmware file: {e}"))?;
                println!("Uploading {} ({} bytes) to device at {ip}...", path, firmware.len());
                if !yes {
                    print!("Proceed? [y/N]: ");
                    use std::io::Write;
                    std::io::stdout().flush()?;
                    let mut input = String::new();
                    std::io::stdin().read_line(&mut input)?;
                    if input.trim() != "y" && input.trim() != "Y" {
                        println!("Update aborted.");
                        return Ok(());
                    }
                }
                // Local files lack WLED compatibility metadata, so always skip validation
                upload_firmware(&ip, firmware, true).map_err(|e| e.message)?;
                println!("\nFirmware upload complete! Device will reboot automatically.");
                return Ok(());
            }

            // Get device info for current version and architecture
            println!("Querying device at {ip}...");
            let info = get_device_info(&ip)?;
            let current_ver = info["ver"].as_str().unwrap_or("unknown");
            let arch = info["arch"].as_str().unwrap_or("");

            // Use 'release' field first (more specific, e.g. "ESP02"), fall back to 'arch'
            let release = info["release"].as_str().unwrap_or("");
            let target_platform = match &platform {
                Some(p) => p.clone(),
                None => {
                    if !release.is_empty() {
                        release.to_string()
                    } else if !arch.is_empty() {
                        arch_to_default_platform(arch).to_string()
                    } else {
                        return Err(
                            "Could not detect device platform. Use --platform to specify it."
                                .into(),
                        );
                    }
                }
            };

            println!("  Current version: {current_ver}");
            println!("  Platform:        {target_platform}");

            // Fetch release info from GitHub
            println!("\nFetching release info from GitHub...");
            let gh_release = match &version {
                Some(v) => get_wled_release_by_tag(v)?,
                None => get_latest_wled_release()?,
            };

            let tag = gh_release["tag_name"].as_str().unwrap_or("unknown");
            let release_ver = tag.trim_start_matches('v');
            println!("  Target version:  {release_ver}");

            if release_ver == current_ver {
                println!("\nDevice is already running version {current_ver}. Nothing to do.");
                return Ok(());
            }

            let (asset_name, download_url) =
                find_firmware_asset(&gh_release, &target_platform, false)?;
            println!("  Firmware binary:  {asset_name}");

            if check {
                println!("\nRun without --check to download and install the firmware.");
                return Ok(());
            }

            if dry_run {
                println!("\nWould download {asset_name} and upload to device at {ip}");
                println!("  {current_ver} -> {release_ver}");
                return Ok(());
            }

            // Prompt for confirmation unless --yes was given
            if !yes {
                print!("Proceed with update? [y/N]: ");
                use std::io::Write;
                std::io::stdout().flush()?;
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                let trimmed = input.trim();
                if trimmed != "y" && trimmed != "Y" {
                    println!("Update aborted.");
                    return Ok(());
                }
            }

            // Check if OTA is locked before downloading
            let device_cfg = get_device_config(&ip)?;
            if device_cfg["ota"]["lock"].as_bool().unwrap_or(false) {
                return Err(
                    "OTA is locked on this device. Unlock it first with:\n  \
                     wld config ota --unlock --password <ota-password>\n  \
                     The default WLED OTA password is \"wledota\"."
                        .into(),
                );
            }

            // Download and upload firmware, retrying with .bin.gz if not enough space
            let mut current_asset = asset_name;
            let mut current_url = download_url;
            loop {
                println!("\nDownloading {current_asset}...");
                let firmware = download_firmware(&current_url)?;
                println!("  Downloaded {} bytes", firmware.len());

                println!("Uploading firmware to device at {ip}...");
                match upload_firmware(&ip, firmware, skip_validation) {
                    Ok(()) => break,
                    Err(e) if e.not_enough_space && !current_asset.ends_with(".bin.gz") => {
                        println!("  Not enough space — retrying with compressed firmware...");
                        let (gz_name, gz_url) =
                            find_firmware_asset(&gh_release, &target_platform, true)?;
                        if gz_name == current_asset {
                            return Err(e.message.into());
                        }
                        current_asset = gz_name;
                        current_url = gz_url;
                    }
                    Err(e) => return Err(e.message.into()),
                }
            }
            println!(
                "\nFirmware update complete! Device is updating from {current_ver} to {release_ver}."
            );
            println!("The device will reboot automatically. This may take up to 30 seconds.");
        }
        Commands::Completions { shell } => {
            clap_complete::generate(
                shell,
                &mut Cli::command(),
                "wld",
                &mut std::io::stdout(),
            );
        }
    }

    Ok(())
}
