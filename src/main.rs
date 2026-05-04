mod config;

#[cfg(feature = "mcp")]
mod mcp;

use clap::{Parser, Subcommand};
use config::{validate_device_address, Config};
use serde_json::json;
use std::time::Duration;
use wled_json_api_library::structures::state::State;
use wled_json_api_library::wled::Wled;

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

fn parse_color(s: &str) -> Result<[u8; 3], Box<dyn std::error::Error>> {
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
#[command(about = "Control WLED lights from your terminal", long_about = None)]
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
    Add {
        /// Name for the device
        name: String,
        /// IP address of the device
        ip: String,
    },
    /// Delete a saved device
    Delete {
        /// Name of the device to delete
        name: String,
    },
    /// List all saved devices
    Ls,
    /// Set the default device
    SetDefault {
        /// Name of the device to set as default
        name: String,
    },
    /// Turn device on
    On {
        /// Device name or IP (uses default if not specified)
        #[arg(short, long)]
        device: Option<String>,
    },
    /// Turn device off
    Off {
        /// Device name or IP (uses default if not specified)
        #[arg(short, long)]
        device: Option<String>,
    },
    /// Start a MCP (Model Context Protocol) server for controlling WLED devices
    #[cfg(feature = "mcp")]
    Mcp,
    /// Set device brightness (0-255)
    Brightness {
        /// Brightness level (0-255, or 0-100 if --percentage is used)
        value: u8,
        /// Device name or IP (uses default if not specified)
        #[arg(short, long)]
        device: Option<String>,
        /// Interpret value as a percentage (0-100) instead of 0-255
        #[arg(short, long)]
        percentage: bool,
    },
    /// Check status of all configured devices
    Status,
    /// Configure device settings (WiFi, OTA, LEDs)
    #[command(name = "config")]
    Configure {
        #[command(subcommand)]
        subcommand: ConfigureCommands,
    },
    /// Manage LED segments
    Segment {
        #[command(subcommand)]
        subcommand: SegmentCommands,
    },
    /// Manage presets
    Preset {
        #[command(subcommand)]
        subcommand: PresetCommands,
    },
}

#[derive(Subcommand)]
enum ConfigureCommands {
    /// Export full device configuration to a JSON file
    Export {
        /// Device name or IP (uses default if not specified)
        #[arg(short, long)]
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
        #[arg(short, long)]
        device: Option<String>,
    },
    /// Configure WiFi settings
    Wifi {
        /// WiFi network name (SSID)
        #[arg(long)]
        ssid: String,
        /// WiFi password
        #[arg(long)]
        password: String,
        /// Device name or IP (uses default if not specified)
        #[arg(short, long)]
        device: Option<String>,
    },
    /// Configure OTA (Over-The-Air) update settings
    Ota {
        /// Lock OTA updates to prevent firmware changes
        #[arg(long, conflicts_with = "unlock")]
        lock: bool,
        /// Unlock OTA updates to allow firmware changes
        #[arg(long, conflicts_with = "lock")]
        unlock: bool,
        /// Set OTA password
        #[arg(long)]
        password: Option<String>,
        /// Device name or IP (uses default if not specified)
        #[arg(short, long)]
        device: Option<String>,
    },
    /// Configure LED strip settings
    Led {
        /// Maximum power budget in milliamps (e.g. 850)
        #[arg(long)]
        power: Option<u32>,
        /// LED strip type (WS2812B, SK6812, TM1814, WS2801, APA102, LPD8806, P9813, or numeric code)
        #[arg(long, value_name = "TYPE")]
        led_type: Option<String>,
        /// Number of LEDs in the strip
        #[arg(long)]
        count: Option<u16>,
        /// GPIO pin number for data line
        #[arg(long)]
        pin: Option<u8>,
        /// Device name or IP (uses default if not specified)
        #[arg(short, long)]
        device: Option<String>,
    },
}

#[derive(Subcommand)]
enum SegmentCommands {
    /// List all segments on a device
    List {
        /// Device name or IP (uses default if not specified)
        #[arg(short, long)]
        device: Option<String>,
    },
    /// Create or modify a segment
    Set {
        /// Segment ID (0-based)
        #[arg(long)]
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
        #[arg(short, long)]
        device: Option<String>,
    },
    /// Delete a segment
    Delete {
        /// Segment ID to delete
        #[arg(long)]
        id: u8,
        /// Device name or IP (uses default if not specified)
        #[arg(short, long)]
        device: Option<String>,
    },
}

#[derive(Subcommand)]
enum PresetCommands {
    /// List all presets on a device
    List {
        /// Device name or IP (uses default if not specified)
        #[arg(short, long)]
        device: Option<String>,
    },
    /// Save current state as a preset
    Save {
        /// Preset ID (1-250)
        #[arg(long)]
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
        #[arg(short, long)]
        device: Option<String>,
    },
    /// Load a preset
    Load {
        /// Preset ID to load
        #[arg(long)]
        id: u16,
        /// Device name or IP (uses default if not specified)
        #[arg(short, long)]
        device: Option<String>,
    },
    /// Delete a preset
    Delete {
        /// Preset ID to delete
        #[arg(long)]
        id: u16,
        /// Device name or IP (uses default if not specified)
        #[arg(short, long)]
        device: Option<String>,
    },
}

fn main() {
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
                        println!("  Segment {id}: LEDs {start}-{stop} ({status})");
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
                device,
            } => {
                let ip = resolve_device_ip(device.as_deref())?;
                let payload = json!({
                    "nw": {"ins": [{"ssid": ssid, "psk": password}]}
                });

                if dry_run {
                    let display = json!({
                        "nw": {"ins": [{"ssid": ssid, "psk": "***"}]}
                    });
                    println!("Would configure WiFi on device at {ip}:");
                    println!("{}", serde_json::to_string_pretty(&display)?);
                } else {
                    post_device_config(&ip, &payload)?;
                    println!("WiFi configured on device at {ip}");
                    println!("Note: Device may restart to apply WiFi changes");
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
                led_type,
                count,
                pin,
                device,
            } => {
                if power.is_none() && led_type.is_none() && count.is_none() && pin.is_none() {
                    return Err(
                        "At least one option required: --power, --led-type, --count, or --pin"
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
                if type_code.is_some() || count.is_some() || pin.is_some() {
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
                    if led_type.is_some() {
                        changes.push("LED type");
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
        },
    }

    Ok(())
}
