use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    schemars, tool, tool_handler, tool_router,
    transport::stdio,
    ErrorData as McpError, ServerHandler, ServiceExt,
};

use std::time::Duration;

use crate::config::Config;
use crate::{
    get_device_config, get_device_info, get_device_presets, get_device_state, get_device_status,
    parse_color, post_device_state, set_device_brightness, set_device_power, DeviceStatus,
};
use serde_json::json;

const DEVICE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct EmptyParams {}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct WledDeviceParams {
    /// Device name or IP address (optional - if not specified, the default device is used)
    pub device: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct WledBrightnessParams {
    /// Brightness level (0-255)
    pub value: u8,
    /// Device name or IP address (optional - if not specified, the default device is used)
    pub device: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct WledSegmentSetParams {
    /// Segment ID (0-based)
    pub id: u8,
    /// First LED index (inclusive)
    pub start: Option<u16>,
    /// Last LED index (exclusive)
    pub stop: Option<u16>,
    /// Primary color as "R,G,B" string (e.g. "255,0,0") or "#RRGGBB"
    pub color: Option<String>,
    /// Effect ID
    pub effect: Option<u8>,
    /// Effect speed (0-255)
    pub speed: Option<u8>,
    /// Effect intensity (0-255)
    pub intensity: Option<u8>,
    /// Color palette ID
    pub palette: Option<u8>,
    /// Segment brightness (0-255)
    pub brightness: Option<u8>,
    /// Turn segment on (true) or off (false)
    pub on: Option<bool>,
    /// Device name or IP address (optional)
    pub device: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct WledPresetIdParams {
    /// Preset ID
    pub id: u16,
    /// Device name or IP address (optional)
    pub device: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct WledPresetSaveParams {
    /// Preset ID (1-250)
    pub id: u16,
    /// Preset name
    pub name: Option<String>,
    /// Device name or IP address (optional)
    pub device: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct WledConfigDiffParams {
    /// Expected configuration as a JSON string
    pub expected_json: String,
    /// Device name or IP address (optional)
    pub device: Option<String>,
}

fn diff_json_mcp(
    local: &serde_json::Value,
    device: &serde_json::Value,
    prefix: &str,
) -> Vec<String> {
    let mut lines = Vec::new();
    match (local, device) {
        (serde_json::Value::Object(lmap), serde_json::Value::Object(dmap)) => {
            for (k, lv) in lmap {
                let key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                if let Some(dv) = dmap.get(k) {
                    lines.extend(diff_json_mcp(lv, dv, &key));
                } else {
                    lines.push(format!("+ {key}: {lv}"));
                }
            }
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

#[derive(Clone)]
pub struct WledMcpServer {
    tool_router: ToolRouter<WledMcpServer>,
}

#[tool_router]
impl WledMcpServer {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "List saved WLED devices from configuration")]
    async fn wled_devices(
        &self,
        Parameters(_params): Parameters<EmptyParams>,
    ) -> Result<CallToolResult, McpError> {
        match Config::load() {
            Ok(config) => {
                if config.devices.is_empty() {
                    return Ok(CallToolResult::success(vec![Content::text(
                        "No devices saved",
                    )]));
                }

                let mut output = String::from("Saved devices:\n");
                for (name, ip) in &config.devices {
                    let default_marker = if config.default_device.as_ref() == Some(name) {
                        " (default)"
                    } else {
                        ""
                    };
                    output.push_str(&format!("  {name} - {ip}{default_marker}\n"));
                }
                Ok(CallToolResult::success(vec![Content::text(output)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to load configuration: {e}"
            ))])),
        }
    }

    #[tool(
        description = "Turn WLED device on. By default, the default device is used, but you can optionally specify a device name or IP address."
    )]
    async fn wled_on(
        &self,
        Parameters(params): Parameters<WledDeviceParams>,
    ) -> Result<CallToolResult, McpError> {
        let device = params.device.clone();
        match tokio::time::timeout(
            DEVICE_TIMEOUT,
            tokio::task::spawn_blocking(move || {
                set_device_power(device.as_deref(), true).map_err(|e| e.to_string())
            }),
        )
        .await
        {
            Ok(Ok(Ok(()))) => Ok(CallToolResult::success(vec![Content::text(
                "Device turned on successfully",
            )])),
            Ok(Ok(Err(e))) => Ok(CallToolResult::error(vec![Content::text(e)])),
            Ok(Err(e)) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Task error: {e}"
            ))])),
            Err(_) => Ok(CallToolResult::error(vec![Content::text(
                "Operation timed out while communicating with device",
            )])),
        }
    }

    #[tool(
        description = "Turn WLED device off. By default, the default device is used, but you can optionally specify a device name or IP address."
    )]
    async fn wled_off(
        &self,
        Parameters(params): Parameters<WledDeviceParams>,
    ) -> Result<CallToolResult, McpError> {
        let device = params.device.clone();
        match tokio::time::timeout(
            DEVICE_TIMEOUT,
            tokio::task::spawn_blocking(move || {
                set_device_power(device.as_deref(), false).map_err(|e| e.to_string())
            }),
        )
        .await
        {
            Ok(Ok(Ok(()))) => Ok(CallToolResult::success(vec![Content::text(
                "Device turned off successfully",
            )])),
            Ok(Ok(Err(e))) => Ok(CallToolResult::error(vec![Content::text(e)])),
            Ok(Err(e)) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Task error: {e}"
            ))])),
            Err(_) => Ok(CallToolResult::error(vec![Content::text(
                "Operation timed out while communicating with device",
            )])),
        }
    }

    #[tool(
        description = "Set WLED device brightness (0-255). By default, the default device is used, but you can optionally specify a device name or IP address."
    )]
    async fn wled_brightness(
        &self,
        Parameters(params): Parameters<WledBrightnessParams>,
    ) -> Result<CallToolResult, McpError> {
        let device = params.device.clone();
        let value = params.value;
        match tokio::time::timeout(
            DEVICE_TIMEOUT,
            tokio::task::spawn_blocking(move || {
                set_device_brightness(device.as_deref(), value).map_err(|e| e.to_string())
            }),
        )
        .await
        {
            Ok(Ok(Ok(()))) => Ok(CallToolResult::success(vec![Content::text(format!(
                "Device brightness set to {value} successfully"
            ))])),
            Ok(Ok(Err(e))) => Ok(CallToolResult::error(vec![Content::text(e)])),
            Ok(Err(e)) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Task error: {e}"
            ))])),
            Err(_) => Ok(CallToolResult::error(vec![Content::text(
                "Operation timed out while communicating with device",
            )])),
        }
    }

    #[tool(description = "List all segments on the WLED device")]
    async fn wled_segment_list(
        &self,
        Parameters(params): Parameters<WledDeviceParams>,
    ) -> Result<CallToolResult, McpError> {
        let device = params.device.clone();
        match tokio::time::timeout(
            DEVICE_TIMEOUT,
            tokio::task::spawn_blocking(move || -> Result<String, String> {
                let config = Config::load().map_err(|e| e.to_string())?;
                let ip = config.get_device_ip(device.as_deref()).map_err(|e| e.to_string())?;
                let state = get_device_state(&ip).map_err(|e| e.to_string())?;
                let segments = match state["seg"].as_array() {
                    Some(s) => s.clone(),
                    None => return Ok("No segments found in device state".to_string()),
                };
                if segments.is_empty() {
                    return Ok("No segments configured".to_string());
                }
                let mut output = format!("Segments on device at {ip}:\n\n");
                for seg in &segments {
                    let id = seg["id"].as_u64().unwrap_or(0);
                    let start = seg["start"].as_u64().unwrap_or(0);
                    let stop = seg["stop"].as_u64().unwrap_or(0);
                    let on = seg["on"].as_bool().unwrap_or(false);
                    let bri = seg["bri"].as_u64().unwrap_or(0);
                    let fx = seg["fx"].as_u64().unwrap_or(0);
                    let sx = seg["sx"].as_u64().unwrap_or(0);
                    let ix = seg["ix"].as_u64().unwrap_or(0);
                    let pal = seg["pal"].as_u64().unwrap_or(0);
                    let status = if on { "ON" } else { "OFF" };
                    output.push_str(&format!("  Segment {id}: LEDs {start}-{stop} ({status})\n"));
                    output.push_str(&format!(
                        "    brightness={bri} effect={fx} speed={sx} intensity={ix} palette={pal}\n"
                    ));
                }
                Ok(output)
            }),
        )
        .await
        {
            Ok(Ok(Ok(output))) => Ok(CallToolResult::success(vec![Content::text(output)])),
            Ok(Ok(Err(e))) => Ok(CallToolResult::error(vec![Content::text(e)])),
            Ok(Err(e)) => Ok(CallToolResult::error(vec![Content::text(format!("Task error: {e}"))])),
            Err(_) => Ok(CallToolResult::error(vec![Content::text(
                "Operation timed out while communicating with device",
            )])),
        }
    }

    #[tool(description = "Set or update a WLED segment's properties")]
    async fn wled_segment_set(
        &self,
        Parameters(params): Parameters<WledSegmentSetParams>,
    ) -> Result<CallToolResult, McpError> {
        let device = params.device.clone();
        let id = params.id;
        let color_str = params.color.clone();
        let start = params.start;
        let stop = params.stop;
        let effect = params.effect;
        let speed = params.speed;
        let intensity = params.intensity;
        let palette = params.palette;
        let brightness = params.brightness;
        let on = params.on;
        match tokio::time::timeout(
            DEVICE_TIMEOUT,
            tokio::task::spawn_blocking(move || -> Result<String, String> {
                let config = Config::load().map_err(|e| e.to_string())?;
                let ip = config.get_device_ip(device.as_deref()).map_err(|e| e.to_string())?;
                let parsed_color = color_str
                    .as_ref()
                    .map(|c| parse_color(c).map_err(|e| e.to_string()))
                    .transpose()?;
                let mut seg = json!({"id": id});
                if let Some(s) = start { seg["start"] = json!(s); }
                if let Some(s) = stop { seg["stop"] = json!(s); }
                if let Some(rgb) = parsed_color { seg["col"] = json!([[rgb[0], rgb[1], rgb[2]]]); }
                if let Some(fx) = effect { seg["fx"] = json!(fx); }
                if let Some(sx) = speed { seg["sx"] = json!(sx); }
                if let Some(ix) = intensity { seg["ix"] = json!(ix); }
                if let Some(pal) = palette { seg["pal"] = json!(pal); }
                if let Some(bri) = brightness { seg["bri"] = json!(bri); }
                if let Some(o) = on { seg["on"] = json!(o); }
                let payload = json!({"seg": [seg]});
                post_device_state(&ip, &payload).map_err(|e| e.to_string())?;
                Ok(format!("Segment {id} updated on device at {ip}"))
            }),
        )
        .await
        {
            Ok(Ok(Ok(msg))) => Ok(CallToolResult::success(vec![Content::text(msg)])),
            Ok(Ok(Err(e))) => Ok(CallToolResult::error(vec![Content::text(e)])),
            Ok(Err(e)) => Ok(CallToolResult::error(vec![Content::text(format!("Task error: {e}"))])),
            Err(_) => Ok(CallToolResult::error(vec![Content::text(
                "Operation timed out while communicating with device",
            )])),
        }
    }

    #[tool(description = "List all presets saved on the WLED device")]
    async fn wled_preset_list(
        &self,
        Parameters(params): Parameters<WledDeviceParams>,
    ) -> Result<CallToolResult, McpError> {
        let device = params.device.clone();
        match tokio::time::timeout(
            DEVICE_TIMEOUT,
            tokio::task::spawn_blocking(move || -> Result<String, String> {
                let config = Config::load().map_err(|e| e.to_string())?;
                let ip = config.get_device_ip(device.as_deref()).map_err(|e| e.to_string())?;
                let presets = get_device_presets(&ip).map_err(|e| e.to_string())?;
                let obj = presets.as_object().ok_or("Invalid presets response from device")?;
                let mut output = String::new();
                let mut found = false;
                for (key, value) in obj {
                    if key.parse::<u16>().is_err() { continue; }
                    found = true;
                    let name = value["n"].as_str().unwrap_or("(unnamed)");
                    let on = value["on"].as_bool();
                    let bri = value["bri"].as_u64();
                    let mut details = Vec::new();
                    if let Some(o) = on { details.push(if o { "on".to_string() } else { "off".to_string() }); }
                    if let Some(b) = bri { details.push(format!("brightness={b}")); }
                    let detail_str = if details.is_empty() { String::new() } else { format!(" ({})", details.join(", ")) };
                    output.push_str(&format!("  Preset {key}: {name}{detail_str}\n"));
                }
                if !found {
                    output = format!("No presets saved on device at {ip}");
                }
                Ok(output)
            }),
        )
        .await
        {
            Ok(Ok(Ok(output))) => Ok(CallToolResult::success(vec![Content::text(output)])),
            Ok(Ok(Err(e))) => Ok(CallToolResult::error(vec![Content::text(e)])),
            Ok(Err(e)) => Ok(CallToolResult::error(vec![Content::text(format!("Task error: {e}"))])),
            Err(_) => Ok(CallToolResult::error(vec![Content::text(
                "Operation timed out while communicating with device",
            )])),
        }
    }

    #[tool(description = "Save the current WLED state as a preset")]
    async fn wled_preset_save(
        &self,
        Parameters(params): Parameters<WledPresetSaveParams>,
    ) -> Result<CallToolResult, McpError> {
        let device = params.device.clone();
        let id = params.id;
        let name = params.name.clone();
        match tokio::time::timeout(
            DEVICE_TIMEOUT,
            tokio::task::spawn_blocking(move || -> Result<String, String> {
                if id == 0 || id > 250 {
                    return Err("Preset ID must be between 1 and 250".to_string());
                }
                let config = Config::load().map_err(|e| e.to_string())?;
                let ip = config.get_device_ip(device.as_deref()).map_err(|e| e.to_string())?;
                let mut payload = json!({"psave": id});
                if let Some(ref n) = name { payload["n"] = json!(n); }
                post_device_state(&ip, &payload).map_err(|e| e.to_string())?;
                let display_name = name.as_deref().unwrap_or("(unnamed)");
                Ok(format!("Saved current state as preset {id} ({display_name}) on device at {ip}"))
            }),
        )
        .await
        {
            Ok(Ok(Ok(msg))) => Ok(CallToolResult::success(vec![Content::text(msg)])),
            Ok(Ok(Err(e))) => Ok(CallToolResult::error(vec![Content::text(e)])),
            Ok(Err(e)) => Ok(CallToolResult::error(vec![Content::text(format!("Task error: {e}"))])),
            Err(_) => Ok(CallToolResult::error(vec![Content::text(
                "Operation timed out while communicating with device",
            )])),
        }
    }

    #[tool(description = "Load a preset on the WLED device")]
    async fn wled_preset_load(
        &self,
        Parameters(params): Parameters<WledPresetIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let device = params.device.clone();
        let id = params.id;
        match tokio::time::timeout(
            DEVICE_TIMEOUT,
            tokio::task::spawn_blocking(move || -> Result<String, String> {
                let config = Config::load().map_err(|e| e.to_string())?;
                let ip = config.get_device_ip(device.as_deref()).map_err(|e| e.to_string())?;
                let payload = json!({"ps": id});
                post_device_state(&ip, &payload).map_err(|e| e.to_string())?;
                Ok(format!("Loaded preset {id} on device at {ip}"))
            }),
        )
        .await
        {
            Ok(Ok(Ok(msg))) => Ok(CallToolResult::success(vec![Content::text(msg)])),
            Ok(Ok(Err(e))) => Ok(CallToolResult::error(vec![Content::text(e)])),
            Ok(Err(e)) => Ok(CallToolResult::error(vec![Content::text(format!("Task error: {e}"))])),
            Err(_) => Ok(CallToolResult::error(vec![Content::text(
                "Operation timed out while communicating with device",
            )])),
        }
    }

    #[tool(description = "Get device info (version, memory, uptime, WiFi, LED stats) as formatted text")]
    async fn wled_debug_info(
        &self,
        Parameters(params): Parameters<WledDeviceParams>,
    ) -> Result<CallToolResult, McpError> {
        let device = params.device.clone();
        match tokio::time::timeout(
            DEVICE_TIMEOUT,
            tokio::task::spawn_blocking(move || -> Result<String, String> {
                let config = Config::load().map_err(|e| e.to_string())?;
                let ip = config.get_device_ip(device.as_deref()).map_err(|e| e.to_string())?;
                let info = get_device_info(&ip).map_err(|e| e.to_string())?;
                let mut output = format!("Device info for {ip}:\n\n");
                if let Some(name) = info["name"].as_str() { output.push_str(&format!("  Name:       {name}\n")); }
                if let Some(ver) = info["ver"].as_str() { output.push_str(&format!("  Version:    {ver}\n")); }
                if let Some(vid) = info["vid"].as_u64() { output.push_str(&format!("  Build ID:   {vid}\n")); }
                if let Some(mac) = info["mac"].as_str() { output.push_str(&format!("  MAC:        {mac}\n")); }
                if let Some(uptime) = info["uptime"].as_u64() {
                    let h = uptime / 3600;
                    let m = (uptime % 3600) / 60;
                    let s = uptime % 60;
                    output.push_str(&format!("  Uptime:     {h}h {m}m {s}s\n"));
                }
                if let Some(heap) = info["freeheap"].as_u64() {
                    let warn = if heap < 10000 { " (LOW!)" } else { "" };
                    output.push_str(&format!("  Free heap:  {heap} bytes{warn}\n"));
                }
                if let Some(wifi) = info.get("wifi") {
                    if let Some(signal) = wifi["signal"].as_i64() {
                        let quality = match signal { 80..=100 => "excellent", 60..=79 => "good", 40..=59 => "fair", _ => "poor" };
                        output.push_str(&format!("  WiFi:       {signal}% ({quality})\n"));
                    }
                }
                if let Some(leds) = info.get("leds") {
                    if let Some(count) = leds["count"].as_u64() { output.push_str(&format!("  LEDs:       {count}\n")); }
                    if let Some(fps) = leds["fps"].as_u64() { output.push_str(&format!("  FPS:        {fps}\n")); }
                    if let Some(pwr) = leds["pwr"].as_u64() { output.push_str(&format!("  Power:      {pwr} mA\n")); }
                }
                Ok(output)
            }),
        )
        .await
        {
            Ok(Ok(Ok(output))) => Ok(CallToolResult::success(vec![Content::text(output)])),
            Ok(Ok(Err(e))) => Ok(CallToolResult::error(vec![Content::text(e)])),
            Ok(Err(e)) => Ok(CallToolResult::error(vec![Content::text(format!("Task error: {e}"))])),
            Err(_) => Ok(CallToolResult::error(vec![Content::text(
                "Operation timed out while communicating with device",
            )])),
        }
    }

    #[tool(description = "Export the full WLED device configuration as a JSON string")]
    async fn wled_config_export(
        &self,
        Parameters(params): Parameters<WledDeviceParams>,
    ) -> Result<CallToolResult, McpError> {
        let device = params.device.clone();
        match tokio::time::timeout(
            DEVICE_TIMEOUT,
            tokio::task::spawn_blocking(move || -> Result<String, String> {
                let config = Config::load().map_err(|e| e.to_string())?;
                let ip = config.get_device_ip(device.as_deref()).map_err(|e| e.to_string())?;
                let cfg = get_device_config(&ip).map_err(|e| e.to_string())?;
                serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())
            }),
        )
        .await
        {
            Ok(Ok(Ok(output))) => Ok(CallToolResult::success(vec![Content::text(output)])),
            Ok(Ok(Err(e))) => Ok(CallToolResult::error(vec![Content::text(e)])),
            Ok(Err(e)) => Ok(CallToolResult::error(vec![Content::text(format!("Task error: {e}"))])),
            Err(_) => Ok(CallToolResult::error(vec![Content::text(
                "Operation timed out while communicating with device",
            )])),
        }
    }

    #[tool(description = "Compare an expected JSON config against the device config and return diff text")]
    async fn wled_config_diff(
        &self,
        Parameters(params): Parameters<WledConfigDiffParams>,
    ) -> Result<CallToolResult, McpError> {
        let device = params.device.clone();
        let expected_json = params.expected_json.clone();
        match tokio::time::timeout(
            DEVICE_TIMEOUT,
            tokio::task::spawn_blocking(move || -> Result<String, String> {
                let local: serde_json::Value =
                    serde_json::from_str(&expected_json).map_err(|e| e.to_string())?;
                let config = Config::load().map_err(|e| e.to_string())?;
                let ip = config.get_device_ip(device.as_deref()).map_err(|e| e.to_string())?;
                let device_cfg = get_device_config(&ip).map_err(|e| e.to_string())?;
                let diffs = diff_json_mcp(&local, &device_cfg, "");
                if diffs.is_empty() {
                    Ok("No differences found. Local config matches device configuration.".to_string())
                } else {
                    let mut output = String::from("Differences (local vs device):\n");
                    for line in &diffs {
                        output.push_str(&format!("  {line}\n"));
                    }
                    Ok(output)
                }
            }),
        )
        .await
        {
            Ok(Ok(Ok(output))) => Ok(CallToolResult::success(vec![Content::text(output)])),
            Ok(Ok(Err(e))) => Ok(CallToolResult::error(vec![Content::text(e)])),
            Ok(Err(e)) => Ok(CallToolResult::error(vec![Content::text(format!("Task error: {e}"))])),
            Err(_) => Ok(CallToolResult::error(vec![Content::text(
                "Operation timed out while communicating with device",
            )])),
        }
    }

    #[tool(description = "Check status of all configured WLED devices")]
    async fn wled_status(
        &self,
        Parameters(_params): Parameters<EmptyParams>,
    ) -> Result<CallToolResult, McpError> {
        match tokio::time::timeout(
            DEVICE_TIMEOUT,
            tokio::task::spawn_blocking(|| -> Result<String, String> {
                let config = Config::load().map_err(|e| e.to_string())?;

                if config.devices.is_empty() {
                    return Ok("No devices saved".to_string());
                }

                let mut output = String::from("Checking status of all devices:\n\n");
                let mut all_reachable = true;

                for (name, ip) in &config.devices {
                    let default_marker = if config.default_device.as_ref() == Some(name) {
                        " (default)"
                    } else {
                        ""
                    };

                    output.push_str(&format!("  {name} ({ip}){default_marker}: "));

                    match get_device_status(ip) {
                        DeviceStatus::On => {
                            output.push_str("ON\n");
                        }
                        DeviceStatus::Off => {
                            output.push_str("OFF\n");
                        }
                        DeviceStatus::Unreachable => {
                            output.push_str("UNREACHABLE\n");
                            all_reachable = false;
                        }
                    }
                }

                if !all_reachable {
                    output.push_str("\nWarning: Some devices are unreachable");
                }

                Ok(output)
            }),
        )
        .await
        {
            Ok(Ok(Ok(output))) => Ok(CallToolResult::success(vec![Content::text(output)])),
            Ok(Ok(Err(e))) => Ok(CallToolResult::error(vec![Content::text(e)])),
            Ok(Err(e)) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Task error: {e}"
            ))])),
            Err(_) => Ok(CallToolResult::error(vec![Content::text(
                "Operation timed out while communicating with device",
            )])),
        }
    }
}

#[tool_handler]
impl ServerHandler for WledMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2025_03_26,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation::from_build_env(),
            instructions: None,
        }
    }
}

pub fn handle_mcp_command() -> Result<(), Box<dyn std::error::Error>> {
    // Set up tracing for the MCP server
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::DEBUG.into()),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    // Create the MCP server
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        tracing::info!("Starting WLED MCP server");

        let service = WledMcpServer::new().serve(stdio()).await?;
        service.waiting().await?;
        Ok(())
    })
}
