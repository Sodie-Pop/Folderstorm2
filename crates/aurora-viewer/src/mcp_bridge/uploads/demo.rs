//! Folderstorm: `AURORA_DEMO_MCP=uploads` and `camera-get`
//! (folderstorm/plans/mcp-uploads.md): a small folder written under
//! `.scratch/`, read with `camera_get`, previewed, executed (asked: it
//! costs the demo's L$) and its status logged.

use folderstorm_mcp_bridge::protocol::camera_read::GET;
use folderstorm_mcp_bridge::protocol::uploads::{EXECUTE, PREVIEW, STATUS};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// The demo folder's name (the new inventory folder's too).
pub const FOLDER: &str = "Assistant upload demo";

/// `AURORA_DEMO_MCP=uploads`.
pub fn wanted() -> bool {
    std::env::var("AURORA_DEMO_MCP").is_ok_and(|v| v == "uploads")
}

/// `AURORA_DEMO_MCP=camera-get`.
pub fn camera_only() -> bool {
    std::env::var("AURORA_DEMO_MCP").is_ok_and(|v| v == "camera-get")
}

fn png(path: &Path, rgb: [u8; 3]) -> std::io::Result<()> {
    let image = image::RgbImage::from_pixel(64, 64, image::Rgb(rgb));
    image.save(path).map_err(std::io::Error::other)
}

/// Half a second of a 440 Hz tone, 16-bit mono at 44.1 kHz.
pub fn wav_bytes() -> Vec<u8> {
    let rate = 44_100u32;
    let samples: Vec<i16> = (0..rate / 2)
        .map(|i| ((i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 8000.0) as i16)
        .collect();
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// The demo's folder under `base`: two textures in `Textures/` and a sound.
pub fn fixtures(base: &Path) -> std::io::Result<PathBuf> {
    let root = base.join(FOLDER);
    std::fs::create_dir_all(root.join("Textures"))?;
    png(&root.join("Textures").join("Red square.png"), [200, 40, 40])?;
    png(&root.join("Textures").join("Blue square.png"), [40, 60, 200])?;
    std::fs::write(root.join("Chime.wav"), wav_bytes())?;
    Ok(root)
}

/// The first step.
pub fn camera() -> (&'static str, Value) {
    (GET, json!({}))
}

/// The preview of the demo folder into My Inventory.
pub fn preview(folder: &Path) -> (&'static str, Value) {
    let absolute = std::path::absolute(folder).unwrap_or_else(|_| folder.to_owned());
    (PREVIEW, json!({"directory": absolute.display().to_string(), "path": ""}))
}

/// The execute of the previewed plan.
pub fn execute_after(preview: &Value) -> Option<(&'static str, Value)> {
    let plan = preview["plan_id"].as_str()?;
    Some((EXECUTE, json!({"plan_id": plan, "client_request_id": "demo-upload-1"})))
}

/// The status of the plan once executed.
pub fn status(plan: &str) -> (&'static str, Value) {
    (STATUS, json!({"plan_id": plan}))
}
