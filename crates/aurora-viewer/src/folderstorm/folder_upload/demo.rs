//! `AURORA_DEMO_FOLDER_UPLOAD=preview|confirm|upload`: a small disk folder
//! for the folder upload, built below the ignored `.scratch` (docs/TESTING.md).

use std::path::{Path, PathBuf};

/// The demo's step, from the variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The folder in the uploader's Folder preview.
    Preview,
    /// The uploader's confirmation with Folderstorm's block.
    Confirm,
    /// Confirmed and run on the offline demo server, then the receipt.
    Upload,
}

impl Mode {
    pub fn from_env() -> Option<Self> {
        Self::parse(&std::env::var("AURORA_DEMO_FOLDER_UPLOAD").ok()?)
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "1" | "preview" => Some(Mode::Preview),
            "confirm" => Some(Mode::Confirm),
            "upload" => Some(Mode::Upload),
            _ => None,
        }
    }
}

/// The name of the demo's disk folder; the demo also puts a folder of that
/// name in the inventory, to show the name warning.
pub const FOLDER: &str = "Creator Kit";

/// Writes the demo's folder and returns it: textures and a sound in nested
/// folders, an animation, two files that get the same item name, a text
/// file (not uploaded), a folder holding only that text and an empty one
/// (not created), and a system file (not even listed).
pub fn fixtures(base: &Path) -> Result<PathBuf, String> {
    let root = base.join(FOLDER);
    for directory in ["Fabrics", "Sounds", "Animations", "Notes", "Empty"] {
        std::fs::create_dir_all(root.join(directory)).map_err(|e| e.to_string())?;
    }
    let texture = |path: PathBuf, colour: [u8; 3], size: u32| -> Result<(), String> {
        let mut pixels = Vec::with_capacity((size * size * 3) as usize);
        for y in 0..size {
            for x in 0..size {
                let light = if (x / 16 + y / 16) % 2 == 0 { 255 } else { 190 };
                pixels.extend(colour.map(|c| (u16::from(c) * light / 255) as u8));
            }
        }
        image::save_buffer(&path, &pixels, size, size, image::ColorType::Rgb8).map_err(|e| e.to_string())
    };
    texture(root.join("Cover.png"), [120, 65, 220], 256)?;
    texture(root.join("Fabrics/Wood.png"), [150, 100, 60], 128)?;
    texture(root.join("Fabrics/Wood.tga"), [170, 120, 70], 128)?;
    texture(root.join("Fabrics/Linen.png"), [220, 210, 190], 128)?;
    std::fs::write(root.join("Sounds/Chime.wav"), wav()).map_err(|e| e.to_string())?;
    let animation = aurora_assets::import_animation::encode(&crate::demo::idle_animation()).map_err(|e| e.to_string())?;
    std::fs::write(root.join("Animations/Idle.anim"), animation).map_err(|e| e.to_string())?;
    std::fs::write(root.join("Notes/Readme.txt"), b"Not an uploadable asset").map_err(|e| e.to_string())?;
    std::fs::write(root.join("Thumbs.db"), b"system file").map_err(|e| e.to_string())?;
    Ok(root)
}

/// One second of a 16-bit mono 44.1 kHz chime.
fn wav() -> Vec<u8> {
    let samples: Vec<i16> = (0..44_100)
        .map(|i| {
            let time = i as f32 / 44_100.0;
            ((time * 660.0 * std::f32::consts::TAU).sin() * (1.0 - time) * 0.6 * f32::from(i16::MAX)) as i16
        })
        .collect();
    let length = samples.len() as u32 * 2;
    let mut wav = b"RIFF".to_vec();
    wav.extend((length + 36).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(44_100u32.to_le_bytes());
    wav.extend((44_100u32 * 2).to_le_bytes());
    wav.extend(2u16.to_le_bytes());
    wav.extend(16u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend(length.to_le_bytes());
    for sample in samples {
        wav.extend(sample.to_le_bytes());
    }
    wav
}
